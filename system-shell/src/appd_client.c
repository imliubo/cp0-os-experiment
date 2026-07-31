#define _GNU_SOURCE

#include "cp0_appd_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_APPD_SOCKET
#define CP0_APPD_SOCKET "/run/cardputerzero-appd/control.sock"
#endif
#define CP0_APPD_FRAME_BYTES 8192U
#define CP0_APPD_JSON_TOKENS 384U
#define CP0_APPD_PAGE_SIZE 8U

static uint64_t next_request_id = 1;

static int write_all(int descriptor, const char *buffer, size_t length)
{
    size_t offset = 0;
    while (offset < length) {
        ssize_t count = write(descriptor, buffer + offset, length - offset);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0)
            return -1;
        offset += (size_t)count;
    }
    return 0;
}

static int exchange(const char *request, size_t request_length, char *response,
                    size_t response_capacity, size_t *response_length,
                    unsigned int timeout_ms)
{
    const struct timeval timeout = {
        .tv_sec = (time_t)(timeout_ms / 1000U),
        .tv_usec = (suseconds_t)((timeout_ms % 1000U) * 1000U),
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_APPD_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2)
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        strlen(CP0_APPD_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_APPD_SOCKET, strlen(CP0_APPD_SOCKET) + 1U);
#ifdef __APPLE__
    address.sun_len = (uint8_t)address_length;
#endif
    if (connect(descriptor, (const struct sockaddr *)&address,
                address_length) != 0 ||
        write_all(descriptor, request, request_length) != 0)
        goto cleanup;

    while (length < response_capacity - 1U) {
        ssize_t count = read(descriptor, response + length,
                             response_capacity - 1U - length);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0)
            goto cleanup;
        length += (size_t)count;
        char *newline = memchr(response, '\n', length);
        if (newline != NULL) {
            if ((size_t)(newline - response) + 1U != length)
                goto cleanup;
            length--;
            response[length] = '\0';
            *response_length = length;
            result = 0;
            goto cleanup;
        }
    }

cleanup:
    if (descriptor >= 0)
        close(descriptor);
    return result;
}

static int parse_success(const char *document, size_t length,
                         uint64_t request_id,
                         struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS],
                         size_t *token_count, int *data)
{
    int count = cp0_json_parse(document, length, tokens, CP0_APPD_JSON_TOKENS);
    if (count <= 0 || tokens[0].type != CP0_JSON_OBJECT)
        return -1;
    int version = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                      "protocol_version");
    int response_id = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                          "request_id");
    int outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                      "outcome");
    uint64_t parsed_version;
    uint64_t parsed_id;
    if (version < 0 || response_id < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version], &parsed_version) ||
        !cp0_json_get_u64(document, &tokens[response_id], &parsed_id) ||
        parsed_version != 1 || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return -1;
    int status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                     "status");
    *data = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                "data");
    if (status < 0 || *data < 0 ||
        !cp0_json_string_equals(document, &tokens[status], "ok") ||
        tokens[*data].type != CP0_JSON_OBJECT)
        return -1;
    *token_count = (size_t)count;
    return 0;
}

static bool copy_member(const char *document,
                        const struct cp0_json_token *tokens, size_t token_count,
                        int object, const char *key, char *output,
                        size_t output_capacity)
{
    int value = cp0_json_object_get(document, tokens, token_count, object, key);
    return value >= 0 && cp0_json_copy_string(document, &tokens[value], output,
                                               output_capacity);
}

static bool valid_app_id(const char *app_id)
{
    unsigned int parts = 1;
    size_t part_length = 0;
    size_t length;

    if (app_id == NULL || (length = strlen(app_id)) == 0 || length > 128)
        return false;
    for (size_t index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)app_id[index];
        if (byte == '.') {
            if (part_length == 0 || part_length > 32 ||
                app_id[index - 1] == '-')
                return false;
            parts++;
            part_length = 0;
            continue;
        }
        if (part_length == 0 && (byte < 'a' || byte > 'z'))
            return false;
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= '0' && byte <= '9') || byte == '-'))
            return false;
        part_length++;
    }
    return parts >= 3 && part_length > 0 && part_length <= 32 &&
           app_id[length - 1] != '-';
}

static bool valid_document_id(const char *document_id)
{
    if (document_id == NULL || strlen(document_id) != 32)
        return false;
    for (size_t index = 0; index < 32; index++) {
        unsigned char byte = (unsigned char)document_id[index];
        if (!((byte >= '0' && byte <= '9') ||
              (byte >= 'a' && byte <= 'f')))
            return false;
    }
    return true;
}

static bool app_permission_bit(const char *document,
                               const struct cp0_json_token *token,
                               uint16_t *bit)
{
    static const struct {
        const char *name;
        uint16_t bit;
    } permissions[] = {
        {"audio.capture", CP0_APP_PERMISSION_AUDIO_CAPTURE},
        {"audio.playback", CP0_APP_PERMISSION_AUDIO_PLAYBACK},
        {"camera.capture", CP0_APP_PERMISSION_CAMERA_CAPTURE},
        {"documents.open", CP0_APP_PERMISSION_DOCUMENTS_OPEN},
        {"hardware.gpio", CP0_APP_PERMISSION_HARDWARE_GPIO},
        {"network.client", CP0_APP_PERMISSION_NETWORK_CLIENT},
        {"notifications.post", CP0_APP_PERMISSION_NOTIFICATIONS_POST},
        {"radio.lora", CP0_APP_PERMISSION_RADIO_LORA},
    };
    for (size_t index = 0; index < sizeof(permissions) / sizeof(permissions[0]);
         index++) {
        if (cp0_json_string_equals(document, token, permissions[index].name)) {
            *bit = permissions[index].bit;
            return true;
        }
    }
    return false;
}

static int parse_app_page(const char *response, size_t response_length,
                          uint64_t request_id, uint16_t offset,
                          struct cp0_app_summary *apps, size_t capacity,
                          size_t *app_count, bool *has_next,
                          uint16_t *next_offset)
{
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t token_count;
    int data;

    if (apps == NULL || app_count == NULL || has_next == NULL ||
        next_offset == NULL ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int array = cp0_json_object_get(response, tokens, token_count, data, "apps");
    int next = cp0_json_object_get(response, tokens, token_count, data,
                                   "next_offset");
    if (kind < 0 || array < 0 || next < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "applications") ||
        tokens[array].type != CP0_JSON_ARRAY ||
        tokens[array].children > CP0_APPD_PAGE_SIZE ||
        tokens[array].children > capacity)
        return -1;

    for (unsigned int index = 0; index < tokens[array].children; index++) {
        int item = cp0_json_array_get(tokens, token_count, array, index);
        struct cp0_app_summary decoded = {0};
        int running;
        int display;
        int installed_at;
        int package_bytes;
        int data_bytes;
        int permissions;
        if (item < 0 || tokens[item].type != CP0_JSON_OBJECT ||
            !copy_member(response, tokens, token_count, item, "app_id",
                         decoded.app_id, sizeof(decoded.app_id)) ||
            !valid_app_id(decoded.app_id) ||
            !copy_member(response, tokens, token_count, item, "name",
                         decoded.name, sizeof(decoded.name)) ||
            decoded.name[0] == '\0' ||
            !copy_member(response, tokens, token_count, item, "version",
                         decoded.version, sizeof(decoded.version)))
            return -1;
        running = cp0_json_object_get(response, tokens, token_count, item,
                                      "running");
        display = cp0_json_object_get(response, tokens, token_count, item,
                                      "display");
        installed_at = cp0_json_object_get(response, tokens, token_count, item,
                                           "installed_at_unix_seconds");
        package_bytes = cp0_json_object_get(response, tokens, token_count, item,
                                            "package_bytes");
        data_bytes = cp0_json_object_get(response, tokens, token_count, item,
                                         "data_bytes");
        permissions = cp0_json_object_get(response, tokens, token_count, item,
                                          "permissions");
        if (running < 0 || display < 0 || installed_at < 0 ||
            package_bytes < 0 || data_bytes < 0 || permissions < 0 ||
            !cp0_json_get_bool(response, &tokens[running], &decoded.running) ||
            !cp0_json_get_u64(response, &tokens[installed_at],
                              &decoded.installed_at_unix_seconds) ||
            !cp0_json_get_u64(response, &tokens[package_bytes],
                              &decoded.package_bytes) ||
            !cp0_json_get_u64(response, &tokens[data_bytes],
                              &decoded.data_bytes) ||
            tokens[permissions].type != CP0_JSON_ARRAY ||
            tokens[permissions].children > 8)
            return -1;
        if (cp0_json_string_equals(response, &tokens[display], "immersive")) {
            decoded.immersive = true;
        } else if (!cp0_json_string_equals(response, &tokens[display],
                                           "standard")) {
            return -1;
        }
        for (unsigned int permission_index = 0;
             permission_index < tokens[permissions].children;
             permission_index++) {
            int permission = cp0_json_array_get(tokens, token_count, permissions,
                                                permission_index);
            uint16_t bit;
            if (permission < 0 ||
                !app_permission_bit(response, &tokens[permission], &bit) ||
                (decoded.permissions & bit) != 0)
                return -1;
            decoded.permissions |= bit;
        }
        apps[index] = decoded;
    }
    *app_count = tokens[array].children;
    if (cp0_json_is_null(response, &tokens[next])) {
        *has_next = false;
        *next_offset = 0;
    } else {
        uint64_t decoded_offset;
        if (!cp0_json_get_u64(response, &tokens[next], &decoded_offset) ||
            decoded_offset > UINT16_MAX || decoded_offset <= offset)
            return -1;
        *has_next = true;
        *next_offset = (uint16_t)decoded_offset;
    }
    return 0;
}

static int list_page(uint16_t offset, struct cp0_app_summary *apps,
                     size_t capacity, size_t *app_count, bool *has_next,
                     uint16_t *next_offset)
{
    char request[256];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"list\",\"offset\":%u,\"limit\":%u}}\n",
        (unsigned long long)request_id, (unsigned int)offset,
        CP0_APPD_PAGE_SIZE);

    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 500) != 0)
        return -1;
    return parse_app_page(response, response_length, request_id, offset, apps,
                          capacity, app_count, has_next, next_offset);
}

int cp0_appd_list_apps(struct cp0_app_list *list)
{
    uint16_t offset = 0;
    struct cp0_app_list decoded = {0};

    if (list == NULL)
        return -1;
    while (decoded.count < CP0_APPD_MAX_APPS) {
        size_t count;
        bool has_next;
        uint16_t next_offset;
        if (list_page(offset, &decoded.apps[decoded.count],
                      CP0_APPD_MAX_APPS - decoded.count, &count, &has_next,
                      &next_offset) != 0)
            return -1;
        decoded.count += count;
        if (!has_next) {
            *list = decoded;
            return 0;
        }
        if (count == 0)
            return -1;
        offset = next_offset;
    }
    decoded.truncated = true;
    *list = decoded;
    return 0;
}

static int parse_lifecycle_response(const char *response,
                                    size_t response_length,
                                    uint64_t request_id,
                                    const char *expected_kind,
                                    const char *app_id)
{
    char response_app_id[CP0_APP_ID_BYTES];
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t token_count;
    int data;

    if (!valid_app_id(app_id) ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    return kind >= 0 &&
                   cp0_json_string_equals(response, &tokens[kind],
                                          expected_kind) &&
                   copy_member(response, tokens, token_count, data, "app_id",
                               response_app_id, sizeof(response_app_id)) &&
                   strcmp(response_app_id, app_id) == 0
               ? 0
               : -1;
}

static int parse_uninstall_response(const char *response,
                                    size_t response_length,
                                    uint64_t request_id,
                                    const char *app_id)
{
    char response_app_id[CP0_APP_ID_BYTES];
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t token_count;
    int data;
    bool retained;
    bool cleanup_pending;
    if (!valid_app_id(app_id) ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int retained_token = cp0_json_object_get(
        response, tokens, token_count, data, "private_data_retained");
    int cleanup_token = cp0_json_object_get(
        response, tokens, token_count, data, "package_cleanup_pending");
    return kind >= 0 && retained_token >= 0 && cleanup_token >= 0 &&
                   cp0_json_string_equals(response, &tokens[kind],
                                          "uninstalled") &&
                   copy_member(response, tokens, token_count, data, "app_id",
                               response_app_id, sizeof(response_app_id)) &&
                   strcmp(response_app_id, app_id) == 0 &&
                   cp0_json_get_bool(response, &tokens[retained_token],
                                     &retained) &&
                   retained &&
                   cp0_json_get_bool(response, &tokens[cleanup_token],
                                     &cleanup_pending)
               ? 0
               : -1;
}

static int app_lifecycle_command(const char *command, const char *expected_kind,
                                 const char *app_id)
{
    char request[384];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;

    if (!valid_app_id(app_id))
        return -1;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"%s\",\"app_id\":\"%s\"}}\n",
        (unsigned long long)request_id, command, app_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 3000) != 0)
        return -1;
    return parse_lifecycle_response(response, response_length, request_id,
                                    expected_kind, app_id);
}

static int parse_device_settings_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, struct cp0_device_settings *settings)
{
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    struct cp0_device_settings decoded = {0};
    size_t token_count;
    uint64_t denied_count;
    int data;

    if (settings == NULL ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int value = cp0_json_object_get(response, tokens, token_count, data,
                                    "settings");
    if (kind < 0 || value < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], expected_kind) ||
        tokens[value].type != CP0_JSON_OBJECT)
        return -1;

    int authority = cp0_json_object_get(response, tokens, token_count, value,
                                        "authority");
    int developer = cp0_json_object_get(response, tokens, token_count, value,
                                        "developer_mode");
    int developer_allowed = cp0_json_object_get(
        response, tokens, token_count, value, "developer_mode_allowed");
    int recovery = cp0_json_object_get(response, tokens, token_count, value,
                                       "recovery_mode");
    int recovery_allowed = cp0_json_object_get(
        response, tokens, token_count, value, "recovery_mode_allowed");
    int store_allowed = cp0_json_object_get(
        response, tokens, token_count, value, "store_install_allowed");
    int launch_restricted = cp0_json_object_get(
        response, tokens, token_count, value, "app_launch_restricted");
    int denied = cp0_json_object_get(
        response, tokens, token_count, value, "denied_permission_count");
    if (authority < 0 || developer < 0 || developer_allowed < 0 ||
        recovery < 0 || recovery_allowed < 0 || store_allowed < 0 ||
        launch_restricted < 0 || denied < 0 ||
        !cp0_json_get_bool(response, &tokens[developer],
                           &decoded.developer_mode) ||
        !cp0_json_get_bool(response, &tokens[developer_allowed],
                           &decoded.developer_mode_allowed) ||
        !cp0_json_get_bool(response, &tokens[recovery],
                           &decoded.recovery_mode) ||
        !cp0_json_get_bool(response, &tokens[recovery_allowed],
                           &decoded.recovery_mode_allowed) ||
        !cp0_json_get_bool(response, &tokens[store_allowed],
                           &decoded.store_install_allowed) ||
        !cp0_json_get_bool(response, &tokens[launch_restricted],
                           &decoded.app_launch_restricted) ||
        !cp0_json_get_u64(response, &tokens[denied], &denied_count) ||
        denied_count > 8)
        return -1;
    if (cp0_json_string_equals(response, &tokens[authority], "personal"))
        decoded.authority = CP0_AUTHORITY_PERSONAL;
    else if (cp0_json_string_equals(response, &tokens[authority], "parent"))
        decoded.authority = CP0_AUTHORITY_PARENT;
    else if (cp0_json_string_equals(response, &tokens[authority],
                                    "organization"))
        decoded.authority = CP0_AUTHORITY_ORGANIZATION;
    else
        return -1;
    decoded.denied_permission_count = (uint8_t)denied_count;
    *settings = decoded;
    return 0;
}

static int device_settings_command(const char *name, const char *mode,
                                   bool enabled, const char *expected_kind,
                                   struct cp0_device_settings *settings)
{
    char request[320];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length;

    if (mode == NULL) {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"%s\"}}\n",
            (unsigned long long)request_id, name);
    } else {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"%s\",\"mode\":\"%s\",\"enabled\":%s}}\n",
            (unsigned long long)request_id, name, mode,
            enabled ? "true" : "false");
    }
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 3000) != 0)
        return -1;
    return parse_device_settings_response(response, response_length, request_id,
                                          expected_kind, settings);
}

int cp0_appd_get_device_settings(struct cp0_device_settings *settings)
{
    return device_settings_command("get-device-settings", NULL, false,
                                   "device-settings", settings);
}

int cp0_appd_set_device_mode(enum cp0_device_mode mode, bool enabled,
                             struct cp0_device_settings *settings)
{
    const char *mode_name;
    if (mode == CP0_DEVICE_MODE_DEVELOPER)
        mode_name = "developer";
    else if (mode == CP0_DEVICE_MODE_RECOVERY)
        mode_name = "recovery";
    else
        return -1;
    return device_settings_command("set-device-mode", mode_name, enabled,
                                   "device-mode-changed", settings);
}

#ifdef CP0_APPD_CLIENT_TEST
int cp0_appd_test_parse_app_page(
    const char *response, size_t response_length, uint64_t request_id,
    uint16_t offset, struct cp0_app_summary *apps, size_t capacity,
    size_t *app_count, bool *has_next, uint16_t *next_offset)
{
    return parse_app_page(response, response_length, request_id, offset, apps,
                          capacity, app_count, has_next, next_offset);
}

int cp0_appd_test_parse_lifecycle_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, const char *app_id)
{
    return parse_lifecycle_response(response, response_length, request_id,
                                    expected_kind, app_id);
}

int cp0_appd_test_parse_uninstall_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id)
{
    return parse_uninstall_response(response, response_length, request_id,
                                    app_id);
}

bool cp0_appd_test_valid_app_id(const char *app_id)
{
    return valid_app_id(app_id);
}

bool cp0_appd_test_valid_document_id(const char *document_id)
{
    return valid_document_id(document_id);
}

int cp0_appd_test_parse_device_settings_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, struct cp0_device_settings *settings)
{
    return parse_device_settings_response(response, response_length, request_id,
                                          expected_kind, settings);
}
#endif

static int parse_notification_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_notification *notification)
{
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t token_count;
    int data;

    if (notification == NULL ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int item = cp0_json_object_get(response, tokens, token_count, data,
                                   "notification");
    if (kind < 0 || item < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "next-notification"))
        return -1;
    if (cp0_json_is_null(response, &tokens[item]))
        return 0;
    if (tokens[item].type != CP0_JSON_OBJECT)
        return -1;

    struct cp0_notification decoded = {0};
    int notification_id = cp0_json_object_get(
        response, tokens, token_count, item, "notification_id");
    if (notification_id < 0 ||
        !cp0_json_get_u64(response, &tokens[notification_id],
                          &decoded.notification_id) ||
        decoded.notification_id == 0 ||
        !copy_member(response, tokens, token_count, item, "app_id",
                     decoded.app_id, sizeof(decoded.app_id)) ||
        !valid_app_id(decoded.app_id) ||
        !copy_member(response, tokens, token_count, item, "app_name",
                     decoded.app_name, sizeof(decoded.app_name)) ||
        decoded.app_name[0] == '\0' ||
        !copy_member(response, tokens, token_count, item, "title",
                     decoded.title, sizeof(decoded.title)) ||
        decoded.title[0] == '\0' ||
        !copy_member(response, tokens, token_count, item, "body",
                     decoded.body, sizeof(decoded.body)))
        return -1;
    *notification = decoded;
    return 1;
}

#ifdef CP0_APPD_CLIENT_TEST
int cp0_appd_test_parse_notification_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_notification *notification)
{
    return parse_notification_response(response, response_length, request_id,
                                       notification);
}
#endif

int cp0_appd_start_app(const char *app_id)
{
    return app_lifecycle_command("start", "started", app_id);
}

int cp0_appd_stop_app(const char *app_id)
{
    return app_lifecycle_command("stop", "stopped", app_id);
}

int cp0_appd_uninstall_app(const char *app_id)
{
    char request[384];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    if (!valid_app_id(app_id))
        return -1;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"uninstall\",\"app_id\":\"%s\"}}\n",
        (unsigned long long)request_id, app_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 3000) != 0)
        return -1;
    return parse_uninstall_response(response, response_length, request_id,
                                    app_id);
}

int cp0_appd_take_notification(struct cp0_notification *notification)
{
    char request[256];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"take-notification\"}}\n",
        (unsigned long long)request_id);

    if (notification == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 250) != 0)
        return -1;
    return parse_notification_response(response, response_length, request_id,
                                       notification);
}

int cp0_appd_get_permission_prompt(struct cp0_permission_prompt *prompt)
{
    char request[256];
    char response[CP0_APPD_FRAME_BYTES];
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t response_length;
    size_t token_count;
    int data;
    int prompt_token;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"get-permission-prompt\"}}\n",
        (unsigned long long)request_id);

    if (prompt == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 250) != 0 ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    prompt_token = cp0_json_object_get(response, tokens, token_count, data,
                                       "prompt");
    if (kind < 0 || prompt_token < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "pending-permission"))
        return -1;
    if (cp0_json_is_null(response, &tokens[prompt_token]))
        return 0;
    if (tokens[prompt_token].type != CP0_JSON_OBJECT)
        return -1;

    int prompt_id = cp0_json_object_get(response, tokens, token_count,
                                        prompt_token, "prompt_id");
    struct cp0_permission_prompt decoded = {0};
    if (prompt_id < 0 ||
        !cp0_json_get_u64(response, &tokens[prompt_id], &decoded.prompt_id) ||
        decoded.prompt_id == 0 ||
        !copy_member(response, tokens, token_count, prompt_token, "app_name",
                     decoded.app_name, sizeof(decoded.app_name)) ||
        !copy_member(response, tokens, token_count, prompt_token, "permission",
                     decoded.permission, sizeof(decoded.permission)) ||
        !copy_member(response, tokens, token_count, prompt_token, "reason",
                     decoded.reason, sizeof(decoded.reason)))
        return -1;
    *prompt = decoded;
    return 1;
}

int cp0_appd_resolve_permission(uint64_t prompt_id,
                                enum cp0_permission_choice choice)
{
    static const char *choices[] = {"allow-once", "allow-always", "deny"};
    char request[320];
    char response[CP0_APPD_FRAME_BYTES];
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t response_length;
    size_t token_count;
    int data;
    uint64_t request_id = next_request_id++;

    if (prompt_id == 0 || choice < CP0_PERMISSION_ALLOW_ONCE ||
        choice > CP0_PERMISSION_DENY)
        return -1;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"resolve-permission\",\"prompt_id\":%llu,"
        "\"choice\":\"%s\"}}\n",
        (unsigned long long)request_id, (unsigned long long)prompt_id,
        choices[choice]);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 250) != 0 ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    return kind >= 0 && cp0_json_string_equals(
                            response, &tokens[kind], "permission-resolved")
               ? 0
               : -1;
}

static int parse_document_prompt_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_document_prompt *prompt)
{
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    struct cp0_document_prompt decoded = {0};
    size_t token_count;
    int data;

    if (prompt == NULL ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int prompt_token = cp0_json_object_get(response, tokens, token_count, data,
                                           "prompt");
    if (kind < 0 || prompt_token < 0 ||
        !cp0_json_string_equals(response, &tokens[kind],
                                "pending-document"))
        return -1;
    if (cp0_json_is_null(response, &tokens[prompt_token]))
        return 0;
    if (tokens[prompt_token].type != CP0_JSON_OBJECT)
        return -1;

    int prompt_id = cp0_json_object_get(response, tokens, token_count,
                                        prompt_token, "prompt_id");
    int documents = cp0_json_object_get(response, tokens, token_count,
                                        prompt_token, "documents");
    if (prompt_id < 0 || documents < 0 ||
        !cp0_json_get_u64(response, &tokens[prompt_id], &decoded.prompt_id) ||
        decoded.prompt_id == 0 ||
        !copy_member(response, tokens, token_count, prompt_token, "app_name",
                     decoded.app_name, sizeof(decoded.app_name)) ||
        decoded.app_name[0] == '\0' || tokens[documents].type != CP0_JSON_ARRAY ||
        tokens[documents].children == 0 ||
        tokens[documents].children > CP0_DOCUMENT_MAX)
        return -1;

    decoded.document_count = tokens[documents].children;
    for (size_t index = 0; index < decoded.document_count; index++) {
        int item = cp0_json_array_get(tokens, token_count, documents,
                                      (unsigned int)index);
        struct cp0_document_summary *document = &decoded.documents[index];
        int size;
        if (item < 0 || tokens[item].type != CP0_JSON_OBJECT ||
            !copy_member(response, tokens, token_count, item, "document_id",
                         document->document_id,
                         sizeof(document->document_id)) ||
            !valid_document_id(document->document_id) ||
            !copy_member(response, tokens, token_count, item, "name",
                         document->name, sizeof(document->name)) ||
            document->name[0] == '\0')
            return -1;
        size = cp0_json_object_get(response, tokens, token_count, item,
                                   "size_bytes");
        if (size < 0 ||
            !cp0_json_get_u64(response, &tokens[size], &document->size_bytes) ||
            document->size_bytes > 16U * 1024U * 1024U)
            return -1;
    }
    *prompt = decoded;
    return 1;
}

#ifdef CP0_APPD_CLIENT_TEST
int cp0_appd_test_parse_document_prompt_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_document_prompt *prompt)
{
    return parse_document_prompt_response(response, response_length, request_id,
                                          prompt);
}
#endif

int cp0_appd_get_document_prompt(struct cp0_document_prompt *prompt)
{
    char request[256];
    char response[CP0_APPD_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"get-document-prompt\"}}\n",
        (unsigned long long)request_id);

    if (prompt == NULL || request_length <= 0 ||
        (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 250) != 0)
        return -1;
    return parse_document_prompt_response(response, response_length, request_id,
                                          prompt);
}

int cp0_appd_resolve_document(uint64_t prompt_id, const char *document_id)
{
    char request[512];
    char response[CP0_APPD_FRAME_BYTES];
    struct cp0_json_token tokens[CP0_APPD_JSON_TOKENS];
    size_t response_length;
    size_t token_count;
    int data;
    uint64_t request_id = next_request_id++;
    int request_length;

    if (prompt_id == 0 ||
        (document_id != NULL && !valid_document_id(document_id)))
        return -1;
    if (document_id == NULL) {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"resolve-document\",\"prompt_id\":%llu,"
            "\"document_id\":null}}\n",
            (unsigned long long)request_id, (unsigned long long)prompt_id);
    } else {
        request_length = snprintf(
            request, sizeof(request),
            "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
            "\"name\":\"resolve-document\",\"prompt_id\":%llu,"
            "\"document_id\":\"%s\"}}\n",
            (unsigned long long)request_id, (unsigned long long)prompt_id,
            document_id);
    }
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 250) != 0 ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    return kind >= 0 && cp0_json_string_equals(
                            response, &tokens[kind], "document-resolved")
               ? 0
               : -1;
}
