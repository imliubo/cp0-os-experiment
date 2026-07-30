#include "cp0_appd_client.h"

#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

int cp0_appd_test_parse_app_page(
    const char *response, size_t response_length, uint64_t request_id,
    uint16_t offset, struct cp0_app_summary *apps, size_t capacity,
    size_t *app_count, bool *has_next, uint16_t *next_offset);
int cp0_appd_test_parse_lifecycle_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *expected_kind, const char *app_id);
bool cp0_appd_test_valid_app_id(const char *app_id);
bool cp0_appd_test_valid_document_id(const char *document_id);
int cp0_appd_test_parse_notification_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_notification *notification);
int cp0_appd_test_parse_document_prompt_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_document_prompt *prompt);

int main(void)
{
    static const char page[] =
        "{\"protocol_version\":1,\"request_id\":7,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"applications\","
        "\"apps\":[{\"app_id\":\"dev.cardputerzero.first\","
        "\"name\":\"First Card\",\"version\":\"1.0.0\","
        "\"display\":\"standard\",\"running\":false},{"
        "\"app_id\":\"dev.cardputerzero.second\","
        "\"name\":\"Second Card\",\"version\":\"2.1.0\","
        "\"display\":\"immersive\",\"running\":true}],"
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
    assert(!apps[0].running && !apps[0].immersive);
    assert(strcmp(apps[1].version, "2.1.0") == 0);
    assert(apps[1].running && apps[1].immersive);
    assert(cp0_appd_test_parse_app_page(
               page, strlen(page), 8, 8, apps, 2, &count, &has_next,
               &next_offset) < 0);
    assert(cp0_appd_test_parse_app_page(
               page, strlen(page), 7, 8, apps, 1, &count, &has_next,
               &next_offset) < 0);

    static const char bad_display[] =
        "{\"protocol_version\":1,\"request_id\":9,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"applications\","
        "\"apps\":[{\"app_id\":\"dev.cardputerzero.bad\","
        "\"name\":\"Bad\",\"version\":\"1.0.0\","
        "\"display\":\"overlay\",\"running\":true}],"
        "\"next_offset\":null}}}";
    assert(cp0_appd_test_parse_app_page(
               bad_display, strlen(bad_display), 9, 0, apps, 2, &count,
               &has_next, &next_offset) < 0);

    static const char started[] =
        "{\"protocol_version\":1,\"request_id\":11,\"outcome\":{"
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

    static const char notification_response[] =
        "{\"protocol_version\":1,\"request_id\":12,\"outcome\":{"
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
        "{\"protocol_version\":1,\"request_id\":13,\"outcome\":{"
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
        "{\"protocol_version\":1,\"request_id\":14,\"outcome\":{"
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
    return 0;
}
