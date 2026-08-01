#include "cp0_store_client.h"

#include <assert.h>
#include <fcntl.h>
#include <png.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <unistd.h>

int cp0_store_test_parse_catalog_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_catalog *catalog);
int cp0_store_test_parse_today_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_today *today);
int cp0_store_test_parse_refresh_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id);
int cp0_store_test_parse_auto_update_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_auto_update_status *status);
int cp0_store_test_parse_metrics_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_metrics_status *status);
int cp0_store_test_parse_install_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id,
                                          const char *app_id);
int cp0_store_test_parse_install_preflight_response(
    const char *response, size_t response_length, uint64_t request_id,
    uint64_t catalog_sequence, const char *const app_ids[], size_t app_count,
    struct cp0_store_install_preflight *preflight);
int cp0_store_test_parse_install_batch_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *const app_ids[], size_t app_count);
int cp0_store_test_parse_control_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, enum cp0_store_control_action action);
int cp0_store_test_parse_search_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *query, uint16_t offset, uint8_t limit,
    struct cp0_store_search_results *results);
int cp0_store_test_parse_browse_response(
    const char *response, size_t response_length, uint64_t request_id,
    uint16_t offset, uint8_t limit, struct cp0_store_browse_results *results);
int cp0_store_test_parse_details_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, const char *version,
    struct cp0_store_app_details *details);
int cp0_store_test_parse_media_response(
    const char *response, size_t response_length, uint64_t request_id,
    const char *app_id, const char *version, bool screenshot, uint8_t index,
    struct cp0_store_image_metadata *metadata);
int cp0_store_test_receive_chunk(int socket_descriptor, char *buffer,
                                 size_t capacity, int *received_descriptor);
int cp0_store_test_decode_png_descriptor(
    int descriptor, const struct cp0_store_image_metadata *metadata,
    uint32_t *pixels, size_t pixel_capacity);

#define ENVELOPE_START(ID)                                                     \
    "{\"protocol_version\":1,\"request_id\":" ID ",\"outcome\":{"      \
    "\"status\":\"ok\",\"data\":{"
#define APP(ID, PERMISSIONS, STATE, PROGRESS)                                  \
    "{\"app_id\":\"dev.cardputerzero." ID "\",\"name\":\"App " ID       \
    "\",\"version\":\"1.2.3\",\"summary\":\"A reviewed application\"," \
    "\"package_bytes\":4096,\"permissions\":" PERMISSIONS ",\"state\":\"" \
    STATE "\",\"progress_percent\":" PROGRESS "}"
#define FAILED_APP(ID, REASON)                                                 \
    "{\"app_id\":\"dev.cardputerzero." ID "\",\"name\":\"App " ID       \
    "\",\"version\":\"1.2.3\",\"summary\":\"A reviewed application\"," \
    "\"package_bytes\":4096,\"permissions\":[],\"state\":\"failed\","   \
    "\"progress_percent\":0,\"failure_reason\":\"" REASON "\"}"
#define CATALOG_START(ID)                                                      \
    ENVELOPE_START(ID)                                                        \
    "\"kind\":\"catalog\",\"sequence\":4,\"expires_unix_seconds\":200," \
    "\"stale\":false,\"apps\":["
#define CATALOG_END "]}}}"
#define SEARCH_START(ID, QUERY, OFFSET, LIMIT, TOTAL, NEXT, STALE)             \
    ENVELOPE_START(ID)                                                        \
    "\"kind\":\"search-results\",\"query\":\"" QUERY                    \
    "\",\"offset\":" OFFSET ",\"limit\":" LIMIT ",\"total\":" TOTAL    \
    ",\"next_offset\":" NEXT ",\"sequence\":4,"                           \
    "\"expires_unix_seconds\":200,\"stale\":" STALE ",\"apps\":["
#define SEARCH_END "]}}}"
#define BROWSE_START(ID, OFFSET, LIMIT, TOTAL, NEXT, STALE)                    \
    ENVELOPE_START(ID)                                                        \
    "\"kind\":\"browse-results\",\"offset\":" OFFSET ",\"limit\":" LIMIT \
    ",\"total\":" TOTAL ",\"next_offset\":" NEXT ",\"sequence\":4,"       \
    "\"expires_unix_seconds\":200,\"stale\":" STALE ",\"apps\":["
#define BROWSE_END "]}}}"

static void send_descriptors(int socket_descriptor, const int *descriptors,
                             size_t descriptor_count)
{
    char byte = 'x';
    struct iovec vector = {.iov_base = &byte, .iov_len = 1};
    unsigned char control[CMSG_SPACE(sizeof(int) * 2U)] = {0};
    struct msghdr message = {
        .msg_iov = &vector,
        .msg_iovlen = 1,
        .msg_control = control,
        .msg_controllen = CMSG_SPACE(sizeof(int) * descriptor_count),
    };
    struct cmsghdr *header = CMSG_FIRSTHDR(&message);
    header->cmsg_level = SOL_SOCKET;
    header->cmsg_type = SCM_RIGHTS;
    header->cmsg_len = CMSG_LEN(sizeof(int) * descriptor_count);
    memcpy(CMSG_DATA(header), descriptors, sizeof(int) * descriptor_count);
    assert(sendmsg(socket_descriptor, &message, 0) == 1);
}

static void write_red_png(const char *path)
{
    FILE *output = fopen(path, "wb");
    assert(output != NULL);
    png_structp png =
        png_create_write_struct(PNG_LIBPNG_VER_STRING, NULL, NULL, NULL);
    assert(png != NULL);
    png_infop info = png_create_info_struct(png);
    assert(info != NULL);
    assert(setjmp(png_jmpbuf(png)) == 0);
    png_init_io(png, output);
    png_set_IHDR(png, info, 1, 1, 8, PNG_COLOR_TYPE_RGBA,
                 PNG_INTERLACE_NONE, PNG_COMPRESSION_TYPE_DEFAULT,
                 PNG_FILTER_TYPE_DEFAULT);
    png_write_info(png, info);
    unsigned char red[] = {0xff, 0x00, 0x00, 0xff};
    png_bytep rows[] = {red};
    png_write_image(png, rows);
    png_write_end(png, info);
    png_destroy_write_struct(&png, &info);
    assert(fclose(output) == 0);
}

static int parse_catalog(const char *response, uint64_t request_id,
                         struct cp0_store_catalog *catalog)
{
    return cp0_store_test_parse_catalog_response(
        response, strlen(response), request_id, catalog);
}

int main(void)
{
    struct cp0_store_catalog catalog;
    static const char valid[] =
        CATALOG_START("7")
        APP("alpha", "[\"camera.capture\",\"network.client\"]", "available",
            "0")
        ","
        APP("beta", "[\"notifications.post\"]", "downloading", "42")
        CATALOG_END;
    assert(parse_catalog(valid, 7, &catalog) == CP0_STORE_RESULT_OK);
    assert(catalog.sequence == 4 && catalog.count == 2 && !catalog.stale &&
           !catalog.truncated);

    struct cp0_store_today today;
    static const char valid_today[] =
        ENVELOPE_START("60")
        "\"kind\":\"today\",\"sequence\":4,\"expires_unix_seconds\":200,"
        "\"stale\":false,\"editorial\":{\"headline\":\"Made for CP0\","
        "\"featured\":" APP("alpha", "[]", "available", "0")
        ",\"collections\":[{\"title\":\"New and useful\",\"apps\":["
        APP("beta", "[]", "installed", "100")
        "]},{\"title\":\"Tools\",\"apps\":["
        APP("gamma", "[]", "available", "0") ","
        APP("delta", "[]", "available", "0") "]}]}}}}";
    assert(cp0_store_test_parse_today_response(
               valid_today, strlen(valid_today), 60, &today) ==
           CP0_STORE_RESULT_OK);
    assert(today.has_editorial && today.sequence == 4 && !today.stale &&
           today.collection_count == 2 &&
           strcmp(today.headline, "Made for CP0") == 0 &&
           strcmp(today.featured.app_id, "dev.cardputerzero.alpha") == 0 &&
           today.collections[0].count == 1 &&
           strcmp(today.collections[1].apps[1].app_id,
                  "dev.cardputerzero.delta") == 0);
    static const char legacy_today[] =
        ENVELOPE_START("61")
        "\"kind\":\"today\",\"sequence\":4,\"expires_unix_seconds\":200,"
        "\"stale\":true,\"editorial\":null}}}";
    assert(cp0_store_test_parse_today_response(
               legacy_today, strlen(legacy_today), 61, &today) ==
           CP0_STORE_RESULT_OK);
    assert(!today.has_editorial && today.stale && today.collection_count == 0);
    static const char duplicate_today[] =
        ENVELOPE_START("62")
        "\"kind\":\"today\",\"sequence\":4,\"expires_unix_seconds\":200,"
        "\"stale\":false,\"editorial\":{\"headline\":\"Duplicate\","
        "\"featured\":" APP("alpha", "[]", "available", "0")
        ",\"collections\":[{\"title\":\"Again\",\"apps\":["
        APP("alpha", "[]", "available", "0") "]}]}}}}";
    assert(cp0_store_test_parse_today_response(
               duplicate_today, strlen(duplicate_today), 62, &today) ==
           CP0_STORE_RESULT_ERROR);

    struct cp0_store_auto_update_status auto_update;
    static const char valid_auto_update[] =
        ENVELOPE_START("12")
        "\"kind\":\"auto-update-status\",\"enabled\":true,"
        "\"policy_allowed\":true,\"charging\":true,"
        "\"unmetered_network\":false,\"due\":true,"
        "\"checking\":false}}}";
    assert(cp0_store_test_parse_auto_update_response(
               valid_auto_update, strlen(valid_auto_update), 12,
               &auto_update) == CP0_STORE_RESULT_OK);
    assert(auto_update.enabled && auto_update.policy_allowed &&
           auto_update.charging && !auto_update.unmetered_network &&
           auto_update.due && !auto_update.checking);
    static const char invalid_disabled_due[] =
        ENVELOPE_START("13")
        "\"kind\":\"auto-update-status\",\"enabled\":false,"
        "\"policy_allowed\":true,\"charging\":true,"
        "\"unmetered_network\":true,\"due\":true,"
        "\"checking\":false}}}";
    assert(cp0_store_test_parse_auto_update_response(
               invalid_disabled_due, strlen(invalid_disabled_due), 13,
               &auto_update) == CP0_STORE_RESULT_ERROR);
    static const char invalid_auto_extra[] =
        ENVELOPE_START("14")
        "\"kind\":\"auto-update-status\",\"enabled\":true,"
        "\"policy_allowed\":true,\"charging\":true,"
        "\"unmetered_network\":true,\"due\":false,"
        "\"checking\":false,\"extra\":true}}}";
    assert(cp0_store_test_parse_auto_update_response(
               invalid_auto_extra, strlen(invalid_auto_extra), 14,
               &auto_update) == CP0_STORE_RESULT_ERROR);
    struct cp0_store_metrics_status metrics;
    static const char valid_metrics[] =
        ENVELOPE_START("15")
        "\"kind\":\"metrics-status\",\"enabled\":true,"
        "\"policy_allowed\":true,\"configured\":true,"
        "\"pending\":true}}}";
    assert(cp0_store_test_parse_metrics_response(
               valid_metrics, strlen(valid_metrics), 15, &metrics) ==
           CP0_STORE_RESULT_OK);
    assert(metrics.enabled && metrics.policy_allowed && metrics.configured &&
           metrics.pending);
    static const char invalid_metrics[] =
        ENVELOPE_START("16")
        "\"kind\":\"metrics-status\",\"enabled\":false,"
        "\"policy_allowed\":true,\"configured\":true,"
        "\"pending\":true}}}";
    assert(cp0_store_test_parse_metrics_response(
               invalid_metrics, strlen(invalid_metrics), 16, &metrics) ==
           CP0_STORE_RESULT_ERROR);
    assert(strcmp(catalog.apps[0].app_id, "dev.cardputerzero.alpha") == 0);
    assert(catalog.apps[0].permissions ==
           (CP0_STORE_PERMISSION_CAMERA_CAPTURE |
            CP0_STORE_PERMISSION_NETWORK_CLIENT));
    assert(catalog.apps[1].state == CP0_STORE_APP_DOWNLOADING &&
           catalog.apps[1].progress_percent == 42);

    assert(parse_catalog(valid, 8, &catalog) == CP0_STORE_RESULT_ERROR);
    static const char wrong_version[] =
        "{\"protocol_version\":2,\"request_id\":7,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"catalog\",\"sequence\":4,"
        "\"expires_unix_seconds\":200,\"stale\":false,\"apps\":[]}}}";
    assert(parse_catalog(wrong_version, 7, &catalog) ==
           CP0_STORE_RESULT_ERROR);

    static const char unsorted[] =
        CATALOG_START("9")
        APP("beta", "[]", "available", "0") ","
        APP("alpha", "[]", "available", "0")
        CATALOG_END;
    static const char duplicate[] =
        CATALOG_START("10")
        APP("alpha", "[]", "available", "0") ","
        APP("alpha", "[]", "available", "0")
        CATALOG_END;
    assert(parse_catalog(unsorted, 9, &catalog) == CP0_STORE_RESULT_ERROR);
    assert(parse_catalog(duplicate, 10, &catalog) == CP0_STORE_RESULT_ERROR);

    static const char unknown_permission[] =
        CATALOG_START("11")
        APP("alpha", "[\"filesystem.root\"]", "available", "0")
        CATALOG_END;
    static const char unsorted_permissions[] =
        CATALOG_START("12")
        APP("alpha", "[\"network.client\",\"camera.capture\"]", "available",
            "0")
        CATALOG_END;
    static const char duplicate_permissions[] =
        CATALOG_START("13")
        APP("alpha", "[\"camera.capture\",\"camera.capture\"]", "available",
            "0")
        CATALOG_END;
    assert(parse_catalog(unknown_permission, 11, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    assert(parse_catalog(unsorted_permissions, 12, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    assert(parse_catalog(duplicate_permissions, 13, &catalog) ==
           CP0_STORE_RESULT_ERROR);

    static const char invalid_available_progress[] =
        CATALOG_START("14")
        APP("alpha", "[]", "available", "1")
        CATALOG_END;
    static const char invalid_install_progress[] =
        CATALOG_START("15")
        APP("alpha", "[]", "installing", "99")
        CATALOG_END;
    static const char invalid_state[] =
        CATALOG_START("16")
        APP("alpha", "[]", "waiting", "0")
        CATALOG_END;
    assert(parse_catalog(invalid_available_progress, 14, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    assert(parse_catalog(invalid_install_progress, 15, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    assert(parse_catalog(invalid_state, 16, &catalog) ==
           CP0_STORE_RESULT_ERROR);

    static const char unconfigured[] =
        "{\"protocol_version\":1,\"request_id\":17,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"unconfigured\","
        "\"message\":\"Store endpoint is not configured\"}}";
    static const char busy[] =
        "{\"protocol_version\":1,\"request_id\":18,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"busy\","
        "\"message\":\"Another operation is active\"}}";
    static const char policy_restricted[] =
        "{\"protocol_version\":1,\"request_id\":44,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"policy-restricted\","
        "\"message\":\"Install is blocked by policy\"}}";
    static const char insufficient_storage[] =
        "{\"protocol_version\":1,\"request_id\":45,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"insufficient-storage\","
        "\"message\":\"Not enough storage\"}}";
    static const char catalog_changed[] =
        "{\"protocol_version\":1,\"request_id\":46,\"outcome\":{"
        "\"status\":\"error\",\"code\":\"catalog-changed\","
        "\"message\":\"Catalog identity changed\"}}";
    assert(parse_catalog(unconfigured, 17, &catalog) ==
           CP0_STORE_RESULT_UNCONFIGURED);
    assert(parse_catalog(busy, 18, &catalog) == CP0_STORE_RESULT_BUSY);
    assert(parse_catalog(policy_restricted, 44, &catalog) ==
           CP0_STORE_RESULT_POLICY_RESTRICTED);
    assert(parse_catalog(insufficient_storage, 45, &catalog) ==
           CP0_STORE_RESULT_INSUFFICIENT_STORAGE);
    assert(parse_catalog(catalog_changed, 46, &catalog) ==
           CP0_STORE_RESULT_CATALOG_CHANGED);

    static const char refresh[] =
        ENVELOPE_START("19") "\"kind\":\"refresh-accepted\"}}}";
    static const char install[] =
        ENVELOPE_START("20")
        "\"kind\":\"install-accepted\","
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"}}}";
    assert(cp0_store_test_parse_refresh_response(refresh, strlen(refresh), 19) ==
           CP0_STORE_RESULT_OK);
    assert(cp0_store_test_parse_install_response(
               install, strlen(install), 20, "dev.cardputerzero.alpha") ==
           CP0_STORE_RESULT_OK);
    assert(cp0_store_test_parse_install_response(
               install, strlen(install), 20, "dev.cardputerzero.beta") ==
           CP0_STORE_RESULT_ERROR);

    static const char *const batch_ids[] = {
        "dev.cardputerzero.alpha",
        "dev.cardputerzero.beta",
    };
    struct cp0_store_install_preflight preflight;
    static const char install_preflight[] =
        ENVELOPE_START("42")
        "\"kind\":\"install-preflight\",\"authorization_id\":9,"
        "\"catalog_sequence\":4,\"required_bytes\":4096,"
        "\"available_bytes\":8192,\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"permissions\":[\"camera.capture\",\"network.client\"],"
        "\"policy_denied_permissions\":[\"camera.capture\"]},{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"2.0.0\","
        "\"permissions\":[],\"policy_denied_permissions\":[]}]}}}";
    assert(cp0_store_test_parse_install_preflight_response(
               install_preflight, strlen(install_preflight), 42, 4, batch_ids,
               2, &preflight) == CP0_STORE_RESULT_OK);
    assert(preflight.authorization_id == 9 && preflight.catalog_sequence == 4 &&
           preflight.required_bytes == 4096 &&
           preflight.available_bytes == 8192 && preflight.count == 2 &&
           preflight.apps[0].permissions ==
               (CP0_STORE_PERMISSION_CAMERA_CAPTURE |
                CP0_STORE_PERMISSION_NETWORK_CLIENT) &&
           preflight.apps[0].policy_denied_permissions ==
               CP0_STORE_PERMISSION_CAMERA_CAPTURE);
    assert(cp0_store_test_parse_install_preflight_response(
               install_preflight, strlen(install_preflight), 42, 5, batch_ids,
               2, &preflight) == CP0_STORE_RESULT_ERROR);
    static const char invalid_preflight_denied[] =
        ENVELOPE_START("43")
        "\"kind\":\"install-preflight\",\"authorization_id\":9,"
        "\"catalog_sequence\":4,\"required_bytes\":4096,"
        "\"available_bytes\":8192,\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"permissions\":[],"
        "\"policy_denied_permissions\":[\"camera.capture\"]},{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"2.0.0\","
        "\"permissions\":[],\"policy_denied_permissions\":[]}]}}}";
    assert(cp0_store_test_parse_install_preflight_response(
               invalid_preflight_denied, strlen(invalid_preflight_denied), 43,
               4, batch_ids, 2, &preflight) == CP0_STORE_RESULT_ERROR);
    static const char install_batch[] =
        ENVELOPE_START("35")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"},{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"2.0.0\"}]}}}";
    assert(cp0_store_test_parse_install_batch_response(
               install_batch, strlen(install_batch), 35, batch_ids, 2) ==
           CP0_STORE_RESULT_OK);
    assert(cp0_store_test_parse_install_batch_response(
               install_batch, strlen(install_batch), 35, batch_ids, 1) ==
           CP0_STORE_RESULT_ERROR);
    static const char reordered_batch[] =
        ENVELOPE_START("36")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"2.0.0\"},{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"}]}}}";
    static const char partial_batch[] =
        ENVELOPE_START("41")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"}]}}}";
    static const char duplicate_batch[] =
        ENVELOPE_START("37")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"},{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"}]}}}";
    static const char invalid_batch_version[] =
        ENVELOPE_START("38")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"},{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"02.0.0\"}]}}}";
    static const char extra_batch_item[] =
        ENVELOPE_START("39")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\"},{"
        "\"app_id\":\"dev.cardputerzero.beta\",\"version\":\"2.0.0\"},{"
        "\"app_id\":\"dev.cardputerzero.gamma\",\"version\":\"3.0.0\"}]}}}";
    static const char extra_batch_field[] =
        ENVELOPE_START("40")
        "\"kind\":\"install-batch-accepted\",\"apps\":[{"
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"extra\":true},{\"app_id\":\"dev.cardputerzero.beta\","
        "\"version\":\"2.0.0\"}]}}}";
    assert(cp0_store_test_parse_install_batch_response(
               reordered_batch, strlen(reordered_batch), 36, batch_ids, 2) ==
           CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_install_batch_response(
               partial_batch, strlen(partial_batch), 41, batch_ids, 2) ==
           CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_install_batch_response(
               duplicate_batch, strlen(duplicate_batch), 37, batch_ids, 2) ==
           CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_install_batch_response(
               invalid_batch_version, strlen(invalid_batch_version), 38,
               batch_ids, 2) == CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_install_batch_response(
               extra_batch_item, strlen(extra_batch_item), 39, batch_ids, 2) ==
           CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_install_batch_response(
               extra_batch_field, strlen(extra_batch_field), 40, batch_ids,
               2) == CP0_STORE_RESULT_ERROR);
    static const char *const unsorted_batch_ids[] = {
        "dev.cardputerzero.beta",
        "dev.cardputerzero.alpha",
    };
    assert(cp0_store_test_parse_install_batch_response(
               install_batch, strlen(install_batch), 35, unsorted_batch_ids,
               2) == CP0_STORE_RESULT_ERROR);
    static const char *const oversized_batch_ids[] = {
        "dev.cardputerzero.batch0", "dev.cardputerzero.batch1",
        "dev.cardputerzero.batch2", "dev.cardputerzero.batch3",
        "dev.cardputerzero.batch4", "dev.cardputerzero.batch5",
        "dev.cardputerzero.batch6", "dev.cardputerzero.batch7",
        "dev.cardputerzero.batch8",
    };
    assert(cp0_store_test_parse_install_batch_response(
               install_batch, strlen(install_batch), 35, oversized_batch_ids,
               9) == CP0_STORE_RESULT_ERROR);

    static const char paused[] =
        CATALOG_START("28") APP("alpha", "[]", "paused", "42") CATALOG_END;
    assert(parse_catalog(paused, 28, &catalog) == CP0_STORE_RESULT_OK);
    assert(catalog.apps[0].state == CP0_STORE_APP_PAUSED &&
           catalog.apps[0].progress_percent == 42 &&
           catalog.apps[0].failure_reason == CP0_STORE_FAILURE_NONE);
    static const char canceled[] =
        CATALOG_START("29") APP("alpha", "[]", "canceled", "0") CATALOG_END;
    assert(parse_catalog(canceled, 29, &catalog) == CP0_STORE_RESULT_OK);
    static const char failed[] =
        CATALOG_START("30") FAILED_APP("alpha", "network") CATALOG_END;
    assert(parse_catalog(failed, 30, &catalog) == CP0_STORE_RESULT_OK);
    assert(catalog.apps[0].state == CP0_STORE_APP_FAILED &&
           catalog.apps[0].failure_reason == CP0_STORE_FAILURE_NETWORK);
    static const char failed_without_reason[] =
        CATALOG_START("31") APP("alpha", "[]", "failed", "0") CATALOG_END;
    assert(parse_catalog(failed_without_reason, 31, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    static const char failed_unknown_reason[] =
        CATALOG_START("33") FAILED_APP("alpha", "timeout") CATALOG_END;
    assert(parse_catalog(failed_unknown_reason, 33, &catalog) ==
           CP0_STORE_RESULT_ERROR);
    static const char paused_with_reason[] =
        CATALOG_START("34")
        "{\"app_id\":\"dev.cardputerzero.alpha\",\"name\":\"App alpha\","
        "\"version\":\"1.2.3\",\"summary\":\"A reviewed application\","
        "\"package_bytes\":4096,\"permissions\":[],\"state\":\"paused\","
        "\"progress_percent\":42,\"failure_reason\":\"network\"}"
        CATALOG_END;
    assert(parse_catalog(paused_with_reason, 34, &catalog) ==
           CP0_STORE_RESULT_ERROR);

    static const char pause_accepted[] =
        ENVELOPE_START("32")
        "\"kind\":\"operation-accepted\","
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"action\":\"pause\"}}}";
    assert(cp0_store_test_parse_control_response(
               pause_accepted, strlen(pause_accepted), 32,
               "dev.cardputerzero.alpha", CP0_STORE_CONTROL_PAUSE) ==
           CP0_STORE_RESULT_OK);
    assert(cp0_store_test_parse_control_response(
               pause_accepted, strlen(pause_accepted), 32,
               "dev.cardputerzero.alpha", CP0_STORE_CONTROL_CANCEL) ==
           CP0_STORE_RESULT_ERROR);

    static const char prerelease_install[] =
        ENVELOPE_START("23")
        "\"kind\":\"install-accepted\","
        "\"app_id\":\"dev.cardputerzero.alpha\","
        "\"version\":\"1.2.3-beta.1+build.7\"}}}";
    assert(cp0_store_test_parse_install_response(
               prerelease_install, strlen(prerelease_install), 23,
               "dev.cardputerzero.alpha") == CP0_STORE_RESULT_OK);

    static const char invalid_install_version[] =
        ENVELOPE_START("21")
        "\"kind\":\"install-accepted\","
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"01.2.3\"}}}";
    assert(cp0_store_test_parse_install_response(
               invalid_install_version, strlen(invalid_install_version), 21,
               "dev.cardputerzero.alpha") == CP0_STORE_RESULT_ERROR);

    static const char extra_catalog_field[] =
        "{\"protocol_version\":1,\"request_id\":22,\"outcome\":{"
        "\"status\":\"ok\",\"data\":{\"kind\":\"catalog\",\"sequence\":4,"
        "\"expires_unix_seconds\":200,\"stale\":false,\"apps\":[],"
        "\"extra\":true}}}";
    assert(parse_catalog(extra_catalog_field, 22, &catalog) ==
           CP0_STORE_RESULT_ERROR);

    struct cp0_store_search_results search;
    static const char valid_search[] =
        SEARCH_START("24", "app", "0", "2", "3", "2", "true")
        APP("beta", "[]", "available", "0") ","
        APP("alpha", "[]", "installed", "100") SEARCH_END;
    assert(cp0_store_test_parse_search_response(
               valid_search, strlen(valid_search), 24, "app", 0, 2,
               &search) == CP0_STORE_RESULT_OK);
    assert(search.count == 2 && search.total == 3 && search.has_next &&
           search.next_offset == 2 && search.stale);
    assert(strcmp(search.apps[0].app_id, "dev.cardputerzero.beta") == 0);
    assert(cp0_store_test_parse_search_response(
               valid_search, strlen(valid_search), 24, "notes", 0, 2,
               &search) == CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_search_response(
               valid_search, strlen(valid_search), 24, "app", 1, 2,
               &search) == CP0_STORE_RESULT_ERROR);

    static const char empty_search[] =
        SEARCH_START("25", "missing", "0", "8", "0", "null", "false")
        SEARCH_END;
    assert(cp0_store_test_parse_search_response(
               empty_search, strlen(empty_search), 25, "missing", 0, 8,
               &search) == CP0_STORE_RESULT_OK);
    assert(search.count == 0 && search.total == 0 && !search.has_next);

    static const char wrong_next[] =
        SEARCH_START("26", "app", "0", "2", "3", "null", "false")
        APP("alpha", "[]", "available", "0") ","
        APP("beta", "[]", "available", "0") SEARCH_END;
    static const char duplicate_search[] =
        SEARCH_START("27", "app", "0", "2", "2", "null", "false")
        APP("alpha", "[]", "available", "0") ","
        APP("alpha", "[]", "available", "0") SEARCH_END;
    assert(cp0_store_test_parse_search_response(
               wrong_next, strlen(wrong_next), 26, "app", 0, 2,
               &search) == CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_search_response(
               duplicate_search, strlen(duplicate_search), 27, "app", 0, 2,
               &search) == CP0_STORE_RESULT_ERROR);
    assert(cp0_store_test_parse_search_response(
               empty_search, strlen(empty_search), 25, "bad\"query", 0, 8,
               &search) == CP0_STORE_RESULT_ERROR);

    static const char maximum_search_offset[] =
        SEARCH_START("240", "app", "1024", "8", "1024", "null", "false")
        SEARCH_END;
    assert(cp0_store_test_parse_search_response(
               maximum_search_offset, strlen(maximum_search_offset), 240,
               "app", 1024, 8, &search) == CP0_STORE_RESULT_OK);
    assert(search.count == 0 && search.total == 1024 && !search.has_next);
    assert(cp0_store_test_parse_search_response(
               maximum_search_offset, strlen(maximum_search_offset), 240,
               "app", 1025, 8, &search) == CP0_STORE_RESULT_ERROR);

    struct cp0_store_browse_results browse;
    static const char valid_browse[] =
        BROWSE_START("241", "8", "2", "11", "10", "false")
        APP("alpha", "[]", "available", "0") ","
        APP("beta", "[]", "installed", "100") BROWSE_END;
    assert(cp0_store_test_parse_browse_response(
               valid_browse, strlen(valid_browse), 241, 8, 2, &browse) ==
           CP0_STORE_RESULT_OK);
    assert(browse.offset == 8 && browse.count == 2 && browse.total == 11 &&
           browse.has_next && browse.next_offset == 10 && !browse.stale);
    static const char unsorted_browse[] =
        BROWSE_START("242", "0", "2", "2", "null", "false")
        APP("beta", "[]", "available", "0") ","
        APP("alpha", "[]", "available", "0") BROWSE_END;
    assert(cp0_store_test_parse_browse_response(
               unsorted_browse, strlen(unsorted_browse), 242, 0, 2,
               &browse) == CP0_STORE_RESULT_ERROR);
    static const char categorized_browse[] =
        ENVELOPE_START("243")
        "\"kind\":\"browse-results\",\"category\":\"utilities\","
        "\"offset\":0,\"limit\":1,\"total\":1,\"next_offset\":null,"
        "\"sequence\":4,\"expires_unix_seconds\":200,\"stale\":false,"
        "\"apps\":[" APP("alpha", "[]", "available", "0") "]}}}";
    assert(cp0_store_test_parse_browse_response(
               categorized_browse, strlen(categorized_browse), 243, 0, 1,
               &browse) == CP0_STORE_RESULT_ERROR);

    struct cp0_store_app_details details;
    static const char valid_details[] =
        ENVELOPE_START("28")
        "\"kind\":\"app-details\","
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"developer\":\"CardputerZero Labs\",\"category\":\"utilities\","
        "\"age_rating\":\"4+\","
        "\"privacy_url\":\"https://example.com/privacy\","
        "\"support_url\":\"https://example.com/support\","
        "\"description\":\"First line.\\nSecond line.\","
        "\"release_notes\":\"Adds verified media.\","
        "\"screenshot_count\":2}}}";
    assert(cp0_store_test_parse_details_response(
               valid_details, strlen(valid_details), 28,
               "dev.cardputerzero.alpha", "1.2.3", &details) ==
           CP0_STORE_RESULT_OK);
    assert(details.category == CP0_STORE_CATEGORY_UTILITIES &&
           details.age_rating == CP0_STORE_AGE_4_PLUS &&
           details.screenshot_count == 2 &&
           strcmp(details.description, "First line.\nSecond line.") == 0);
    assert(cp0_store_test_parse_details_response(
               valid_details, strlen(valid_details), 28,
               "dev.cardputerzero.alpha", "1.2.4", &details) ==
           CP0_STORE_RESULT_ERROR);

    static const char extra_details[] =
        ENVELOPE_START("29")
        "\"kind\":\"app-details\","
        "\"app_id\":\"dev.cardputerzero.alpha\",\"version\":\"1.2.3\","
        "\"developer\":\"CardputerZero Labs\",\"category\":\"utilities\","
        "\"age_rating\":\"4+\","
        "\"privacy_url\":\"https://example.com/privacy\","
        "\"support_url\":\"https://example.com/support\","
        "\"description\":\"Description\",\"release_notes\":\"Notes\","
        "\"screenshot_count\":1,\"extra\":true}}}";
    assert(cp0_store_test_parse_details_response(
               extra_details, strlen(extra_details), 29,
               "dev.cardputerzero.alpha", "1.2.3", &details) ==
           CP0_STORE_RESULT_ERROR);

    struct cp0_store_image_metadata media;
    static const char valid_icon[] =
        ENVELOPE_START("30")
        "\"kind\":\"media\",\"app_id\":\"dev.cardputerzero.alpha\","
        "\"version\":\"1.2.3\",\"media\":{\"kind\":\"icon\","
        "\"sha256\":\"2222222222222222222222222222222222222222222222222222222222222222\","
        "\"bytes\":2048,\"width\":48,\"height\":48}}}}";
    assert(cp0_store_test_parse_media_response(
               valid_icon, strlen(valid_icon), 30,
               "dev.cardputerzero.alpha", "1.2.3", false, 0, &media) ==
           CP0_STORE_RESULT_OK);
    assert(media.width == 48 && media.height == 48 &&
           media.encoded_bytes == 2048);

    static const char valid_screenshot[] =
        ENVELOPE_START("31")
        "\"kind\":\"media\",\"app_id\":\"dev.cardputerzero.alpha\","
        "\"version\":\"1.2.3\",\"media\":{\"kind\":\"screenshot\","
        "\"index\":1,"
        "\"sha256\":\"4444444444444444444444444444444444444444444444444444444444444444\","
        "\"bytes\":8192,\"width\":320,\"height\":170}}}}";
    assert(cp0_store_test_parse_media_response(
               valid_screenshot, strlen(valid_screenshot), 31,
               "dev.cardputerzero.alpha", "1.2.3", true, 1, &media) ==
           CP0_STORE_RESULT_OK);
    assert(cp0_store_test_parse_media_response(
               valid_screenshot, strlen(valid_screenshot), 31,
               "dev.cardputerzero.alpha", "1.2.3", true, 0, &media) ==
           CP0_STORE_RESULT_ERROR);

    int sockets[2];
    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) == 0);
    int source = open(__FILE__, O_RDONLY);
    assert(source >= 0);
    send_descriptors(sockets[0], &source, 1);
    char received_byte;
    int received_descriptor = -1;
    assert(cp0_store_test_receive_chunk(sockets[1], &received_byte, 1,
                                        &received_descriptor) == 1);
    assert(received_byte == 'x' && received_descriptor >= 0);
    assert((fcntl(received_descriptor, F_GETFD) & FD_CLOEXEC) != 0);
    close(received_descriptor);
    close(sockets[0]);
    close(sockets[1]);

    assert(socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) == 0);
    int duplicate_sources[] = {source, source};
    send_descriptors(sockets[0], duplicate_sources, 2);
    received_descriptor = -1;
    assert(cp0_store_test_receive_chunk(sockets[1], &received_byte, 1,
                                        &received_descriptor) == -1);
    assert(received_descriptor == -1);
    close(source);
    close(sockets[0]);
    close(sockets[1]);

    char png_path[] = "target/test-tmp/store-client-png.XXXXXX";
    int temporary = mkstemp(png_path);
    assert(temporary >= 0);
    close(temporary);
    write_red_png(png_path);
    int png_descriptor = open(png_path, O_RDONLY);
    assert(png_descriptor >= 0 &&
           fcntl(png_descriptor, F_SETFD, FD_CLOEXEC) == 0);
    struct stat png_status;
    assert(fstat(png_descriptor, &png_status) == 0);
    struct cp0_store_image_metadata png_metadata = {
        .encoded_bytes = (uint64_t)png_status.st_size,
        .width = 1,
        .height = 1,
    };
    uint32_t pixel = 0;
    assert(cp0_store_test_decode_png_descriptor(
               png_descriptor, &png_metadata, &pixel, 1) == 0);
    assert(pixel == 0xffff0000U);
    close(png_descriptor);
    assert(unlink(png_path) == 0);
    return 0;
}
