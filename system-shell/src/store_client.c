#define _GNU_SOURCE

#include "cp0_store_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_STORE_SOCKET
#define CP0_STORE_SOCKET "/run/cardputerzero-store/control.sock"
#endif
#define CP0_STORE_FRAME_BYTES (64U * 1024U)
#define CP0_STORE_JSON_TOKENS 4096U
#define CP0_STORE_CATALOG_LIMIT 64U
#define CP0_STORE_MAX_PACKAGE_BYTES (32U * 1024U * 1024U + 4096U)

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
                    strlen(CP0_STORE_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2 || strlen(CP0_STORE_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memcpy(address.sun_path, CP0_STORE_SOCKET, strlen(CP0_STORE_SOCKET) + 1U);
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
            if (part_length == 0 || part_length > 32 || app_id[index - 1] == '-')
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

static bool valid_numeric_identifier(const char *value, size_t length)
{
    if (length == 0 || (length > 1 && value[0] == '0'))
        return false;
    for (size_t index = 0; index < length; index++) {
        if (value[index] < '0' || value[index] > '9')
            return false;
    }
    return true;
}

static bool valid_semver_identifiers(const char *value, size_t length,
                                     bool prerelease)
{
    size_t start = 0;

    if (length == 0)
        return false;
    for (size_t index = 0; index <= length; index++) {
        if (index < length && value[index] != '.') {
            unsigned char byte = (unsigned char)value[index];
            if (!((byte >= 'a' && byte <= 'z') ||
                  (byte >= 'A' && byte <= 'Z') ||
                  (byte >= '0' && byte <= '9') || byte == '-'))
                return false;
            continue;
        }
        size_t identifier_length = index - start;
        if (identifier_length == 0 ||
            (prerelease &&
             value[start] >= '0' && value[start] <= '9' &&
             valid_numeric_identifier(value + start, identifier_length) ==
                 false)) {
            bool numeric = identifier_length > 0;
            for (size_t digit = start; digit < index; digit++)
                numeric = numeric && value[digit] >= '0' && value[digit] <= '9';
            if (identifier_length == 0 || numeric)
                return false;
        }
        start = index + 1U;
    }
    return true;
}

static bool valid_version(const char *version)
{
    size_t length;
    size_t core_end;
    size_t prerelease_start = 0;
    size_t build_start = 0;
    size_t part_start = 0;
    unsigned int core_parts = 0;

    if (version == NULL || (length = strlen(version)) == 0 || length > 64)
        return false;
    core_end = length;
    for (size_t index = 0; index < length; index++) {
        if (version[index] == '+' && build_start == 0) {
            build_start = index + 1U;
            if (prerelease_start == 0)
                core_end = index;
        } else if (version[index] == '-' && prerelease_start == 0 &&
                   build_start == 0) {
            prerelease_start = index + 1U;
            core_end = index;
        }
    }
    if (build_start > 0 &&
        !valid_semver_identifiers(version + build_start,
                                  length - build_start, false))
        return false;
    size_t prerelease_end = build_start > 0 ? build_start - 1U : length;
    if (prerelease_start > 0 &&
        !valid_semver_identifiers(version + prerelease_start,
                                  prerelease_end - prerelease_start, true))
        return false;
    for (size_t index = 0; index <= core_end; index++) {
        if (index < core_end && version[index] != '.')
            continue;
        if (!valid_numeric_identifier(version + part_start, index - part_start))
            return false;
        core_parts++;
        part_start = index + 1U;
    }
    return core_parts == 3;
}

static bool valid_text(const char *value, size_t maximum_chars,
                       size_t maximum_bytes)
{
    size_t length;
    size_t characters = 0;

    if (value == NULL || (length = strlen(value)) == 0 ||
        length > maximum_bytes)
        return false;
    for (size_t index = 0; index < length;) {
        unsigned char byte = (unsigned char)value[index];
        uint32_t codepoint;
        size_t bytes;

        if (byte < 0x80U) {
            codepoint = byte;
            bytes = 1;
        } else if (byte >= 0xc2U && byte <= 0xdfU && index + 1U < length) {
            codepoint = (uint32_t)(byte & 0x1fU) << 6U;
            bytes = 2;
        } else if (byte >= 0xe0U && byte <= 0xefU && index + 2U < length) {
            codepoint = (uint32_t)(byte & 0x0fU) << 12U;
            bytes = 3;
        } else if (byte >= 0xf0U && byte <= 0xf4U && index + 3U < length) {
            codepoint = (uint32_t)(byte & 0x07U) << 18U;
            bytes = 4;
        } else {
            return false;
        }
        for (size_t continuation = 1; continuation < bytes; continuation++) {
            unsigned char next = (unsigned char)value[index + continuation];
            if ((next & 0xc0U) != 0x80U)
                return false;
            codepoint |= (uint32_t)(next & 0x3fU)
                         << (6U * (unsigned int)(bytes - continuation - 1U));
        }
        if ((bytes == 2 && codepoint < 0x80U) ||
            (bytes == 3 && codepoint < 0x800U) ||
            (bytes == 4 && codepoint < 0x10000U) || codepoint > 0x10ffffU ||
            (codepoint >= 0xd800U && codepoint <= 0xdfffU) ||
            codepoint < 0x20U || (codepoint >= 0x7fU && codepoint <= 0x9fU))
            return false;
        characters++;
        if (characters > maximum_chars)
            return false;
        index += bytes;
    }
    return true;
}

static bool valid_error_code(const char *document,
                             const struct cp0_json_token *token)
{
    static const char *codes[] = {
        "invalid-request", "unauthorized", "unconfigured", "unavailable",
        "not-found",       "busy",         "untrusted",    "resource-exhausted",
        "internal",
    };
    for (size_t index = 0; index < sizeof(codes) / sizeof(codes[0]); index++) {
        if (cp0_json_string_equals(document, token, codes[index]))
            return true;
    }
    return false;
}

static int parse_envelope(const char *document, size_t length,
                          uint64_t request_id, struct cp0_json_token *tokens,
                          size_t token_capacity, size_t *token_count, int *data)
{
    int count = cp0_json_parse(document, length, tokens, token_capacity);
    if (count <= 0 || tokens[0].type != CP0_JSON_OBJECT ||
        tokens[0].children != 6)
        return CP0_STORE_RESULT_ERROR;
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
        return CP0_STORE_RESULT_ERROR;
    int status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                     "status");
    if (status < 0)
        return CP0_STORE_RESULT_ERROR;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                       "code");
        int message = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "message");
        char decoded_message[641];
        if (tokens[outcome].children != 6 || code < 0 || message < 0 ||
            !valid_error_code(document, &tokens[code]) ||
            !cp0_json_copy_string(document, &tokens[message], decoded_message,
                                  sizeof(decoded_message)) ||
            !valid_text(decoded_message, 160, 640))
            return CP0_STORE_RESULT_ERROR;
        if (cp0_json_string_equals(document, &tokens[code], "unconfigured"))
            return CP0_STORE_RESULT_UNCONFIGURED;
        if (cp0_json_string_equals(document, &tokens[code], "busy"))
            return CP0_STORE_RESULT_BUSY;
        return CP0_STORE_RESULT_ERROR;
    }
    *data = cp0_json_object_get(document, tokens, (size_t)count, outcome, "data");
    if (tokens[outcome].children != 4 ||
        !cp0_json_string_equals(document, &tokens[status], "ok") || *data < 0 ||
        tokens[*data].type != CP0_JSON_OBJECT)
        return CP0_STORE_RESULT_ERROR;
    *token_count = (size_t)count;
    return CP0_STORE_RESULT_OK;
}

static bool parse_state(const char *document, const struct cp0_json_token *token,
                        enum cp0_store_app_state *state)
{
    static const char *names[] = {"available", "queued", "downloading",
                                  "installing", "installed", "failed"};
    for (size_t index = 0; index < sizeof(names) / sizeof(names[0]); index++) {
        if (cp0_json_string_equals(document, token, names[index])) {
            *state = (enum cp0_store_app_state)index;
            return true;
        }
    }
    return false;
}

static bool permission_bit(const char *document,
                           const struct cp0_json_token *token, uint16_t *bit)
{
    static const struct {
        const char *name;
        uint16_t bit;
    } permissions[] = {
        {"audio.capture", CP0_STORE_PERMISSION_AUDIO_CAPTURE},
        {"audio.playback", CP0_STORE_PERMISSION_AUDIO_PLAYBACK},
        {"camera.capture", CP0_STORE_PERMISSION_CAMERA_CAPTURE},
        {"documents.open", CP0_STORE_PERMISSION_DOCUMENTS_OPEN},
        {"hardware.gpio", CP0_STORE_PERMISSION_HARDWARE_GPIO},
        {"network.client", CP0_STORE_PERMISSION_NETWORK_CLIENT},
        {"notifications.post", CP0_STORE_PERMISSION_NOTIFICATIONS_POST},
        {"radio.lora", CP0_STORE_PERMISSION_RADIO_LORA},
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

static bool parse_app(const char *document,
                      const struct cp0_json_token *tokens, size_t token_count,
                      int item, struct cp0_store_app_summary *app)
{
    int package_bytes = cp0_json_object_get(document, tokens, token_count, item,
                                            "package_bytes");
    int state = cp0_json_object_get(document, tokens, token_count, item, "state");
    int progress = cp0_json_object_get(document, tokens, token_count, item,
                                       "progress_percent");
    int permissions = cp0_json_object_get(document, tokens, token_count, item,
                                          "permissions");
    uint64_t parsed_progress;
    if (tokens[item].type != CP0_JSON_OBJECT || tokens[item].children != 16 ||
        package_bytes < 0 || state < 0 ||
        progress < 0 || permissions < 0 ||
        !copy_member(document, tokens, token_count, item, "app_id", app->app_id,
                     sizeof(app->app_id)) ||
        !valid_app_id(app->app_id) ||
        !copy_member(document, tokens, token_count, item, "name", app->name,
                     sizeof(app->name)) ||
        !valid_text(app->name, 32, 128) ||
        !copy_member(document, tokens, token_count, item, "version", app->version,
                     sizeof(app->version)) ||
        !valid_version(app->version) ||
        !copy_member(document, tokens, token_count, item, "summary", app->summary,
                     sizeof(app->summary)) ||
        !valid_text(app->summary, 96, 384) ||
        !cp0_json_get_u64(document, &tokens[package_bytes], &app->package_bytes) ||
        app->package_bytes == 0 || app->package_bytes > CP0_STORE_MAX_PACKAGE_BYTES ||
        !parse_state(document, &tokens[state], &app->state) ||
        !cp0_json_get_u64(document, &tokens[progress], &parsed_progress) ||
        parsed_progress > 100 || tokens[permissions].type != CP0_JSON_ARRAY ||
        tokens[permissions].children > 8)
        return false;
    app->progress_percent = (uint8_t)parsed_progress;
    if (((app->state == CP0_STORE_APP_AVAILABLE ||
          app->state == CP0_STORE_APP_QUEUED ||
          app->state == CP0_STORE_APP_FAILED) &&
         app->progress_percent != 0) ||
        ((app->state == CP0_STORE_APP_INSTALLING ||
          app->state == CP0_STORE_APP_INSTALLED) &&
         app->progress_percent != 100))
        return false;

    char previous[32] = {0};
    app->permissions = 0;
    for (unsigned int index = 0; index < tokens[permissions].children; index++) {
        int permission = cp0_json_array_get(tokens, token_count, permissions, index);
        char name[32];
        uint16_t bit;
        if (permission < 0 ||
            !cp0_json_copy_string(document, &tokens[permission], name,
                                  sizeof(name)) ||
            !permission_bit(document, &tokens[permission], &bit) ||
            (index > 0 && strcmp(previous, name) >= 0) ||
            (app->permissions & bit) != 0)
            return false;
        memcpy(previous, name, strlen(name) + 1U);
        app->permissions |= bit;
    }
    return true;
}

static int parse_catalog_response(const char *response, size_t response_length,
                                  uint64_t request_id,
                                  struct cp0_store_catalog *catalog)
{
    struct cp0_json_token *tokens =
        calloc(CP0_STORE_JSON_TOKENS, sizeof(*tokens));
    struct cp0_store_catalog decoded = {0};
    size_t token_count;
    int data;
    int result;

    if (tokens == NULL || catalog == NULL) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    result = parse_envelope(response, response_length, request_id, tokens,
                            CP0_STORE_JSON_TOKENS, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK) {
        free(tokens);
        return result;
    }
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    int sequence = cp0_json_object_get(response, tokens, token_count, data,
                                       "sequence");
    int expires = cp0_json_object_get(response, tokens, token_count, data,
                                      "expires_unix_seconds");
    int stale = cp0_json_object_get(response, tokens, token_count, data, "stale");
    int apps = cp0_json_object_get(response, tokens, token_count, data, "apps");
    if (tokens[data].children != 10 || kind < 0 || sequence < 0 || expires < 0 ||
        stale < 0 || apps < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], "catalog") ||
        !cp0_json_get_u64(response, &tokens[sequence], &decoded.sequence) ||
        decoded.sequence == 0 ||
        !cp0_json_get_u64(response, &tokens[expires],
                          &decoded.expires_unix_seconds) ||
        decoded.expires_unix_seconds == 0 ||
        !cp0_json_get_bool(response, &tokens[stale], &decoded.stale) ||
        tokens[apps].type != CP0_JSON_ARRAY ||
        tokens[apps].children > CP0_STORE_CATALOG_LIMIT) {
        free(tokens);
        return CP0_STORE_RESULT_ERROR;
    }
    decoded.truncated = tokens[apps].children > CP0_STORE_MAX_APPS;
    decoded.count = tokens[apps].children;
    if (decoded.count > CP0_STORE_MAX_APPS)
        decoded.count = CP0_STORE_MAX_APPS;
    char previous_id[CP0_STORE_APP_ID_BYTES] = {0};
    for (unsigned int index = 0; index < tokens[apps].children; index++) {
        int item = cp0_json_array_get(tokens, token_count, apps, index);
        struct cp0_store_app_summary temporary = {0};
        if (item < 0 || !parse_app(response, tokens, token_count, item, &temporary) ||
            (index > 0 && strcmp(previous_id, temporary.app_id) >= 0)) {
            free(tokens);
            return CP0_STORE_RESULT_ERROR;
        }
        memcpy(previous_id, temporary.app_id, strlen(temporary.app_id) + 1U);
        if (index < CP0_STORE_MAX_APPS)
            decoded.apps[index] = temporary;
    }
    free(tokens);
    *catalog = decoded;
    return CP0_STORE_RESULT_OK;
}

static int parse_accepted(const char *response, size_t response_length,
                          uint64_t request_id, const char *expected_kind,
                          const char *expected_app_id)
{
    struct cp0_json_token tokens[64];
    size_t token_count;
    int data;
    int result = parse_envelope(response, response_length, request_id, tokens,
                                64, &token_count, &data);
    if (result != CP0_STORE_RESULT_OK)
        return result;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    unsigned int expected_children = expected_app_id == NULL ? 2U : 6U;
    if (tokens[data].children != expected_children || kind < 0 ||
        !cp0_json_string_equals(response, &tokens[kind], expected_kind))
        return CP0_STORE_RESULT_ERROR;
    if (expected_app_id != NULL) {
        char app_id[CP0_STORE_APP_ID_BYTES];
        char version[CP0_STORE_VERSION_BYTES];
        if (!copy_member(response, tokens, token_count, data, "app_id", app_id,
                         sizeof(app_id)) ||
            strcmp(app_id, expected_app_id) != 0 ||
            !copy_member(response, tokens, token_count, data, "version", version,
                         sizeof(version)) ||
            !valid_version(version))
            return CP0_STORE_RESULT_ERROR;
    }
    return CP0_STORE_RESULT_OK;
}

#ifdef CP0_STORE_CLIENT_TEST
int cp0_store_test_parse_catalog_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_catalog *catalog)
{
    return parse_catalog_response(response, response_length, request_id, catalog);
}

int cp0_store_test_parse_refresh_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id)
{
    return parse_accepted(response, response_length, request_id,
                          "refresh-accepted", NULL);
}

int cp0_store_test_parse_install_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id,
                                          const char *app_id)
{
    return parse_accepted(response, response_length, request_id,
                          "install-accepted", app_id);
}
#endif

int cp0_store_list(struct cp0_store_catalog *catalog)
{
    char request[192];
    char *response = malloc(CP0_STORE_FRAME_BYTES);
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"list\"}}\n",
        (unsigned long long)request_id);
    int result = CP0_STORE_RESULT_ERROR;

    if (catalog != NULL && response != NULL && request_length > 0 &&
        (size_t)request_length < sizeof(request) &&
        exchange(request, (size_t)request_length, response,
                 CP0_STORE_FRAME_BYTES, &response_length, 500) == 0)
        result = parse_catalog_response(response, response_length, request_id,
                                        catalog);
    free(response);
    return result;
}

int cp0_store_refresh(void)
{
    char request[192];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"refresh\"}}\n",
        (unsigned long long)request_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 1000) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_accepted(response, response_length, request_id,
                          "refresh-accepted", NULL);
}

int cp0_store_install(const char *app_id)
{
    char request[384];
    char response[1024];
    size_t response_length;
    uint64_t request_id = next_request_id++;

    if (!valid_app_id(app_id))
        return CP0_STORE_RESULT_ERROR;
    int request_length = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":1,\"request_id\":%llu,\"command\":{"
        "\"name\":\"install\",\"app_id\":\"%s\"}}\n",
        (unsigned long long)request_id, app_id);
    if (request_length <= 0 || (size_t)request_length >= sizeof(request) ||
        exchange(request, (size_t)request_length, response, sizeof(response),
                 &response_length, 1000) != 0)
        return CP0_STORE_RESULT_ERROR;
    return parse_accepted(response, response_length, request_id,
                          "install-accepted", app_id);
}
