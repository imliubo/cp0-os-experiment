#include "broker_client.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>

int main(void) {
    static const char success[] =
        "{\"protocol_version\":1,\"request_id\":2,\"outcome\":{"
        "\"status\":\"http-response\",\"status_code\":206,"
        "\"body_base64\":\"AAH+/3g=\"}}\n";
    static const char blocked[] =
        "{\"protocol_version\":1,\"request_id\":2,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"blocked-address\","
        "\"message\":\"blocked\"}}\n";
    uint8_t body[8] = {0};
    int64_t packed =
        cp0_broker_decode_http_response(success, body, sizeof(body));

    assert(packed >= 0);
    assert((uint16_t)((uint64_t)packed >> 32) == 206U);
    assert((uint32_t)packed == 5U);
    assert(memcmp(body, "\x00\x01\xfe\xffx", 5U) == 0);
    assert(cp0_broker_decode_http_response(success, body, 4U) ==
           CP0_BROKER_RESOURCE_LIMIT);
    assert(cp0_broker_decode_http_response(blocked, body, sizeof(body)) ==
           CP0_BROKER_DENIED);

    static const char opened[] =
        "{\"protocol_version\":1,\"request_id\":3,\"outcome\":{"
        "\"status\":\"document-opened\","
        "\"document_id\":\"00000000000000010000000000000002\","
        "\"size_bytes\":5}}\n";
    static const char pending[] =
        "{\"protocol_version\":1,\"request_id\":3,\"outcome\":{"
        "\"status\":\"document-selection-pending\",\"prompt_id\":8}}\n";
    int source = open("Cargo.toml", O_RDONLY);
    int descriptor = -1;
    uint32_t size_bytes = 0;
    assert(source >= 0);
    assert(cp0_broker_decode_document_response(opened, source, &descriptor,
                                               &size_bytes) == CP0_BROKER_OK);
    assert(descriptor == source && size_bytes == 5U);
    close(descriptor);
    source = open("Cargo.toml", O_RDONLY);
    assert(source >= 0);
    assert(cp0_broker_decode_document_response(pending, source, &descriptor,
                                               &size_bytes) ==
           CP0_BROKER_UNAVAILABLE);
    assert(fcntl(source, F_GETFD) < 0);

    static const char captured[] =
        "{\"protocol_version\":1,\"request_id\":5,\"outcome\":{"
        "\"status\":\"audio-captured\",\"samples_base64\":\"AID/fw==\"}}\n";
    uint8_t audio[4] = {0};
    assert(cp0_broker_decode_audio_capture_response(captured, audio,
                                                    sizeof(audio)) == 4);
    assert(memcmp(audio, "\x00\x80\xff\x7f", sizeof(audio)) == 0);
    assert(cp0_broker_decode_audio_capture_response(captured, audio, 2U) ==
           CP0_BROKER_INTERNAL);

    static const char camera_captured[] =
        "{\"protocol_version\":1,\"request_id\":6,\"outcome\":{"
        "\"status\":\"camera-captured\",\"width\":320,\"height\":170,"
        "\"pixel_format\":\"rgb565-le\",\"size_bytes\":108800}}\n";
    source = open("Cargo.toml", O_RDONLY);
    assert(source >= 0);
    descriptor = -1;
    assert(cp0_broker_decode_camera_response(camera_captured, source,
                                             &descriptor) == CP0_BROKER_OK);
    assert(descriptor == source);
    close(descriptor);
    static const char invalid_camera[] =
        "{\"protocol_version\":1,\"request_id\":6,\"outcome\":{"
        "\"status\":\"camera-captured\",\"width\":640,\"height\":170,"
        "\"pixel_format\":\"rgb565-le\",\"size_bytes\":108800}}\n";
    source = open("Cargo.toml", O_RDONLY);
    assert(source >= 0);
    descriptor = -1;
    assert(cp0_broker_decode_camera_response(invalid_camera, source,
                                             &descriptor) ==
           CP0_BROKER_INTERNAL);
    assert(descriptor == -1);
    assert(fcntl(source, F_GETFD) < 0);

    static const char gpio_value[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"gpio-value\",\"line\":\"grove-function\","
        "\"value\":true}}\n";
    static const char wrong_gpio_line[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"gpio-value\",\"line\":\"external-5v-power\","
        "\"value\":true}}\n";
    assert(cp0_broker_decode_gpio_response(gpio_value, 0, 0, 0) == 1);
    assert(cp0_broker_decode_gpio_response(wrong_gpio_line, 0, 0, 0) ==
           CP0_BROKER_INTERNAL);

    static const char lora_packet[] =
        "{\"protocol_version\":1,\"request_id\":10,\"outcome\":{"
        "\"status\":\"lora-packet\",\"payload_base64\":\"AAH+/w==\","
        "\"rssi_dbm\":-92,\"snr_quarter_db\":-5}}\n";
    static const char no_lora_packet[] =
        "{\"protocol_version\":1,\"request_id\":10,\"outcome\":{"
        "\"status\":\"lora-no-packet\"}}\n";
    uint8_t lora[8] = {0};
    uint8_t metadata[4] = {0};
    assert(cp0_broker_decode_lora_response(lora_packet, lora, sizeof(lora),
                                           metadata, sizeof(metadata)) == 4);
    assert(memcmp(lora, "\x00\x01\xfe\xff", 4U) == 0);
    assert(metadata[0] == 0xa4U && metadata[1] == 0xffU &&
           metadata[2] == 0xfbU && metadata[3] == 0U);
    assert(cp0_broker_decode_lora_response(no_lora_packet, lora,
                                           sizeof(lora), metadata,
                                           sizeof(metadata)) == 0);
    assert(memcmp(metadata, "\x00\x00\x00\x00", sizeof(metadata)) == 0);

    static const char storage_value[] =
        "{\"protocol_version\":1,\"request_id\":12,\"outcome\":{"
        "\"status\":\"storage-value\",\"value_base64\":\"AAH+/w==\"}}\n";
    static const char storage_missing[] =
        "{\"protocol_version\":1,\"request_id\":12,\"outcome\":{"
        "\"status\":\"storage-not-found\"}}\n";
    uint8_t stored[8] = {0};
    assert(cp0_broker_decode_storage_get_response(storage_value, stored,
                                                  sizeof(stored)) == 4);
    assert(memcmp(stored, "\x00\x01\xfe\xff", 4U) == 0);
    assert(cp0_broker_decode_storage_get_response(storage_missing, stored,
                                                  sizeof(stored)) == 0);

    static const char photo_imported[] =
        "{\"protocol_version\":1,\"request_id\":18,\"outcome\":{"
        "\"status\":\"photo-imported\",\"photo_id\":1722470400123}}\n";
    static const char photo_removed[] =
        "{\"protocol_version\":1,\"request_id\":19,\"outcome\":{"
        "\"status\":\"storage-deleted\",\"existed\":true}}\n";
    assert(cp0_broker_decode_photo_import_response(photo_imported) ==
           INT64_C(1722470400123));
    assert(cp0_broker_decode_photo_remove_response(photo_removed) == 1);

    static const char media_next[] =
        "{\"protocol_version\":1,\"request_id\":17,\"outcome\":{"
        "\"status\":\"media-action\",\"action\":\"next\"}}\n";
    static const char media_empty[] =
        "{\"protocol_version\":1,\"request_id\":17,\"outcome\":{"
        "\"status\":\"media-action-empty\"}}\n";
    static const char media_invalid[] =
        "{\"protocol_version\":1,\"request_id\":17,\"outcome\":{"
        "\"status\":\"media-action\",\"action\":\"seek\"}}\n";
    static const char media_unavailable[] =
        "{\"protocol_version\":1,\"request_id\":17,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"inactive\"}}\n";
    assert(cp0_broker_decode_media_action_response(media_next) == 3);
    assert(cp0_broker_decode_media_action_response(media_empty) == 0);
    assert(cp0_broker_decode_media_action_response(media_invalid) ==
           CP0_BROKER_INTERNAL);
    assert(cp0_broker_decode_media_action_response(media_unavailable) ==
           CP0_BROKER_UNAVAILABLE);
    assert(cp0_broker_decode_media_action_response(NULL) ==
           CP0_BROKER_INVALID_ARGUMENT);
    return 0;
}
