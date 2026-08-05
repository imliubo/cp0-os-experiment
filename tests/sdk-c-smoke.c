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
    static const uint8_t storage_key[] = "state";
    uint8_t storage_value[16] = {1};
    uint32_t storage_value_length = 0;
    uint8_t storage_existed = 0;
    static const uint8_t photo_key[] = "photo-0001-meta";
    uint8_t photo_value[32] = {1};
    uint32_t photo_value_length = 0;
    uint8_t photo_existed = 0;
    uint64_t photo_id = 0;
    static const uint8_t intent_action[] = "dev.cardputerzero.documents.open";
    uint8_t intent_action_buffer[CP0_MAX_INTENT_ACTION_BYTES] = {0};
    uint8_t intent_payload[32] = {0};
    cp0_intent_message_t intent_message = {0};
    cp0_media_action_t media_action = CP0_MEDIA_PLAY_PAUSE;
    uint8_t media_action_available = 0;
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
    (void)cp0_camera_capture_photo(&photo_id);
    (void)cp0_gpio_read(CP0_GPIO_GROVE_FUNCTION, &gpio_value);
    (void)cp0_gpio_write(CP0_GPIO_GROVE_FUNCTION, gpio_value);
    (void)cp0_lora_receive(lora_payload, sizeof(lora_payload), &lora_metadata,
                           1U, &lora_payload_length);
    (void)cp0_storage_put(storage_key, sizeof(storage_key) - 1U,
                          storage_value, sizeof(storage_value));
    (void)cp0_storage_get(storage_key, sizeof(storage_key) - 1U,
                          storage_value, sizeof(storage_value),
                          &storage_value_length);
    (void)cp0_storage_delete(storage_key, sizeof(storage_key) - 1U,
                             &storage_existed);
    (void)cp0_photos_put(photo_key, sizeof(photo_key) - 1U, photo_value,
                         sizeof(photo_value));
    (void)cp0_photos_get(photo_key, sizeof(photo_key) - 1U, photo_value,
                         sizeof(photo_value), &photo_value_length);
    (void)cp0_photos_index_get_for_update(
        photo_value, sizeof(photo_value), &photo_value_length);
    (void)cp0_photos_delete(photo_key, sizeof(photo_key) - 1U,
                            &photo_existed);
    (void)cp0_photos_import_rgb565(camera_pixels, CP0_CAMERA_PIXEL_COUNT, 1,
                                   &photo_id);
    (void)cp0_photos_load_rgb565(photo_id, camera_pixels,
                                 CP0_CAMERA_PIXEL_COUNT);
    (void)cp0_photos_load_view_rgb565(
        photo_id, CP0_PHOTO_VIEW_ACTUAL, 0, 0, camera_pixels,
        CP0_CAMERA_PIXEL_COUNT);
    (void)cp0_photos_remove(photo_id, &photo_existed);
    (void)cp0_intent_send(intent_action, sizeof(intent_action) - 1U,
                          intent_payload, sizeof(intent_payload));
    (void)cp0_intent_take(intent_action_buffer, sizeof(intent_action_buffer),
                          intent_payload, sizeof(intent_payload),
                          &intent_message);
    (void)cp0_media_session_update(
        CP0_MEDIA_PLAYING,
        CP0_MEDIA_SUPPORT_PLAY_PAUSE | CP0_MEDIA_SUPPORT_NEXT);
    (void)cp0_media_take_action(&media_action, &media_action_available);
    return result == CP0_ERROR_UNAVAILABLE ? (int)cp0_monotonic_milliseconds()
                                           : (int)result;
}
