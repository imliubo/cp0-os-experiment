#define _POSIX_C_SOURCE 200809L

#include "cp0_provision_client.h"
#include "cp0_json.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#ifndef CP0_PROVISION_SOCKET
#define CP0_PROVISION_SOCKET "/run/cardputerzero-provisiond/provision.sock"
#endif

#define CP0_PROVISION_FRAME_BYTES (16U * 1024U)
#define CP0_PROVISION_JSON_TOKENS 768U
#define CP0_PROVISION_ERROR_SENTINEL 100
#define CP0_PROVISION_TIMEOUT_SYSTEM 20
#define CP0_PROVISION_TIMEOUT_PASSWORD 75
#define CP0_PROVISION_TIMEOUT_WIFI_SCAN 45
#define CP0_PROVISION_TIMEOUT_WIFI_CONNECT 75
#define CP0_PROVISION_TIMEOUT_COMMIT 75

#ifndef MSG_NOSIGNAL
#define MSG_NOSIGNAL 0
#endif

static uint64_t next_request_id = 1;

static void clear_secret(void *buffer, size_t length)
{
    volatile unsigned char *bytes = buffer;
    while (length-- > 0)
        *bytes++ = 0;
}

static bool append_bytes(char *output, size_t capacity, size_t *offset,
                         const char *input, size_t length)
{
    if (*offset > capacity || length > capacity - *offset)
        return false;
    memcpy(output + *offset, input, length);
    *offset += length;
    return true;
}

static bool append_json_string(char *output, size_t capacity, size_t *offset,
                               const char *input)
{
    static const char hex[] = "0123456789abcdef";
    if (input == NULL || !append_bytes(output, capacity, offset, "\"", 1))
        return false;
    for (const unsigned char *byte = (const unsigned char *)input; *byte;
         byte++) {
        char escaped[6];
        size_t length = 1;
        if (*byte == '\"' || *byte == '\\') {
            escaped[0] = '\\';
            escaped[1] = (char)*byte;
            length = 2;
        } else if (*byte < 0x20U) {
            escaped[0] = '\\';
            escaped[1] = 'u';
            escaped[2] = '0';
            escaped[3] = '0';
            escaped[4] = hex[*byte >> 4U];
            escaped[5] = hex[*byte & 0x0fU];
            length = sizeof(escaped);
        } else {
            escaped[0] = (char)*byte;
        }
        if (!append_bytes(output, capacity, offset, escaped, length))
            return false;
    }
    return append_bytes(output, capacity, offset, "\"", 1);
}

static int exchange(const char *request, size_t request_length, char *response,
                    size_t response_capacity, size_t *response_length,
                    unsigned int timeout_seconds)
{
    const struct timeval timeout = {
        .tv_sec = (time_t)timeout_seconds,
        .tv_usec = 0,
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_PROVISION_SOCKET) + 1U);
    int descriptor = -1;
    ssize_t count;
    int result = -1;

    if (request == NULL || response == NULL || response_length == NULL ||
        request_length == 0 || request_length > CP0_PROVISION_FRAME_BYTES ||
        response_capacity < CP0_PROVISION_FRAME_BYTES + 1U ||
        timeout_seconds == 0 ||
        strlen(CP0_PROVISION_SOCKET) >= sizeof(address.sun_path))
        return -1;
    descriptor = socket(AF_UNIX, SOCK_SEQPACKET, 0);
    if (descriptor < 0 || fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0)
        goto cleanup;
    memcpy(address.sun_path, CP0_PROVISION_SOCKET,
           strlen(CP0_PROVISION_SOCKET) + 1U);
#ifdef __APPLE__
    address.sun_len = (uint8_t)address_length;
#endif
    if (connect(descriptor, (const struct sockaddr *)&address,
                address_length) != 0)
        goto cleanup;
    do {
        count = send(descriptor, request, request_length, MSG_NOSIGNAL);
    } while (count < 0 && errno == EINTR);
    if (count < 0 || (size_t)count != request_length)
        goto cleanup;
    do {
        count = recv(descriptor, response, response_capacity - 1U, 0);
    } while (count < 0 && errno == EINTR);
    if (count <= 0 || (size_t)count > CP0_PROVISION_FRAME_BYTES ||
        response[count - 1] != '\n' || memchr(response, '\n', count - 1) != NULL)
        goto cleanup;
    response[count - 1] = '\0';
    *response_length = (size_t)count - 1U;
    result = 0;

cleanup:
    if (descriptor >= 0)
        close(descriptor);
    return result;
}

static int phase_value(const char *document, const struct cp0_json_token *token,
                       enum cp0_provision_phase *phase)
{
    static const char *values[] = {
        "unprovisioned", "region",      "owner",      "password-ready",
        "network",       "remote-access", "review",     "committing",
        "complete",      "repair-required",
    };
    for (unsigned int index = 0; index < sizeof(values) / sizeof(values[0]);
         index++) {
        if (cp0_json_string_equals(document, token, values[index])) {
            *phase = (enum cp0_provision_phase)index;
            return 0;
        }
    }
    return -1;
}

static bool copy_optional(const char *document,
                          const struct cp0_json_token *tokens,
                          size_t token_count, int object, const char *key,
                          char *output, size_t capacity)
{
    int token = cp0_json_object_get(document, tokens, token_count, object, key);
    if (token < 0)
        return false;
    if (cp0_json_is_null(document, &tokens[token])) {
        output[0] = '\0';
        return true;
    }
    return cp0_json_copy_string(document, &tokens[token], output, capacity);
}

static int parse_error(const char *document,
                       const struct cp0_json_token *tokens,
                       size_t token_count, int outcome,
                       char error[CP0_PROVISION_ERROR_MAX + 1])
{
    int code = cp0_json_object_get(document, tokens, token_count, outcome,
                                   "code");
    int message = cp0_json_object_get(document, tokens, token_count, outcome,
                                      "message");
    if (code < 0 || message < 0 ||
        !cp0_json_copy_string(document, &tokens[message], error,
                              CP0_PROVISION_ERROR_MAX + 1U))
        return CP0_PROVISION_FAILED;
    if (cp0_json_string_equals(document, &tokens[code], "unavailable"))
        return CP0_PROVISION_UNAVAILABLE;
    if (cp0_json_string_equals(document, &tokens[code], "invalid-state"))
        return CP0_PROVISION_INVALID_STATE;
    if (cp0_json_string_equals(document, &tokens[code], "invalid-value"))
        return CP0_PROVISION_INVALID_VALUE;
    if (cp0_json_string_equals(document, &tokens[code], "authentication"))
        return CP0_PROVISION_AUTHENTICATION;
    if (cp0_json_string_equals(document, &tokens[code], "repair-required"))
        return CP0_PROVISION_REPAIR_REQUIRED;
    return CP0_PROVISION_FAILED;
}

static int parse_envelope(const char *document, size_t length,
                          uint64_t request_id,
                          struct cp0_json_token tokens[], size_t capacity,
                          int *outcome,
                          char error[CP0_PROVISION_ERROR_MAX + 1])
{
    int count = cp0_json_parse(document, length, tokens, capacity);
    uint64_t version;
    uint64_t parsed_id;
    int version_token;
    int id_token;
    int status;
    if (error != NULL)
        error[0] = '\0';
    if (count <= 0 || tokens[0].type != CP0_JSON_OBJECT)
        return CP0_PROVISION_FAILED;
    version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                        "protocol_version");
    id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "request_id");
    *outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "outcome");
    if (version_token < 0 || id_token < 0 || *outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &parsed_id) ||
        version != 1U || parsed_id != request_id ||
        tokens[*outcome].type != CP0_JSON_OBJECT)
        return CP0_PROVISION_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, *outcome,
                                 "status");
    if (status < 0 || tokens[status].type != CP0_JSON_STRING)
        return CP0_PROVISION_FAILED;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int result = parse_error(document, tokens, (size_t)count, *outcome,
                                 error);
        return result == CP0_PROVISION_FAILED
                   ? CP0_PROVISION_FAILED
                   : -(CP0_PROVISION_ERROR_SENTINEL + result);
    }
    return count;
}

static int parse_state_response(const char *document, size_t length,
                                uint64_t request_id,
                                struct cp0_provision_status *state,
                                char error[CP0_PROVISION_ERROR_MAX + 1])
{
    struct cp0_json_token tokens[CP0_PROVISION_JSON_TOKENS];
    struct cp0_provision_status parsed = {0};
    int outcome;
    int count = parse_envelope(document, length, request_id, tokens,
                               CP0_PROVISION_JSON_TOKENS, &outcome, error);
    int state_token;
    int status_token;
    int phase;
    int password;
    int ssh;
    int network;
    int runtime;
    if (count <= 0)
        return count <= -(CP0_PROVISION_ERROR_SENTINEL + 1)
                   ? -count - CP0_PROVISION_ERROR_SENTINEL
                   : CP0_PROVISION_FAILED;
    status_token = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "status");
    if (!cp0_json_string_equals(document, &tokens[status_token], "state"))
        return CP0_PROVISION_FAILED;
    state_token = cp0_json_object_get(document, tokens, (size_t)count,
                                      outcome, "state");
    if (state_token < 0 || tokens[state_token].type != CP0_JSON_OBJECT)
        return CP0_PROVISION_FAILED;
    phase = cp0_json_object_get(document, tokens, (size_t)count, state_token,
                                "phase");
    password = cp0_json_object_get(document, tokens, (size_t)count,
                                   state_token, "password_configured");
    ssh = cp0_json_object_get(document, tokens, (size_t)count, state_token,
                              "ssh_enabled");
    network = cp0_json_object_get(document, tokens, (size_t)count, state_token,
                                  "network_choice");
    if (phase < 0 || password < 0 || ssh < 0 || network < 0 ||
        phase_value(document, &tokens[phase], &parsed.phase) != 0 ||
        !cp0_json_get_bool(document, &tokens[password],
                           &parsed.password_configured) ||
        !cp0_json_get_bool(document, &tokens[ssh], &parsed.ssh_enabled) ||
        !copy_optional(document, tokens, (size_t)count, state_token, "locale",
                       parsed.locale, sizeof(parsed.locale)) ||
        !copy_optional(document, tokens, (size_t)count, state_token, "country",
                       parsed.country, sizeof(parsed.country)) ||
        !copy_optional(document, tokens, (size_t)count, state_token, "timezone",
                       parsed.timezone, sizeof(parsed.timezone)) ||
        !copy_optional(document, tokens, (size_t)count, state_token, "hostname",
                       parsed.hostname, sizeof(parsed.hostname)) ||
        !copy_optional(document, tokens, (size_t)count, state_token,
                       "display_name", parsed.display_name,
                       sizeof(parsed.display_name)) ||
        !copy_optional(document, tokens, (size_t)count, state_token, "username",
                       parsed.username, sizeof(parsed.username)))
        return CP0_PROVISION_FAILED;
    if (!cp0_json_is_null(document, &tokens[network])) {
        int kind;
        if (tokens[network].type != CP0_JSON_OBJECT ||
            (kind = cp0_json_object_get(document, tokens, (size_t)count,
                                        network, "kind")) < 0)
            return CP0_PROVISION_FAILED;
        if (cp0_json_string_equals(document, &tokens[kind], "ethernet"))
            parsed.network_kind = CP0_PROVISION_NETWORK_ETHERNET;
        else if (cp0_json_string_equals(document, &tokens[kind], "offline"))
            parsed.network_kind = CP0_PROVISION_NETWORK_OFFLINE;
        else if (cp0_json_string_equals(document, &tokens[kind], "wifi")) {
            int ssid = cp0_json_object_get(document, tokens, (size_t)count,
                                           network, "ssid");
            if (ssid < 0 || !cp0_json_copy_string(
                                document, &tokens[ssid], parsed.network_ssid,
                                sizeof(parsed.network_ssid)))
                return CP0_PROVISION_FAILED;
            parsed.network_kind = CP0_PROVISION_NETWORK_WIFI;
        } else {
            return CP0_PROVISION_FAILED;
        }
    }
    runtime = cp0_json_object_get(document, tokens, (size_t)count, state_token,
                                  "network_runtime");
    if (runtime >= 0) {
        int manager;
        int ethernet;
        int wifi_available;
        int wifi_connected;
        if (tokens[runtime].type != CP0_JSON_OBJECT ||
            (manager = cp0_json_object_get(document, tokens, (size_t)count,
                                           runtime,
                                           "network_manager_available")) < 0 ||
            (ethernet = cp0_json_object_get(document, tokens, (size_t)count,
                                            runtime,
                                            "ethernet_connected")) < 0 ||
            (wifi_available = cp0_json_object_get(
                 document, tokens, (size_t)count, runtime, "wifi_available")) <
                0 ||
            (wifi_connected = cp0_json_object_get(
                 document, tokens, (size_t)count, runtime, "wifi_connected")) <
                0 ||
            !cp0_json_get_bool(document, &tokens[manager],
                               &parsed.network_manager_available) ||
            !cp0_json_get_bool(document, &tokens[ethernet],
                               &parsed.ethernet_connected) ||
            !cp0_json_get_bool(document, &tokens[wifi_available],
                               &parsed.wifi_available) ||
            !cp0_json_get_bool(document, &tokens[wifi_connected],
                               &parsed.wifi_connected) ||
            !copy_optional(document, tokens, (size_t)count, runtime,
                           "ethernet_ipv4", parsed.ethernet_ipv4,
                           sizeof(parsed.ethernet_ipv4)) ||
            !copy_optional(document, tokens, (size_t)count, runtime,
                           "wifi_ipv4", parsed.wifi_ipv4,
                           sizeof(parsed.wifi_ipv4)))
            return CP0_PROVISION_FAILED;
        if ((!parsed.network_manager_available &&
             (parsed.ethernet_connected || parsed.wifi_available ||
              parsed.wifi_connected || parsed.ethernet_ipv4[0] != '\0' ||
              parsed.wifi_ipv4[0] != '\0')) ||
            (parsed.ethernet_ipv4[0] != '\0' &&
             (!parsed.ethernet_connected ||
              inet_pton(AF_INET, parsed.ethernet_ipv4, &(struct in_addr){0}) !=
                  1)) ||
            (parsed.wifi_connected && !parsed.wifi_available) ||
            (parsed.wifi_ipv4[0] != '\0' &&
             (!parsed.wifi_connected ||
              inet_pton(AF_INET, parsed.wifi_ipv4, &(struct in_addr){0}) != 1)))
            return CP0_PROVISION_FAILED;
    }
    *state = parsed;
    return CP0_PROVISION_OK;
}

static int parse_wifi_response(const char *document, size_t length,
                               uint64_t request_id,
                               struct cp0_provision_wifi_list *list,
                               char error[CP0_PROVISION_ERROR_MAX + 1])
{
    struct cp0_json_token tokens[CP0_PROVISION_JSON_TOKENS];
    struct cp0_provision_wifi_list parsed = {0};
    int outcome;
    int count = parse_envelope(document, length, request_id, tokens,
                               CP0_PROVISION_JSON_TOKENS, &outcome, error);
    int status;
    int networks;
    if (count <= 0)
        return count <= -(CP0_PROVISION_ERROR_SENTINEL + 1)
                   ? -count - CP0_PROVISION_ERROR_SENTINEL
                   : CP0_PROVISION_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    networks = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                   "networks");
    if (status < 0 || networks < 0 ||
        !cp0_json_string_equals(document, &tokens[status], "wifi-list") ||
        tokens[networks].type != CP0_JSON_ARRAY ||
        tokens[networks].children > CP0_PROVISION_WIFI_MAX)
        return CP0_PROVISION_FAILED;
    parsed.count = tokens[networks].children;
    for (size_t index = 0; index < parsed.count; index++) {
        int item = cp0_json_array_get(tokens, (size_t)count, networks,
                                      (unsigned int)index);
        int ssid;
        int signal;
        int security;
        int connected;
        uint64_t signal_value;
        if (item < 0 || tokens[item].type != CP0_JSON_OBJECT ||
            (ssid = cp0_json_object_get(document, tokens, (size_t)count, item,
                                        "ssid")) < 0 ||
            (signal = cp0_json_object_get(document, tokens, (size_t)count,
                                          item, "signal_percent")) < 0 ||
            (security = cp0_json_object_get(document, tokens, (size_t)count,
                                            item, "security")) < 0 ||
            (connected = cp0_json_object_get(document, tokens, (size_t)count,
                                             item, "connected")) < 0 ||
            !cp0_json_copy_string(document, &tokens[ssid],
                                  parsed.networks[index].ssid,
                                  sizeof(parsed.networks[index].ssid)) ||
            !cp0_json_get_u64(document, &tokens[signal], &signal_value) ||
            signal_value > 100 ||
            !cp0_json_get_bool(document, &tokens[connected],
                               &parsed.networks[index].connected))
            return CP0_PROVISION_FAILED;
        parsed.networks[index].signal_percent = (uint8_t)signal_value;
        if (cp0_json_string_equals(document, &tokens[security], "open"))
            parsed.networks[index].security = CP0_PROVISION_WIFI_OPEN;
        else if (cp0_json_string_equals(document, &tokens[security], "wpa2"))
            parsed.networks[index].security = CP0_PROVISION_WIFI_WPA2;
        else if (cp0_json_string_equals(document, &tokens[security], "wpa3"))
            parsed.networks[index].security = CP0_PROVISION_WIFI_WPA3;
        else if (cp0_json_string_equals(document, &tokens[security],
                                        "unsupported"))
            parsed.networks[index].security = CP0_PROVISION_WIFI_UNSUPPORTED;
        else
            return CP0_PROVISION_FAILED;
    }
    *list = parsed;
    return CP0_PROVISION_OK;
}

static int run_request(char *request, size_t length,
                       struct cp0_provision_status *status,
                       struct cp0_provision_wifi_list *wifi,
                       uint64_t request_id, unsigned int timeout_seconds,
                       char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char response[CP0_PROVISION_FRAME_BYTES + 1U];
    size_t response_length;
    int result;
    if (exchange(request, length, response, sizeof(response), &response_length,
                 timeout_seconds) != 0) {
        if (error != NULL)
            snprintf(error, CP0_PROVISION_ERROR_MAX + 1U,
                     "Provisioning service is unavailable");
        return CP0_PROVISION_UNAVAILABLE;
    }
    result = wifi != NULL
                 ? parse_wifi_response(response, response_length, request_id,
                                       wifi, error)
                 : parse_state_response(response, response_length, request_id,
                                        status, error);
    clear_secret(response, sizeof(response));
    return result;
}

static int command_no_fields(const char *name,
                             struct cp0_provision_status *status,
                             struct cp0_provision_wifi_list *wifi,
                             unsigned int timeout_seconds,
                             char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[256];
    uint64_t request_id = next_request_id++;
    int count = snprintf(request, sizeof(request),
                         "{\"protocol_version\":1,\"request_id\":%llu,"
                         "\"command\":{\"name\":\"%s\"}}\n",
                         (unsigned long long)request_id, name);
    if (count <= 0 || (size_t)count >= sizeof(request))
        return CP0_PROVISION_FAILED;
    return run_request(request, (size_t)count, status, wifi, request_id,
                       timeout_seconds, error);
}

static bool begin_command(char *request, size_t capacity, size_t *offset,
                          uint64_t request_id, const char *name)
{
    int count = snprintf(request, capacity,
                         "{\"protocol_version\":1,\"request_id\":%llu,"
                         "\"command\":{\"name\":\"%s\"",
                         (unsigned long long)request_id, name);
    if (count <= 0 || (size_t)count >= capacity)
        return false;
    *offset = (size_t)count;
    return true;
}

static bool append_member(char *request, size_t capacity, size_t *offset,
                          const char *name, const char *value)
{
    return append_bytes(request, capacity, offset, ",\"", 2) &&
           append_bytes(request, capacity, offset, name, strlen(name)) &&
           append_bytes(request, capacity, offset, "\":", 2) &&
           append_json_string(request, capacity, offset, value);
}

static bool finish_command(char *request, size_t capacity, size_t *offset)
{
    return append_bytes(request, capacity, offset, "}}\n", 3);
}

int cp0_provision_get_status(struct cp0_provision_status *status,
                             char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return status == NULL
               ? CP0_PROVISION_FAILED
               : command_no_fields("get-status", status, NULL,
                                   CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_set_region(const char *locale, const char *country,
                             const char *timezone, const char *hostname,
                             struct cp0_provision_status *status,
                             char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[1024];
    size_t offset;
    uint64_t request_id = next_request_id++;
    if (status == NULL ||
        !begin_command(request, sizeof(request), &offset, request_id,
                       "set-region") ||
        !append_member(request, sizeof(request), &offset, "locale", locale) ||
        !append_member(request, sizeof(request), &offset, "country", country) ||
        !append_member(request, sizeof(request), &offset, "timezone", timezone) ||
        !append_member(request, sizeof(request), &offset, "hostname", hostname) ||
        !finish_command(request, sizeof(request), &offset))
        return CP0_PROVISION_FAILED;
    return run_request(request, offset, status, NULL, request_id,
                       CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_set_owner(const char *display_name, const char *username,
                            struct cp0_provision_status *status,
                            char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[768];
    size_t offset;
    uint64_t request_id = next_request_id++;
    if (status == NULL ||
        !begin_command(request, sizeof(request), &offset, request_id,
                       "set-owner") ||
        !append_member(request, sizeof(request), &offset, "display_name",
                       display_name) ||
        !append_member(request, sizeof(request), &offset, "username", username) ||
        !finish_command(request, sizeof(request), &offset))
        return CP0_PROVISION_FAILED;
    return run_request(request, offset, status, NULL, request_id,
                       CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_set_password(
    char *password, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[768] = {0};
    size_t offset = 0;
    uint64_t request_id = next_request_id++;
    size_t password_length = password == NULL ? 0 : strlen(password);
    int result = CP0_PROVISION_FAILED;
    if (password != NULL && status != NULL &&
        begin_command(request, sizeof(request), &offset, request_id,
                      "set-password") &&
        append_member(request, sizeof(request), &offset, "password", password) &&
        finish_command(request, sizeof(request), &offset))
        result = run_request(request, offset, status, NULL, request_id,
                             CP0_PROVISION_TIMEOUT_PASSWORD, error);
    clear_secret(request, sizeof(request));
    if (password != NULL)
        clear_secret(password, password_length);
    return result;
}

int cp0_provision_change_password(
    char *current_password, char *new_password,
    struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[1024] = {0};
    size_t offset = 0;
    uint64_t request_id = next_request_id++;
    size_t current_length =
        current_password == NULL ? 0 : strlen(current_password);
    size_t new_length = new_password == NULL ? 0 : strlen(new_password);
    int result = CP0_PROVISION_FAILED;
    if (current_password != NULL && new_password != NULL && status != NULL &&
        begin_command(request, sizeof(request), &offset, request_id,
                      "change-password") &&
        append_member(request, sizeof(request), &offset, "current_password",
                      current_password) &&
        append_member(request, sizeof(request), &offset, "new_password",
                      new_password) &&
        finish_command(request, sizeof(request), &offset))
        result = run_request(request, offset, status, NULL, request_id,
                             CP0_PROVISION_TIMEOUT_PASSWORD, error);
    clear_secret(request, sizeof(request));
    if (current_password != NULL)
        clear_secret(current_password, current_length);
    if (new_password != NULL)
        clear_secret(new_password, new_length);
    return result;
}

int cp0_provision_list_wifi(struct cp0_provision_wifi_list *list,
                            char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return list == NULL
               ? CP0_PROVISION_FAILED
               : command_no_fields("list-wifi", NULL, list,
                                   CP0_PROVISION_TIMEOUT_WIFI_SCAN, error);
}

int cp0_provision_connect_wifi(
    const char *ssid, enum cp0_provision_wifi_security security,
    char *password, bool hidden, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    static const char *security_names[] = {"open", "wpa2", "wpa3"};
    char request[1024] = {0};
    size_t offset = 0;
    uint64_t request_id = next_request_id++;
    size_t password_length = password == NULL ? 0 : strlen(password);
    int result = CP0_PROVISION_FAILED;
    if (ssid != NULL && password != NULL && status != NULL &&
        security < CP0_PROVISION_WIFI_UNSUPPORTED &&
        begin_command(request, sizeof(request), &offset, request_id,
                      "connect-wifi") &&
        append_member(request, sizeof(request), &offset, "ssid", ssid) &&
        append_member(request, sizeof(request), &offset, "security",
                      security_names[security]) &&
        append_member(request, sizeof(request), &offset, "password", password) &&
        append_bytes(request, sizeof(request), &offset,
                     hidden ? ",\"hidden\":true" : ",\"hidden\":false",
                     strlen(hidden ? ",\"hidden\":true"
                                   : ",\"hidden\":false")) &&
        finish_command(request, sizeof(request), &offset))
        result = run_request(request, offset, status, NULL, request_id,
                             CP0_PROVISION_TIMEOUT_WIFI_CONNECT, error);
    clear_secret(request, sizeof(request));
    if (password != NULL)
        clear_secret(password, password_length);
    return result;
}

int cp0_provision_use_ethernet(struct cp0_provision_status *status,
                               char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return status == NULL
               ? CP0_PROVISION_FAILED
               : command_no_fields("use-ethernet", status, NULL,
                                   CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_use_offline(struct cp0_provision_status *status,
                              char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return status == NULL
               ? CP0_PROVISION_FAILED
               : command_no_fields("use-offline", status, NULL,
                                   CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_set_ssh_enabled(
    bool enabled, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    char request[256];
    uint64_t request_id = next_request_id++;
    int count = snprintf(request, sizeof(request),
                         "{\"protocol_version\":1,\"request_id\":%llu,"
                         "\"command\":{\"name\":\"set-ssh-enabled\","
                         "\"enabled\":%s}}\n",
                         (unsigned long long)request_id,
                         enabled ? "true" : "false");
    if (status == NULL || count <= 0 || (size_t)count >= sizeof(request))
        return CP0_PROVISION_FAILED;
    return run_request(request, (size_t)count, status, NULL, request_id,
                       CP0_PROVISION_TIMEOUT_SYSTEM, error);
}

int cp0_provision_commit(struct cp0_provision_status *status,
                         char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return status == NULL
               ? CP0_PROVISION_FAILED
               : command_no_fields("commit", status, NULL,
                                   CP0_PROVISION_TIMEOUT_COMMIT, error);
}

#ifdef CP0_PROVISION_CLIENT_TEST
int cp0_provision_test_parse_state(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return parse_state_response(response, length, request_id, status, error);
}

int cp0_provision_test_parse_wifi(
    const char *response, size_t length, uint64_t request_id,
    struct cp0_provision_wifi_list *list,
    char error[CP0_PROVISION_ERROR_MAX + 1])
{
    return parse_wifi_response(response, length, request_id, list, error);
}

bool cp0_provision_test_escape(const char *input, char *output,
                               size_t capacity)
{
    size_t offset = 0;
    if (!append_json_string(output, capacity, &offset, input) ||
        offset >= capacity)
        return false;
    output[offset] = '\0';
    return true;
}
#endif
