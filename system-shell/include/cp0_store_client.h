#ifndef CP0_STORE_CLIENT_H
#define CP0_STORE_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_STORE_MAX_APPS 32
#define CP0_STORE_APP_ID_BYTES 129
#define CP0_STORE_APP_NAME_BYTES 129
#define CP0_STORE_VERSION_BYTES 65
#define CP0_STORE_SUMMARY_BYTES 385
#define CP0_STORE_SEARCH_QUERY_BYTES 97
#define CP0_STORE_SEARCH_MAX_APPS 8
#define CP0_STORE_INSTALL_BATCH_MAX 8
#define CP0_STORE_DEVELOPER_BYTES 321
#define CP0_STORE_URL_BYTES 2049
#define CP0_STORE_DESCRIPTION_BYTES 4097
#define CP0_STORE_RELEASE_NOTES_BYTES 2049
#define CP0_STORE_MEDIA_SHA256_BYTES 65
#define CP0_STORE_MAX_SCREENSHOTS 5
#define CP0_STORE_ICON_MAX_PIXELS (48U * 48U)
#define CP0_STORE_SCREENSHOT_PIXELS (320U * 170U)
#define CP0_STORE_EDITORIAL_COLLECTION_MAX 2
#define CP0_STORE_EDITORIAL_COLLECTION_APP_MAX 4
#define CP0_STORE_EDITORIAL_HEADLINE_BYTES 193
#define CP0_STORE_EDITORIAL_TITLE_BYTES 129

enum cp0_store_result {
    CP0_STORE_RESULT_OK = 0,
    CP0_STORE_RESULT_UNCONFIGURED = 1,
    CP0_STORE_RESULT_BUSY = 2,
    CP0_STORE_RESULT_POLICY_RESTRICTED = 3,
    CP0_STORE_RESULT_INSUFFICIENT_STORAGE = 4,
    CP0_STORE_RESULT_CATALOG_CHANGED = 5,
    CP0_STORE_RESULT_ERROR = -1,
};

enum cp0_store_app_state {
    CP0_STORE_APP_AVAILABLE,
    CP0_STORE_APP_QUEUED,
    CP0_STORE_APP_DOWNLOADING,
    CP0_STORE_APP_PAUSED,
    CP0_STORE_APP_INSTALLING,
    CP0_STORE_APP_INSTALLED,
    CP0_STORE_APP_CANCELED,
    CP0_STORE_APP_FAILED,
};

enum cp0_store_failure_reason {
    CP0_STORE_FAILURE_NONE,
    CP0_STORE_FAILURE_NETWORK,
    CP0_STORE_FAILURE_STORAGE,
    CP0_STORE_FAILURE_VERIFICATION,
    CP0_STORE_FAILURE_INSTALLER,
    CP0_STORE_FAILURE_CATALOG_CHANGED,
    CP0_STORE_FAILURE_INTERNAL,
};

enum cp0_store_control_action {
    CP0_STORE_CONTROL_PAUSE,
    CP0_STORE_CONTROL_RESUME,
    CP0_STORE_CONTROL_CANCEL,
};

enum cp0_store_permission {
    CP0_STORE_PERMISSION_AUDIO_CAPTURE = 1U << 0,
    CP0_STORE_PERMISSION_AUDIO_PLAYBACK = 1U << 1,
    CP0_STORE_PERMISSION_CAMERA_CAPTURE = 1U << 2,
    CP0_STORE_PERMISSION_DOCUMENTS_OPEN = 1U << 3,
    CP0_STORE_PERMISSION_HARDWARE_GPIO = 1U << 4,
    CP0_STORE_PERMISSION_NETWORK_CLIENT = 1U << 5,
    CP0_STORE_PERMISSION_NOTIFICATIONS_POST = 1U << 6,
    CP0_STORE_PERMISSION_RADIO_LORA = 1U << 7,
};

struct cp0_store_app_summary {
    uint64_t package_bytes;
    uint16_t permissions;
    uint8_t progress_percent;
    enum cp0_store_app_state state;
    enum cp0_store_failure_reason failure_reason;
    char app_id[CP0_STORE_APP_ID_BYTES];
    char name[CP0_STORE_APP_NAME_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    char summary[CP0_STORE_SUMMARY_BYTES];
};

struct cp0_store_catalog {
    uint64_t sequence;
    uint64_t expires_unix_seconds;
    size_t count;
    bool stale;
    bool truncated;
    struct cp0_store_app_summary apps[CP0_STORE_MAX_APPS];
};

struct cp0_store_editorial_collection {
    size_t count;
    char title[CP0_STORE_EDITORIAL_TITLE_BYTES];
    struct cp0_store_app_summary
        apps[CP0_STORE_EDITORIAL_COLLECTION_APP_MAX];
};

struct cp0_store_today {
    uint64_t sequence;
    uint64_t expires_unix_seconds;
    size_t collection_count;
    bool stale;
    bool has_editorial;
    char headline[CP0_STORE_EDITORIAL_HEADLINE_BYTES];
    struct cp0_store_app_summary featured;
    struct cp0_store_editorial_collection
        collections[CP0_STORE_EDITORIAL_COLLECTION_MAX];
};

struct cp0_store_install_preflight_app {
    uint16_t permissions;
    uint16_t policy_denied_permissions;
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
};

struct cp0_store_install_preflight {
    uint64_t authorization_id;
    uint64_t catalog_sequence;
    uint64_t required_bytes;
    uint64_t available_bytes;
    size_t count;
    struct cp0_store_install_preflight_app apps[CP0_STORE_INSTALL_BATCH_MAX];
};

struct cp0_store_search_results {
    uint64_t sequence;
    uint64_t expires_unix_seconds;
    uint16_t offset;
    uint16_t total;
    uint16_t next_offset;
    uint8_t limit;
    size_t count;
    bool has_next;
    bool stale;
    char query[CP0_STORE_SEARCH_QUERY_BYTES];
    struct cp0_store_app_summary apps[CP0_STORE_SEARCH_MAX_APPS];
};

enum cp0_store_category {
    CP0_STORE_CATEGORY_DEVELOPER_TOOLS,
    CP0_STORE_CATEGORY_EDUCATION,
    CP0_STORE_CATEGORY_ENTERTAINMENT,
    CP0_STORE_CATEGORY_GAMES,
    CP0_STORE_CATEGORY_HARDWARE,
    CP0_STORE_CATEGORY_MEDIA,
    CP0_STORE_CATEGORY_PRODUCTIVITY,
    CP0_STORE_CATEGORY_UTILITIES,
};

enum cp0_store_age_rating {
    CP0_STORE_AGE_4_PLUS,
    CP0_STORE_AGE_9_PLUS,
    CP0_STORE_AGE_12_PLUS,
    CP0_STORE_AGE_17_PLUS,
};

struct cp0_store_app_details {
    enum cp0_store_category category;
    enum cp0_store_age_rating age_rating;
    uint8_t screenshot_count;
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    char developer[CP0_STORE_DEVELOPER_BYTES];
    char privacy_url[CP0_STORE_URL_BYTES];
    char support_url[CP0_STORE_URL_BYTES];
    char description[CP0_STORE_DESCRIPTION_BYTES];
    char release_notes[CP0_STORE_RELEASE_NOTES_BYTES];
};

struct cp0_store_image_metadata {
    uint64_t encoded_bytes;
    uint16_t width;
    uint16_t height;
    char sha256[CP0_STORE_MEDIA_SHA256_BYTES];
};

struct cp0_store_auto_update_status {
    bool enabled;
    bool policy_allowed;
    bool charging;
    bool unmetered_network;
    bool due;
    bool checking;
};

struct cp0_store_metrics_status {
    bool enabled;
    bool policy_allowed;
    bool configured;
    bool pending;
};

int cp0_store_list(struct cp0_store_catalog *catalog);
int cp0_store_today(struct cp0_store_today *today);
int cp0_store_search(const char *query, uint16_t offset, uint8_t limit,
                     struct cp0_store_search_results *results);
int cp0_store_refresh(void);
int cp0_store_get_auto_update(
    struct cp0_store_auto_update_status *status);
int cp0_store_set_auto_update(
    bool enabled, struct cp0_store_auto_update_status *status);
int cp0_store_get_metrics(struct cp0_store_metrics_status *status);
int cp0_store_set_metrics(bool enabled,
                          struct cp0_store_metrics_status *status);
int cp0_store_preflight_install(
    uint64_t catalog_sequence, const char *const app_ids[], size_t app_count,
    struct cp0_store_install_preflight *preflight);
int cp0_store_install(const char *app_id, uint64_t authorization_id);
int cp0_store_install_batch(const char *const app_ids[], size_t app_count,
                            uint64_t authorization_id);
int cp0_store_control(const char *app_id,
                      enum cp0_store_control_action action);
int cp0_store_get_details(const char *app_id, const char *expected_version,
                          struct cp0_store_app_details *details);
int cp0_store_get_icon(const char *app_id, const char *expected_version,
                       uint32_t *pixels, size_t pixel_capacity,
                       struct cp0_store_image_metadata *metadata);
int cp0_store_get_screenshot(
    const char *app_id, const char *expected_version, uint8_t index,
    uint32_t *pixels, size_t pixel_capacity,
    struct cp0_store_image_metadata *metadata);

#endif
