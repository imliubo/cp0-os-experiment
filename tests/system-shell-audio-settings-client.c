#include "cp0_audio_settings_client.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

int cp0_audio_settings_test_parse_state_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_audio_output_state *state);
int cp0_audio_settings_test_parse_key_click_response(
    const char *response, size_t response_length, uint64_t request_id);
int cp0_audio_settings_test_parse_key_sounds_response(
    const char *response, size_t response_length, uint64_t request_id,
    bool enabled);

int main(void)
{
    static const char available[] =
        "{\"protocol_version\":3,\"request_id\":7,\"outcome\":{"
        "\"status\":\"output-state\",\"state\":{\"available\":true,"
        "\"volume_percent\":75,\"muted\":false}}}";
    static const char unavailable[] =
        "{\"protocol_version\":3,\"request_id\":8,\"outcome\":{"
        "\"status\":\"output-state\",\"state\":{\"available\":false,"
        "\"volume_percent\":null,\"muted\":null}}}";
    static const char device_error[] =
        "{\"protocol_version\":3,\"request_id\":9,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"audio output is unavailable\"}}";
    static const char inconsistent[] =
        "{\"protocol_version\":3,\"request_id\":10,\"outcome\":{"
        "\"status\":\"output-state\",\"state\":{\"available\":false,"
        "\"volume_percent\":70,\"muted\":null}}}";
    static const char key_click[] =
        "{\"protocol_version\":3,\"request_id\":11,\"outcome\":{"
        "\"status\":\"played\",\"frames\":512}}";
    static const char key_sounds[] =
        "{\"protocol_version\":3,\"request_id\":12,\"outcome\":{"
        "\"status\":\"key-sounds-state\",\"enabled\":false}}";
    struct cp0_audio_output_state state;

    assert(cp0_audio_settings_test_parse_state_response(
               available, strlen(available), 7, &state) ==
           CP0_AUDIO_SETTINGS_OK);
    assert(state.available && state.volume_percent == 75 && !state.muted);
    assert(cp0_audio_settings_test_parse_state_response(
               available, strlen(available), 6, &state) ==
           CP0_AUDIO_SETTINGS_FAILED);
    assert(cp0_audio_settings_test_parse_state_response(
               unavailable, strlen(unavailable), 8, &state) ==
           CP0_AUDIO_SETTINGS_UNAVAILABLE);
    assert(!state.available && state.volume_percent == 0 && !state.muted);
    assert(cp0_audio_settings_test_parse_state_response(
               device_error, strlen(device_error), 9, &state) ==
           CP0_AUDIO_SETTINGS_UNAVAILABLE);
    assert(cp0_audio_settings_test_parse_state_response(
               inconsistent, strlen(inconsistent), 10, &state) ==
           CP0_AUDIO_SETTINGS_FAILED);
    assert(cp0_audio_settings_test_parse_key_click_response(
               key_click, strlen(key_click), 11) == CP0_AUDIO_SETTINGS_OK);
    assert(cp0_audio_settings_test_parse_key_sounds_response(
               key_sounds, strlen(key_sounds), 12, false) ==
           CP0_AUDIO_SETTINGS_OK);
    assert(cp0_audio_settings_test_parse_key_sounds_response(
               key_sounds, strlen(key_sounds), 12, true) ==
           CP0_AUDIO_SETTINGS_FAILED);
    return 0;
}
