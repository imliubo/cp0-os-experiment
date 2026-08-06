#define _POSIX_C_SOURCE 200809L

#include "cp0_audio_settings_client.h"
#include "cp0_json.h"

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

#ifndef CP0_AUDIO_SETTINGS_SOCKET
#define CP0_AUDIO_SETTINGS_SOCKET "/run/cardputerzero-audiod/audio.sock"
#endif

#define CP0_AUDIO_SETTINGS_FRAME_BYTES 4096U
#define CP0_AUDIO_SETTINGS_JSON_TOKENS 64U
#define CP0_AUDIO_SETTINGS_TIMEOUT_MSEC 500U

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
    const struct timeval timeout = {
        .tv_sec = 0,
        .tv_usec = CP0_AUDIO_SETTINGS_TIMEOUT_MSEC * 1000U,
    };
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    socklen_t address_length =
        (socklen_t)(offsetof(struct sockaddr_un, sun_path) +
                    strlen(CP0_AUDIO_SETTINGS_SOCKET) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    size_t length = 0;
    int result = -1;

    if (descriptor < 0 || request == NULL || response == NULL ||
        response_capacity < 2U)
        goto cleanup;
    if (fcntl(descriptor, F_SETFD, FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO, &timeout,
                   sizeof(timeout)) != 0 ||
        strlen(CP0_AUDIO_SETTINGS_SOCKET) >= sizeof(address.sun_path))
        goto cleanup;
    memcpy(address.sun_path, CP0_AUDIO_SETTINGS_SOCKET,
           strlen(CP0_AUDIO_SETTINGS_SOCKET) + 1U);
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

static int parse_state_response(const char *document, size_t length,
                                uint64_t request_id,
                                struct cp0_audio_output_state *state)
{
    struct cp0_json_token tokens[CP0_AUDIO_SETTINGS_JSON_TOKENS];
    int count = cp0_json_parse(document, length, tokens,
                               CP0_AUDIO_SETTINGS_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    int version_token;
    int id_token;
    int outcome;
    int status;

    if (document == NULL || state == NULL || count <= 0 ||
        tokens[0].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                        "protocol_version");
    id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "request_id");
    outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                  "outcome");
    if (version_token < 0 || id_token < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &parsed_id) ||
        version != 3U || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    if (status < 0 || tokens[status].type != CP0_JSON_STRING)
        return CP0_AUDIO_SETTINGS_FAILED;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "code");
        if (code >= 0 && cp0_json_string_equals(document, &tokens[code],
                                                 "unavailable"))
            return CP0_AUDIO_SETTINGS_UNAVAILABLE;
        return CP0_AUDIO_SETTINGS_FAILED;
    }
    if (!cp0_json_string_equals(document, &tokens[status], "output-state"))
        return CP0_AUDIO_SETTINGS_FAILED;

    int state_token = cp0_json_object_get(document, tokens, (size_t)count,
                                          outcome, "state");
    if (state_token < 0 || tokens[state_token].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    int available_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "available");
    int volume_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "volume_percent");
    int muted_token = cp0_json_object_get(
        document, tokens, (size_t)count, state_token, "muted");
    bool available;
    if (available_token < 0 || volume_token < 0 || muted_token < 0 ||
        !cp0_json_get_bool(document, &tokens[available_token], &available))
        return CP0_AUDIO_SETTINGS_FAILED;
    if (!available) {
        if (!cp0_json_is_null(document, &tokens[volume_token]) ||
            !cp0_json_is_null(document, &tokens[muted_token]))
            return CP0_AUDIO_SETTINGS_FAILED;
        *state = (struct cp0_audio_output_state){0};
        return CP0_AUDIO_SETTINGS_UNAVAILABLE;
    }
    uint64_t percent;
    bool muted;
    if (!cp0_json_get_u64(document, &tokens[volume_token], &percent) ||
        percent > 100U ||
        !cp0_json_get_bool(document, &tokens[muted_token], &muted))
        return CP0_AUDIO_SETTINGS_FAILED;
    *state = (struct cp0_audio_output_state){
        .available = true,
        .muted = muted,
        .volume_percent = (unsigned int)percent,
    };
    return CP0_AUDIO_SETTINGS_OK;
}

static int command(const char *name, const char *member,
                   struct cp0_audio_output_state *state)
{
    char request[256];
    char response[CP0_AUDIO_SETTINGS_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int count;

    if (name == NULL || state == NULL)
        return CP0_AUDIO_SETTINGS_FAILED;
    count = snprintf(request, sizeof(request),
                     "{\"protocol_version\":3,\"request_id\":%llu,"
                     "\"command\":{\"name\":\"%s\"%s}}\n",
                     (unsigned long long)request_id, name,
                     member == NULL ? "" : member);
    if (count <= 0 || (size_t)count >= sizeof(request) ||
        exchange(request, (size_t)count, response, sizeof(response),
                 &response_length) != 0)
        return CP0_AUDIO_SETTINGS_FAILED;
    return parse_state_response(response, response_length, request_id, state);
}

int cp0_audio_get_output_state(struct cp0_audio_output_state *state)
{
    return command("get-output-state", NULL, state);
}

int cp0_audio_set_output_volume(unsigned int percent,
                                struct cp0_audio_output_state *state)
{
    char member[48];
    int count;
    if (percent > 100U)
        return CP0_AUDIO_SETTINGS_FAILED;
    count = snprintf(member, sizeof(member), ",\"percent\":%u", percent);
    if (count <= 0 || (size_t)count >= sizeof(member))
        return CP0_AUDIO_SETTINGS_FAILED;
    return command("set-output-volume", member, state);
}

int cp0_audio_adjust_output_volume(enum cp0_audio_settings_direction direction,
                                   struct cp0_audio_output_state *state)
{
    const char *value;
    if (direction == CP0_AUDIO_SETTINGS_DECREASE)
        value = "decrease";
    else if (direction == CP0_AUDIO_SETTINGS_INCREASE)
        value = "increase";
    else
        return CP0_AUDIO_SETTINGS_FAILED;
    char member[64];
    int count = snprintf(member, sizeof(member),
                         ",\"direction\":\"%s\"", value);
    if (count <= 0 || (size_t)count >= sizeof(member))
        return CP0_AUDIO_SETTINGS_FAILED;
    return command("adjust-output-volume", member, state);
}

int cp0_audio_set_output_muted(bool muted,
                               struct cp0_audio_output_state *state)
{
    return command("set-output-muted",
                   muted ? ",\"muted\":true" : ",\"muted\":false",
                   state);
}

static int parse_key_click_response(const char *document, size_t length,
                                    uint64_t request_id)
{
    struct cp0_json_token tokens[CP0_AUDIO_SETTINGS_JSON_TOKENS];
    int count = cp0_json_parse(document, length, tokens,
                               CP0_AUDIO_SETTINGS_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    uint64_t frames;
    int version_token;
    int id_token;
    int outcome;
    int status;
    int frames_token;

    if (document == NULL || count <= 0 || tokens[0].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                        "protocol_version");
    id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "request_id");
    outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                  "outcome");
    if (version_token < 0 || id_token < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &parsed_id) ||
        version != 3U || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    if (status < 0 || tokens[status].type != CP0_JSON_STRING)
        return CP0_AUDIO_SETTINGS_FAILED;
    if (cp0_json_string_equals(document, &tokens[status], "error")) {
        int code = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "code");
        return code >= 0 && cp0_json_string_equals(document, &tokens[code],
                                                    "unavailable")
                   ? CP0_AUDIO_SETTINGS_UNAVAILABLE
                   : CP0_AUDIO_SETTINGS_FAILED;
    }
    frames_token = cp0_json_object_get(document, tokens, (size_t)count,
                                       outcome, "frames");
    if (!cp0_json_string_equals(document, &tokens[status], "played") ||
        frames_token < 0 ||
        !cp0_json_get_u64(document, &tokens[frames_token], &frames) ||
        frames != 240U)
        return CP0_AUDIO_SETTINGS_FAILED;
    return CP0_AUDIO_SETTINGS_OK;
}

int cp0_audio_play_key_click(void)
{
    char request[192];
    char response[CP0_AUDIO_SETTINGS_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int count = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":3,\"request_id\":%llu,"
        "\"command\":{\"name\":\"play-key-click\"}}\n",
        (unsigned long long)request_id);
    if (count <= 0 || (size_t)count >= sizeof(request) ||
        exchange(request, (size_t)count, response, sizeof(response),
                 &response_length) != 0)
        return CP0_AUDIO_SETTINGS_FAILED;
    return parse_key_click_response(response, response_length, request_id);
}

static int parse_key_sounds_response(const char *document, size_t length,
                                     uint64_t request_id, bool enabled)
{
    struct cp0_json_token tokens[CP0_AUDIO_SETTINGS_JSON_TOKENS];
    int count = cp0_json_parse(document, length, tokens,
                               CP0_AUDIO_SETTINGS_JSON_TOKENS);
    uint64_t version;
    uint64_t parsed_id;
    bool parsed_enabled;
    int version_token;
    int id_token;
    int outcome;
    int status;
    int enabled_token;

    if (document == NULL || count <= 0 || tokens[0].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    version_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                        "protocol_version");
    id_token = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                   "request_id");
    outcome = cp0_json_object_get(document, tokens, (size_t)count, 0,
                                  "outcome");
    if (version_token < 0 || id_token < 0 || outcome < 0 ||
        !cp0_json_get_u64(document, &tokens[version_token], &version) ||
        !cp0_json_get_u64(document, &tokens[id_token], &parsed_id) ||
        version != 3U || parsed_id != request_id ||
        tokens[outcome].type != CP0_JSON_OBJECT)
        return CP0_AUDIO_SETTINGS_FAILED;
    status = cp0_json_object_get(document, tokens, (size_t)count, outcome,
                                 "status");
    enabled_token = cp0_json_object_get(document, tokens, (size_t)count,
                                        outcome, "enabled");
    if (status < 0 || enabled_token < 0 ||
        !cp0_json_string_equals(document, &tokens[status],
                                "key-sounds-state") ||
        !cp0_json_get_bool(document, &tokens[enabled_token], &parsed_enabled) ||
        parsed_enabled != enabled)
        return CP0_AUDIO_SETTINGS_FAILED;
    return CP0_AUDIO_SETTINGS_OK;
}

int cp0_audio_set_key_sounds_enabled(bool enabled)
{
    char request[224];
    char response[CP0_AUDIO_SETTINGS_FRAME_BYTES];
    size_t response_length;
    uint64_t request_id = next_request_id++;
    int count = snprintf(
        request, sizeof(request),
        "{\"protocol_version\":3,\"request_id\":%llu,"
        "\"command\":{\"name\":\"set-key-sounds-enabled\","
        "\"enabled\":%s}}\n",
        (unsigned long long)request_id, enabled ? "true" : "false");
    if (count <= 0 || (size_t)count >= sizeof(request) ||
        exchange(request, (size_t)count, response, sizeof(response),
                 &response_length) != 0)
        return CP0_AUDIO_SETTINGS_FAILED;
    return parse_key_sounds_response(response, response_length, request_id,
                                     enabled);
}

#ifdef CP0_AUDIO_SETTINGS_CLIENT_TEST
int cp0_audio_settings_test_parse_state_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_audio_output_state *state)
{
    return parse_state_response(response, response_length, request_id, state);
}

int cp0_audio_settings_test_parse_key_click_response(
    const char *response, size_t response_length, uint64_t request_id)
{
    return parse_key_click_response(response, response_length, request_id);
}

int cp0_audio_settings_test_parse_key_sounds_response(
    const char *response, size_t response_length, uint64_t request_id,
    bool enabled)
{
    return parse_key_sounds_response(response, response_length, request_id,
                                     enabled);
}
#endif
