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

enum cp0_store_result {
    CP0_STORE_RESULT_OK = 0,
    CP0_STORE_RESULT_UNCONFIGURED = 1,
    CP0_STORE_RESULT_BUSY = 2,
    CP0_STORE_RESULT_ERROR = -1,
};

enum cp0_store_app_state {
    CP0_STORE_APP_AVAILABLE,
    CP0_STORE_APP_QUEUED,
    CP0_STORE_APP_DOWNLOADING,
    CP0_STORE_APP_INSTALLING,
    CP0_STORE_APP_INSTALLED,
    CP0_STORE_APP_FAILED,
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

int cp0_store_list(struct cp0_store_catalog *catalog);
int cp0_store_search(const char *query, uint16_t offset, uint8_t limit,
                     struct cp0_store_search_results *results);
int cp0_store_refresh(void);
int cp0_store_install(const char *app_id);

#endif
