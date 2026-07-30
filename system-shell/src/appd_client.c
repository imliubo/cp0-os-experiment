#include "cp0_appd_client.h"
#include "cp0_json.h"

#include <errno.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>
#include <unistd.h>

#define CP0_APPD_SOCKET "/run/cardputerzero-appd/control.sock"
#define CP0_APPD_FRAME_BYTES 8192U
#define CP0_APPD_JSON_TOKENS 128U

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
                    size_t response_capacity, size_t *response_length)
{
    const struct timeval timeout = {.tv_sec = 0, .tv_usec = 250000};
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    int descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2)
        goto cleanup;
    if (setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        strlen(CP0_APPD_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_APPD_SOCKET, sizeof(CP0_APPD_SOCKET));
    if (connect(descriptor, (const struct sockaddr *)&address,
                sizeof(address)) != 0 ||
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
                 &response_length) != 0 ||
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
                 &response_length) != 0 ||
        parse_success(response, response_length, request_id, tokens,
                      &token_count, &data) != 0)
        return -1;
    int kind = cp0_json_object_get(response, tokens, token_count, data, "kind");
    return kind >= 0 && cp0_json_string_equals(
                            response, &tokens[kind], "permission-resolved")
               ? 0
               : -1;
}
