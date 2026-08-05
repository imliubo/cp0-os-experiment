#include "cp0_appd_client.h"

#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

int cp0_appd_test_parse_app_page(
    const char *response, size_t response_length, uint64_t request_id,
    uint16_t offset, struct cp0_app_summary *apps, size_t capacity,
    size_t *app_count, bool *has_next, uint16_t *next_offset);
int cp0_appd_test_parse_task_page(
    const char *response, size_t response_length, uint64_t request_id,
    uint8_t offset, struct cp0_task_summary *tasks, size_t capacity,
    size_t *task_count, bool *has_next, uint8_t *next_offset);
int cp0_appd_test_parse_lifecycle_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, const char *app_id);
int cp0_appd_test_parse_uninstall_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id);
bool cp0_appd_test_valid_app_id(const char *app_id);
bool cp0_appd_test_valid_document_id(const char *document_id);
int cp0_appd_test_parse_notification_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_notification *notification);
int cp0_appd_test_parse_document_prompt_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_document_prompt *prompt);
int cp0_appd_test_parse_device_settings_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, struct cp0_device_settings *settings);
int cp0_appd_test_parse_app_permissions_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, struct cp0_app_permissions *permissions);
int cp0_appd_test_parse_permission_reset_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, enum cp0_app_permission permission);
int cp0_appd_test_parse_media_action_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_action, char app_id[CP0_APP_ID_BYTES]);
void cp0_appd_test_convert_xrgb8888_to_rgb565_le(
    const uint32_t *source, unsigned char *destination, size_t pixel_count);
int cp0_appd_test_parse_screenshot_import_response(
    const char *response, size_t response_length, uint64_t request_id,
    uint64_t *photo_id);

int main(void)
{
    static const char page[] =
        "{\"protocol_version\":2,\"request_id\":7,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"applications\","
        "\"apps\":[{\"app_id\":\"dev.cardputerzero.first\","
        "\"name\":\"First Card\",\"version\":\"1.0.0\","
        "\"display\":\"standard\",\"running\":false,\"removable\":true,"
        "\"installed_at_unix_seconds\":1722470400,"
        "\"package_bytes\":65536,\"data_bytes\":4096,"
        "\"permissions\":[\"network.client\",\"camera.capture\"]},{"
        "\"app_id\":\"dev.cardputerzero.second\","
        "\"name\":\"Second Card\",\"version\":\"2.1.0\","
        "\"display\":\"immersive\",\"running\":true,\"removable\":false,"
        "\"installed_at_unix_seconds\":1722470500,"
        "\"package_bytes\":1000,\"data_bytes\":2000,"
        "\"permissions\":[]}],"
        "\"next_offset\":10}}}";
    struct cp0_app_summary apps[2];
    size_t count;
    bool has_next;
    uint16_t next_offset;
    assert(cp0_appd_test_parse_app_page(
               page, strlen(page), 7, 8, apps, 2, &count, &has_next,
               &next_offset) == 0);
    assert(count == 2 && has_next && next_offset == 10);
    assert(strcmp(apps[0].name, "First Card") == 0);
    assert(!apps[0].running && apps[0].removable && !apps[0].immersive);
    assert(apps[0].installed_at_unix_seconds == 1722470400);
    assert(apps[0].package_bytes == 65536 && apps[0].data_bytes == 4096);
    assert(apps[0].permissions ==
           (CP0_APP_PERMISSION_NETWORK_CLIENT |
            CP0_APP_PERMISSION_CAMERA_CAPTURE));
    assert(strcmp(apps[1].version, "2.1.0") == 0);
    assert(apps[1].running && !apps[1].removable && apps[1].immersive);
    assert(cp0_appd_test_parse_app_page(
               page, strlen(page), 8, 8, apps, 2, &count, &has_next,
               &next_offset) < 0);
    assert(cp0_appd_test_parse_app_page(
               page, strlen(page), 7, 8, apps, 1, &count, &has_next,
               &next_offset) < 0);

    static const char bad_display[] =
        "{\"protocol_version\":2,\"request_id\":9,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"applications\","
        "\"apps\":[{\"app_id\":\"dev.cardputerzero.bad\","
        "\"name\":\"Bad\",\"version\":\"1.0.0\","
        "\"display\":\"overlay\",\"running\":true,\"removable\":true,"
        "\"installed_at_unix_seconds\":1,\"package_bytes\":1,"
        "\"data_bytes\":0,\"permissions\":[]}],"
        "\"next_offset\":null}}}";
    assert(cp0_appd_test_parse_app_page(
               bad_display, strlen(bad_display), 9, 0, apps, 2, &count,
               &has_next, &next_offset) < 0);

    static const char permissions_response[] =
        "{\"protocol_version\":2,\"request_id\":23,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"application-permissions\","
        "\"app_id\":\"dev.cardputerzero.camera\",\"permissions\":["
        "{\"permission\":\"camera.capture\",\"decision\":\"denied\"},"
        "{\"permission\":\"photos.read\",\"decision\":\"allowed\"},"
        "{\"permission\":\"photos.write\",\"decision\":\"ask\"},"
        "{\"permission\":\"network.client\","
        "\"decision\":\"policy-denied\"}]}}}";
    struct cp0_app_permissions permission_state;
    assert(cp0_appd_test_parse_app_permissions_response(
               permissions_response, strlen(permissions_response), 23,
               "dev.cardputerzero.camera", &permission_state) == 0);
    assert(permission_state.known ==
           (CP0_APP_PERMISSION_CAMERA_CAPTURE |
            CP0_APP_PERMISSION_PHOTOS_READ |
            CP0_APP_PERMISSION_PHOTOS_WRITE |
            CP0_APP_PERMISSION_NETWORK_CLIENT));
    assert(permission_state.allowed == CP0_APP_PERMISSION_PHOTOS_READ);
    assert(permission_state.denied == CP0_APP_PERMISSION_CAMERA_CAPTURE);
    assert(permission_state.policy_denied ==
           CP0_APP_PERMISSION_NETWORK_CLIENT);
    assert(cp0_appd_test_parse_app_permissions_response(
               permissions_response, strlen(permissions_response), 23,
               "dev.cardputerzero.other", &permission_state) < 0);

    static const char duplicate_permission[] =
        "{\"protocol_version\":2,\"request_id\":24,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"application-permissions\","
        "\"app_id\":\"dev.cardputerzero.camera\",\"permissions\":["
        "{\"permission\":\"camera.capture\",\"decision\":\"ask\"},"
        "{\"permission\":\"camera.capture\",\"decision\":\"denied\"}]}}}";
    assert(cp0_appd_test_parse_app_permissions_response(
               duplicate_permission, strlen(duplicate_permission), 24,
               "dev.cardputerzero.camera", &permission_state) < 0);

    static const char reset_response[] =
        "{\"protocol_version\":2,\"request_id\":25,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"permission-reset\","
        "\"app_id\":\"dev.cardputerzero.camera\","
        "\"permission\":\"camera.capture\"}}}";
    assert(cp0_appd_test_parse_permission_reset_response(
               reset_response, strlen(reset_response), 25,
               "dev.cardputerzero.camera",
               CP0_APP_PERMISSION_CAMERA_CAPTURE) == 0);
    assert(cp0_appd_test_parse_permission_reset_response(
               reset_response, strlen(reset_response), 25,
               "dev.cardputerzero.camera", CP0_APP_PERMISSION_PHOTOS_WRITE) <
           0);

    static const char task_page[] =
        "{\"protocol_version\":2,\"request_id\":10,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"tasks\","
        "\"tasks\":[{\"task_id\":4,\"account_uid\":20000,"
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"name\":\"First Card\",\"version\":\"1.0.0\","
        "\"display\":\"standard\",\"state\":\"foreground\","
        "\"created_sequence\":1,\"last_activated_sequence\":7,"
        "\"checkpoint\":{\"status\":\"not-requested\"},"
        "\"runtime_generation\":11,\"thumbnail_generation\":3},{"
        "\"task_id\":2,\"account_uid\":20001,"
        "\"app_id\":\"dev.cardputerzero.second\","
        "\"name\":\"Second Card\",\"version\":\"2.1.0\","
        "\"display\":\"immersive\",\"state\":\"checkpointed\","
        "\"created_sequence\":2,\"last_activated_sequence\":6,"
        "\"checkpoint\":{\"status\":\"available\","
        "\"schema_version\":1,\"bytes\":128},"
        "\"runtime_generation\":null,\"thumbnail_generation\":9}],"
        "\"next_offset\":null}}}";
    struct cp0_task_summary tasks[2];
    uint8_t task_next_offset;
    assert(cp0_appd_test_parse_task_page(
               task_page, strlen(task_page), 10, 0, tasks, 2, &count,
               &has_next, &task_next_offset) == 0);
    assert(count == 2 && !has_next);
    assert(tasks[0].task_id == 4 && tasks[0].account_uid == 20000 &&
           tasks[0].state == CP0_TASK_FOREGROUND);
    assert(tasks[0].runtime_generation == 11 &&
           tasks[0].thumbnail_generation == 3);
    assert(!tasks[0].checkpoint_available && !tasks[0].immersive);
    assert(tasks[1].state == CP0_TASK_CHECKPOINTED &&
           tasks[1].checkpoint_available && tasks[1].immersive);
    assert(tasks[1].runtime_generation == 0 &&
           tasks[1].thumbnail_generation == 9);
    assert(cp0_appd_test_parse_task_page(
               task_page, strlen(task_page), 10, 0, tasks, 1, &count,
               &has_next, &task_next_offset) < 0);

    static const char started[] =
        "{\"protocol_version\":2,\"request_id\":11,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"started\","
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"unit\":\"cardputerzero-app-20000.service\"}}}";
    assert(cp0_appd_test_parse_lifecycle_response(
               started, strlen(started), 11, "started",
               "dev.cardputerzero.first") == 0);
    assert(cp0_appd_test_parse_lifecycle_response(
               started, strlen(started), 11, "stopped",
               "dev.cardputerzero.first") < 0);
    assert(cp0_appd_test_parse_lifecycle_response(
               started, strlen(started), 11, "started",
               "dev.cardputerzero.second") < 0);

    static const char uninstalled[] =
        "{\"protocol_version\":2,\"request_id\":17,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"uninstalled\","
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"private_data_retained\":true,"
        "\"package_cleanup_pending\":false}}}";
    assert(cp0_appd_test_parse_uninstall_response(
               uninstalled, strlen(uninstalled), 17,
               "dev.cardputerzero.first") == 0);
    assert(cp0_appd_test_parse_uninstall_response(
               uninstalled, strlen(uninstalled), 17,
               "dev.cardputerzero.second") < 0);
    static const char destructive_uninstall[] =
        "{\"protocol_version\":2,\"request_id\":18,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"uninstalled\","
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"private_data_retained\":false,"
        "\"package_cleanup_pending\":false}}}";
    assert(cp0_appd_test_parse_uninstall_response(
               destructive_uninstall, strlen(destructive_uninstall), 18,
               "dev.cardputerzero.first") < 0);

    static const char notification_response[] =
        "{\"protocol_version\":2,\"request_id\":12,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"next-notification\","
        "\"notification\":{\"notification_id\":4,"
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"app_name\":\"First Card\",\"title\":\"Complete\","
        "\"body\":\"The operation completed\"}}}}";
    struct cp0_notification notification;
    assert(cp0_appd_test_parse_notification_response(
               notification_response, strlen(notification_response), 12,
               &notification) == 1);
    assert(notification.notification_id == 4);
    assert(strcmp(notification.app_name, "First Card") == 0);
    assert(strcmp(notification.title, "Complete") == 0);
    assert(strcmp(notification.body, "The operation completed") == 0);

    static const char empty_queue[] =
        "{\"protocol_version\":2,\"request_id\":13,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"next-notification\","
        "\"notification\":null}}}";
    assert(cp0_appd_test_parse_notification_response(
               empty_queue, strlen(empty_queue), 13, &notification) == 0);
    assert(cp0_appd_test_parse_notification_response(
               notification_response, strlen(notification_response), 13,
               &notification) < 0);

    assert(cp0_appd_test_valid_app_id("dev.cardputerzero.first"));
    assert(!cp0_appd_test_valid_app_id("../../etc"));
    assert(!cp0_appd_test_valid_app_id("Dev.cardputerzero.first"));
    assert(!cp0_appd_test_valid_app_id("dev..first"));

    static const char document_response[] =
        "{\"protocol_version\":2,\"request_id\":14,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"pending-document\","
        "\"prompt\":{\"prompt_id\":5,"
        "\"app_id\":\"dev.cardputerzero.first\","
        "\"app_name\":\"First Card\",\"documents\":[{"
        "\"document_id\":\"00000000000000010000000000000002\","
        "\"name\":\"notes.txt\",\"size_bytes\":17}]}}}}";
    struct cp0_document_prompt document_prompt;
    assert(cp0_appd_test_parse_document_prompt_response(
               document_response, strlen(document_response), 14,
               &document_prompt) == 1);
    assert(document_prompt.prompt_id == 5 &&
           document_prompt.document_count == 1);
    assert(strcmp(document_prompt.documents[0].name, "notes.txt") == 0);
    assert(document_prompt.documents[0].size_bytes == 17);
    assert(cp0_appd_test_valid_document_id(
        "00000000000000010000000000000002"));
    assert(!cp0_appd_test_valid_document_id("../../etc/passwd"));

    static const char settings_response[] =
        "{\"protocol_version\":2,\"request_id\":15,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"device-settings\","
        "\"settings\":{\"authority\":\"organization\","
        "\"developer_mode\":false,\"developer_mode_allowed\":false,"
        "\"recovery_mode\":true,\"recovery_mode_allowed\":true,"
        "\"store_install_allowed\":false,"
        "\"app_launch_restricted\":true,"
        "\"denied_permission_count\":3}}}}";
    struct cp0_device_settings settings;
    assert(cp0_appd_test_parse_device_settings_response(
               settings_response, strlen(settings_response), 15,
               "device-settings", &settings) == 0);
    assert(settings.authority == CP0_AUTHORITY_ORGANIZATION);
    assert(!settings.developer_mode && !settings.developer_mode_allowed);
    assert(settings.recovery_mode && settings.recovery_mode_allowed);
    assert(!settings.store_install_allowed && settings.app_launch_restricted);
    assert(settings.denied_permission_count == 3);
    assert(cp0_appd_test_parse_device_settings_response(
               settings_response, strlen(settings_response), 16,
               "device-settings", &settings) < 0);

    static const char invalid_settings[] =
        "{\"protocol_version\":2,\"request_id\":16,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"device-settings\","
        "\"settings\":{\"authority\":\"remote\","
        "\"developer_mode\":false,\"developer_mode_allowed\":false,"
        "\"recovery_mode\":false,\"recovery_mode_allowed\":true,"
        "\"store_install_allowed\":false,"
        "\"app_launch_restricted\":true,"
        "\"denied_permission_count\":11}}}}";
    assert(cp0_appd_test_parse_device_settings_response(
               invalid_settings, strlen(invalid_settings), 16,
               "device-settings", &settings) < 0);

    static const char media_sent[] =
        "{\"protocol_version\":2,\"request_id\":20,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{"
        "\"kind\":\"media-action-dispatched\","
        "\"app_id\":\"dev.cardputerzero.player\","
        "\"action\":\"play-pause\"}}}";
    static const char media_unavailable[] =
        "{\"protocol_version\":2,\"request_id\":20,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unavailable\","
        "\"message\":\"inactive\"}}";
    static const char media_busy[] =
        "{\"protocol_version\":2,\"request_id\":20,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"resource-exhausted\","
        "\"message\":\"full\"}}";
    static const char media_extra[] =
        "{\"protocol_version\":2,\"request_id\":20,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{"
        "\"kind\":\"media-action-dispatched\","
        "\"app_id\":\"dev.cardputerzero.player\","
        "\"action\":\"play-pause\",\"target\":\"forged\"}}}";
    char media_app_id[CP0_APP_ID_BYTES];
    assert(cp0_appd_test_parse_media_action_response(
               media_sent, strlen(media_sent), 20, "play-pause",
               media_app_id) == CP0_MEDIA_DISPATCH_SENT);
    assert(strcmp(media_app_id, "dev.cardputerzero.player") == 0);
    assert(cp0_appd_test_parse_media_action_response(
               media_sent, strlen(media_sent), 20, "next", media_app_id) ==
           CP0_MEDIA_DISPATCH_FAILED);
    assert(cp0_appd_test_parse_media_action_response(
               media_unavailable, strlen(media_unavailable), 20, "next",
               media_app_id) == CP0_MEDIA_DISPATCH_UNAVAILABLE);
    assert(cp0_appd_test_parse_media_action_response(
               media_busy, strlen(media_busy), 20, "next", media_app_id) ==
           CP0_MEDIA_DISPATCH_BUSY);
    assert(cp0_appd_test_parse_media_action_response(
               media_extra, strlen(media_extra), 20, "play-pause",
               media_app_id) == CP0_MEDIA_DISPATCH_FAILED);

    static const uint32_t xrgb[] = {
        0x00000000U,
        0x00ffffffU,
        0x00ff0000U,
        0x0000ff00U,
        0x000000ffU,
    };
    static const unsigned char expected_rgb565[] = {
        0x00, 0x00, 0xff, 0xff, 0x00, 0xf8, 0xe0, 0x07, 0x1f, 0x00,
    };
    unsigned char rgb565[sizeof(expected_rgb565)];
    cp0_appd_test_convert_xrgb8888_to_rgb565_le(
        xrgb, rgb565, sizeof(xrgb) / sizeof(xrgb[0]));
    assert(memcmp(rgb565, expected_rgb565, sizeof(rgb565)) == 0);

    static const char screenshot_imported[] =
        "{\"protocol_version\":2,\"request_id\":21,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"screenshot-imported\","
        "\"photo_id\":1722470400123}}}";
    static const char screenshot_extra[] =
        "{\"protocol_version\":2,\"request_id\":21,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"screenshot-imported\","
        "\"photo_id\":1722470400123,\"path\":\"/host/leak\"}}}";
    uint64_t photo_id = 0;
    assert(cp0_appd_test_parse_screenshot_import_response(
               screenshot_imported, strlen(screenshot_imported), 21,
               &photo_id) == 0);
    assert(photo_id == 1722470400123ULL);
    assert(cp0_appd_test_parse_screenshot_import_response(
               screenshot_imported, strlen(screenshot_imported), 22,
               &photo_id) < 0);
    assert(cp0_appd_test_parse_screenshot_import_response(
               screenshot_extra, strlen(screenshot_extra), 21, &photo_id) <
           0);
    return 0;
}
