#include "cardputerzero.h"

static_assert(sizeof(cp0_key_event_t) == 8U, "key event ABI changed");
static_assert(sizeof(cp0_lora_metadata_t) == CP0_LORA_METADATA_BYTES,
              "LoRa metadata ABI changed");

extern "C" int sdk_cxx_smoke() {
    cp0_key_event_t event{};
    cp0_http_response_t response{};
    uint8_t body[32]{};
    static const uint8_t url[] = "https://example.com";
    cp0_document_t document{};
    int16_t audio_samples[4]{};
    uint32_t frames_captured{};
    static uint16_t camera_pixels[CP0_CAMERA_PIXEL_COUNT]{};
    uint8_t gpio_value{};
    uint8_t lora_payload[CP0_MAX_LORA_PAYLOAD_BYTES]{};
    cp0_lora_metadata_t lora_metadata{};
    uint32_t lora_payload_length{};
    (void)cp0_http_get(url, sizeof(url) - 1U, body, sizeof(body), &response);
    (void)cp0_document_open(&document);
    (void)cp0_audio_play(audio_samples, 4U);
    (void)cp0_audio_capture(audio_samples, 4U, &frames_captured);
    (void)cp0_camera_capture(camera_pixels, CP0_CAMERA_PIXEL_COUNT);
    (void)cp0_gpio_read(CP0_GPIO_GROVE_FUNCTION, &gpio_value);
    (void)cp0_gpio_write(CP0_GPIO_GROVE_FUNCTION, gpio_value);
    (void)cp0_lora_receive(lora_payload, sizeof(lora_payload), &lora_metadata,
                           1U, &lora_payload_length);
    return cp0_poll_key_event(&event, sizeof(event), 1);
}
