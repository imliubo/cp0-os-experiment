#include "cardputerzero.h"

static_assert(sizeof(cp0_key_event_t) == 8U, "key event ABI changed");

extern "C" int sdk_cxx_smoke() {
    cp0_key_event_t event{};
    cp0_http_response_t response{};
    uint8_t body[32]{};
    static const uint8_t url[] = "https://example.com";
    cp0_document_t document{};
    int16_t audio_samples[4]{};
    uint32_t frames_captured{};
    (void)cp0_http_get(url, sizeof(url) - 1U, body, sizeof(body), &response);
    (void)cp0_document_open(&document);
    (void)cp0_audio_play(audio_samples, 4U);
    (void)cp0_audio_capture(audio_samples, 4U, &frames_captured);
    return cp0_poll_key_event(&event, sizeof(event), 1);
}
