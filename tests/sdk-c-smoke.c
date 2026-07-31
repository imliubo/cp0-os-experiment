#include "cardputerzero.h"

_Static_assert(sizeof(cp0_key_event_t) == 8U, "key event ABI changed");
_Static_assert(sizeof(cp0_rect_t) == 8U, "damage rectangle ABI changed");
_Static_assert(sizeof(cp0_lora_metadata_t) == CP0_LORA_METADATA_BYTES,
               "LoRa metadata ABI changed");

int sdk_c_smoke(void) {
    static const uint8_t title[] = "C SDK";
    static const uint8_t body[] = "ready";
    cp0_result_t result = cp0_post_notification(
        title, (uint32_t)(sizeof(title) - 1U), body,
        (uint32_t)(sizeof(body) - 1U));
    cp0_key_event_t event = {0};
    cp0_http_response_t response = {0};
    uint8_t network_body[64] = {0};
    static const uint8_t url[] = "https://example.com";
    cp0_document_t document = {0};
    uint32_t bytes_read = 0;
    int16_t audio_samples[8] = {0};
    uint32_t frames_captured = 0;
    static uint16_t camera_pixels[CP0_CAMERA_PIXEL_COUNT];
    uint8_t gpio_value = 0;
    uint8_t lora_payload[CP0_MAX_LORA_PAYLOAD_BYTES] = {0};
    cp0_lora_metadata_t lora_metadata = {0};
    uint32_t lora_payload_length = 0;
    (void)cp0_poll_key_event(&event, sizeof(event), 0);
    (void)cp0_display_dimensions();
    (void)cp0_http_get(url, (uint32_t)(sizeof(url) - 1U), network_body,
                       sizeof(network_body), &response);
    if (cp0_document_open(&document) == CP0_OK) {
        (void)cp0_document_read(&document, 0, network_body,
                                sizeof(network_body), &bytes_read);
        (void)cp0_document_close(&document);
    }
    (void)cp0_audio_play(audio_samples, 8U);
    (void)cp0_audio_capture(audio_samples, 8U, &frames_captured);
    (void)cp0_camera_capture(camera_pixels, CP0_CAMERA_PIXEL_COUNT);
    (void)cp0_gpio_read(CP0_GPIO_GROVE_FUNCTION, &gpio_value);
    (void)cp0_gpio_write(CP0_GPIO_GROVE_FUNCTION, gpio_value);
    (void)cp0_lora_receive(lora_payload, sizeof(lora_payload), &lora_metadata,
                           1U, &lora_payload_length);
    return result == CP0_ERROR_UNAVAILABLE ? (int)cp0_monotonic_milliseconds()
                                           : (int)result;
}
