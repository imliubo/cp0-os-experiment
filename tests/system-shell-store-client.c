#include "cp0_store_client.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

int cp0_store_test_parse_catalog_response(
    const char *response, size_t response_length, uint64_t request_id,
    struct cp0_store_catalog *catalog);
int cp0_store_test_parse_refresh_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id);
int cp0_store_test_parse_install_response(const char *response,
                                          size_t response_length,
                                          uint64_t request_id,
                                          const char *app_id);

#define ENVELOPE_START(ID)                                                     \
    "{\"protocol_version\":1,\"request_id\":" ID ",\"outcome\":{"      \
    "\"status\":\"ok\",\"data\":{"
#define APP(ID, PERMISSIONS, STATE, PROGRESS)                                  \
    "{\"app_id\":\"dev.cardputerzero." ID "\",\"name\":\"App " ID       \
    "\",\"version\":\"1.2.3\",\"summary\":\"A reviewed application\"," \
    "\"package_bytes\":4096,\"permissions\":" PERMISSIONS ",\"state\":\"" \
    STATE "\",\"progress_percent\":" PROGRESS "}"
#define CATALOG_START(ID)                                                      \
    ENVELOPE_START(ID)                                                        \
    "\"kind\":\"catalog\",\"sequence\":4,\"expires_unix_seconds\":200," \
    "\"stale\":false,\"apps\":["
#define CATALOG_END "]}}}"

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
        APP("alpha", "[]", "paused", "0")
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
    assert(parse_catalog(unconfigured, 17, &catalog) ==
           CP0_STORE_RESULT_UNCONFIGURED);
    assert(parse_catalog(busy, 18, &catalog) == CP0_STORE_RESULT_BUSY);

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
    return 0;
}
