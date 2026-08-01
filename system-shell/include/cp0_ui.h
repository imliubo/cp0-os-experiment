#ifndef CP0_UI_H
#define CP0_UI_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_UI_WIDTH 320
#define CP0_UI_HEIGHT 170
#define CP0_UI_MAX_APPS 32
#define CP0_UI_APP_ID_MAX 128
#define CP0_UI_APP_NAME_MAX 128
#define CP0_UI_APP_VERSION_MAX 64
#define CP0_UI_PROMPT_APP_NAME_MAX 32
#define CP0_UI_PROMPT_PERMISSION_MAX 31
#define CP0_UI_PROMPT_REASON_MAX 160
#define CP0_UI_NOTIFICATION_APP_NAME_MAX 128
#define CP0_UI_NOTIFICATION_TITLE_MAX 128
#define CP0_UI_NOTIFICATION_BODY_MAX 640
#define CP0_UI_NOTIFICATION_BOTTOM 88
#define CP0_UI_MAX_DOCUMENTS 16
#define CP0_UI_DOCUMENT_ID_MAX 32
#define CP0_UI_DOCUMENT_NAME_MAX 128
#define CP0_UI_STORE_VERSION_MAX 64
#define CP0_UI_STORE_SUMMARY_MAX 384
#define CP0_UI_STORE_SEARCH_QUERY_MAX 96
#define CP0_UI_STORE_SEARCH_PAGE_MAX 8
#define CP0_UI_STORE_CATALOG_MAX 1024
#define CP0_UI_STORE_UPDATE_BATCH_MAX 8
#define CP0_UI_STORE_RECENT_MAX 4
#define CP0_UI_STORE_DEVELOPER_MAX 320
#define CP0_UI_STORE_DESCRIPTION_MAX 4096
#define CP0_UI_STORE_RELEASE_NOTES_MAX 2048
#define CP0_UI_STORE_CATEGORY_MAX 20
#define CP0_UI_STORE_AGE_RATING_MAX 3
#define CP0_UI_STORE_ICON_MAX_PIXELS (48U * 48U)
#define CP0_UI_STORE_SCREENSHOT_PIXELS (320U * 170U)
#define CP0_UI_STORE_EDITORIAL_COLLECTION_MAX 2
#define CP0_UI_STORE_EDITORIAL_COLLECTION_APP_MAX 4
#define CP0_UI_STORE_EDITORIAL_HEADLINE_MAX 192
#define CP0_UI_STORE_EDITORIAL_TITLE_MAX 128

enum cp0_ui_screen {
    CP0_UI_HOME,
    CP0_UI_APPS,
    CP0_UI_STORE,
    CP0_UI_DEVICE,
    CP0_UI_NETWORK,
    CP0_UI_SETTINGS,
    CP0_UI_TASKS,
};

enum cp0_ui_action {
    CP0_UI_UP,
    CP0_UI_DOWN,
    CP0_UI_LEFT,
    CP0_UI_RIGHT,
    CP0_UI_ACCEPT,
    CP0_UI_BACK,
    CP0_UI_GO_HOME,
    CP0_UI_SHOW_TASKS,
    CP0_UI_SHOW_POWER,
    CP0_UI_BRIGHTNESS_DOWN,
    CP0_UI_BRIGHTNESS_UP,
    CP0_UI_MUTE,
    CP0_UI_VOLUME_DOWN,
    CP0_UI_VOLUME_UP,
    CP0_UI_MEDIA_PLAY_PAUSE,
    CP0_UI_MEDIA_PREVIOUS,
    CP0_UI_MEDIA_NEXT,
    CP0_UI_HELP,
    CP0_UI_SCREENSHOT,
};

enum cp0_ui_event {
    CP0_UI_EVENT_NONE,
    CP0_UI_EVENT_SLEEP,
    CP0_UI_EVENT_RESTART,
    CP0_UI_EVENT_POWER_OFF,
    CP0_UI_EVENT_OPEN_APP,
    CP0_UI_EVENT_STOP_APP,
    CP0_UI_EVENT_PERMISSION_ONCE,
    CP0_UI_EVENT_PERMISSION_ALWAYS,
    CP0_UI_EVENT_PERMISSION_DENY,
    CP0_UI_EVENT_DOCUMENT_SELECT,
    CP0_UI_EVENT_DOCUMENT_CANCEL,
    CP0_UI_EVENT_STORE_REFRESH,
    CP0_UI_EVENT_STORE_INSTALL,
    CP0_UI_EVENT_STORE_UPDATE_ALL,
    CP0_UI_EVENT_STORE_PAUSE,
    CP0_UI_EVENT_STORE_RESUME,
    CP0_UI_EVENT_STORE_CANCEL,
    CP0_UI_EVENT_STORE_BROWSE,
    CP0_UI_EVENT_STORE_SEARCH,
    CP0_UI_EVENT_STORE_DETAILS,
    CP0_UI_EVENT_STORE_SCREENSHOT,
    CP0_UI_EVENT_STORE_INSTALL_CONFIRM,
    CP0_UI_EVENT_DEVELOPER_ENABLE,
    CP0_UI_EVENT_DEVELOPER_DISABLE,
    CP0_UI_EVENT_RECOVERY_ENABLE,
    CP0_UI_EVENT_RECOVERY_DISABLE,
    CP0_UI_EVENT_AUTO_UPDATE_ENABLE,
    CP0_UI_EVENT_AUTO_UPDATE_DISABLE,
    CP0_UI_EVENT_METRICS_ENABLE,
    CP0_UI_EVENT_METRICS_DISABLE,
    CP0_UI_EVENT_UNINSTALL_APP,
    CP0_UI_EVENT_MEDIA_PLAY_PAUSE,
    CP0_UI_EVENT_MEDIA_PREVIOUS,
    CP0_UI_EVENT_MEDIA_NEXT,
    CP0_UI_EVENT_SCREENSHOT,
};

enum cp0_ui_screenshot_status {
    CP0_UI_SCREENSHOT_REQUESTED,
    CP0_UI_SCREENSHOT_SAVED,
    CP0_UI_SCREENSHOT_FAILED,
    CP0_UI_SCREENSHOT_UNAVAILABLE,
    CP0_UI_SCREENSHOT_BUSY,
};

enum cp0_ui_authority {
    CP0_UI_AUTHORITY_PERSONAL,
    CP0_UI_AUTHORITY_PARENT,
    CP0_UI_AUTHORITY_ORGANIZATION,
};

enum cp0_ui_app_state {
    CP0_UI_APP_STOPPED,
    CP0_UI_APP_STARTING,
    CP0_UI_APP_RUNNING,
    CP0_UI_APP_FAILED,
};

enum cp0_ui_store_state {
    CP0_UI_STORE_AVAILABLE,
    CP0_UI_STORE_UPDATE,
    CP0_UI_STORE_QUEUED,
    CP0_UI_STORE_DOWNLOADING,
    CP0_UI_STORE_PAUSED,
    CP0_UI_STORE_INSTALLING,
    CP0_UI_STORE_INSTALLED,
    CP0_UI_STORE_CANCELED,
    CP0_UI_STORE_FAILED,
};

enum cp0_ui_store_failure_reason {
    CP0_UI_STORE_FAILURE_NONE,
    CP0_UI_STORE_FAILURE_NETWORK,
    CP0_UI_STORE_FAILURE_STORAGE,
    CP0_UI_STORE_FAILURE_VERIFICATION,
    CP0_UI_STORE_FAILURE_INSTALLER,
    CP0_UI_STORE_FAILURE_CATALOG_CHANGED,
    CP0_UI_STORE_FAILURE_INTERNAL,
};

enum cp0_ui_store_status {
    CP0_UI_STORE_LOADING,
    CP0_UI_STORE_READY,
    CP0_UI_STORE_UNCONFIGURED,
    CP0_UI_STORE_UNAVAILABLE,
};

enum cp0_ui_store_section {
    CP0_UI_STORE_TODAY,
    CP0_UI_STORE_APPS,
    CP0_UI_STORE_SEARCH,
    CP0_UI_STORE_UPDATES,
};

enum cp0_ui_store_detail_status {
    CP0_UI_STORE_DETAIL_LOADING,
    CP0_UI_STORE_DETAIL_READY,
    CP0_UI_STORE_DETAIL_UNAVAILABLE,
};

enum cp0_ui_store_preflight_error {
    CP0_UI_STORE_PREFLIGHT_NONE,
    CP0_UI_STORE_PREFLIGHT_POLICY,
    CP0_UI_STORE_PREFLIGHT_STORAGE,
    CP0_UI_STORE_PREFLIGHT_CATALOG,
    CP0_UI_STORE_PREFLIGHT_UNAVAILABLE,
};

struct cp0_ui_catalog_app {
    bool running;
    bool immersive;
    const char *app_id;
    const char *name;
    const char *version;
    uint16_t permissions;
    uint64_t installed_at_unix_seconds;
    uint64_t package_bytes;
    uint64_t data_bytes;
};

struct cp0_ui_app {
    uint32_t token;
    bool installed;
    bool immersive;
    enum cp0_ui_app_state state;
    uint16_t permissions;
    uint64_t installed_at_unix_seconds;
    uint64_t package_bytes;
    uint64_t data_bytes;
    char app_id[CP0_UI_APP_ID_MAX + 1];
    char name[CP0_UI_APP_NAME_MAX + 1];
    char version[CP0_UI_APP_VERSION_MAX + 1];
};

struct cp0_ui_device_info {
    bool available;
    int battery_percent;
    int temperature_millicelsius;
    bool battery_present;
    bool battery_voltage_available;
    bool battery_current_available;
    int64_t battery_voltage_microvolts;
    int64_t battery_current_microamps;
    unsigned int battery_status;
    unsigned int i2c_bus_state;
    unsigned int display_state;
    unsigned int keyboard_state;
    unsigned int audio_state;
    unsigned int camera_state;
    uint64_t uptime_seconds;
    uint64_t memory_total_bytes;
    uint64_t memory_available_bytes;
    uint64_t storage_total_bytes;
    uint64_t storage_available_bytes;
    const char *model;
    const char *os_version;
};

struct cp0_ui_network_info {
    bool available;
    bool online;
    bool link_up;
    const char *interface_name;
    const char *ipv4_address;
};

struct cp0_ui_store_catalog_app {
    uint64_t package_bytes;
    uint16_t permissions;
    uint8_t progress_percent;
    enum cp0_ui_store_state state;
    enum cp0_ui_store_failure_reason failure_reason;
    const char *app_id;
    const char *name;
    const char *version;
    const char *summary;
    const char *installed_version;
    uint16_t installed_permissions;
};

struct cp0_ui_store_app {
    uint64_t package_bytes;
    uint16_t permissions;
    uint16_t installed_permissions;
    uint8_t progress_percent;
    enum cp0_ui_store_state state;
    enum cp0_ui_store_state operation_state;
    enum cp0_ui_store_failure_reason failure_reason;
    bool update_available;
    char app_id[CP0_UI_APP_ID_MAX + 1];
    char name[CP0_UI_APP_NAME_MAX + 1];
    char version[CP0_UI_STORE_VERSION_MAX + 1];
    char summary[CP0_UI_STORE_SUMMARY_MAX + 1];
};

struct cp0_ui_store_editorial_collection {
    const char *title;
    const struct cp0_ui_store_catalog_app *apps;
    size_t app_count;
};

struct cp0_ui_store_editorial {
    const char *headline;
    const struct cp0_ui_store_catalog_app *featured;
    const struct cp0_ui_store_editorial_collection *collections;
    size_t collection_count;
};

struct cp0_ui_store_editorial_collection_state {
    size_t app_count;
    char title[CP0_UI_STORE_EDITORIAL_TITLE_MAX + 1];
    struct cp0_ui_store_app
        apps[CP0_UI_STORE_EDITORIAL_COLLECTION_APP_MAX];
};

struct cp0_ui_store_completion {
    uint8_t count;
    char app_name[CP0_UI_APP_NAME_MAX + 1];
    char version[CP0_UI_STORE_VERSION_MAX + 1];
};

struct cp0_ui_document_option {
    uint64_t size_bytes;
    const char *document_id;
    const char *name;
};

struct cp0_ui_document {
    uint64_t size_bytes;
    char document_id[CP0_UI_DOCUMENT_ID_MAX + 1];
    char name[CP0_UI_DOCUMENT_NAME_MAX + 1];
};

struct cp0_ui {
    enum cp0_ui_screen screen;
    unsigned int selected;
    unsigned int app_selected;
    unsigned int app_count;
    unsigned int store_selected;
    unsigned int store_count;
    unsigned int store_section;
    unsigned int store_browse_selected;
    unsigned int store_browse_count;
    unsigned int store_search_selected;
    unsigned int store_search_count;
    unsigned int store_recent_selected;
    unsigned int store_recent_count;
    unsigned int store_today_selected;
    unsigned int store_today_collection_selected;
    unsigned int store_today_open_collection;
    unsigned int store_detail_page;
    unsigned int store_operation_action_selected;
    unsigned int store_screenshot_index;
    unsigned int store_detail_text_offset;
    uint8_t store_activity_count;
    uint8_t store_activity_progress_percent;
    uint16_t store_browse_offset;
    uint16_t store_browse_total;
    uint16_t store_browse_next_offset;
    uint16_t store_search_offset;
    uint16_t store_search_total;
    uint16_t store_search_next_offset;
    bool store_search_has_next;
    bool store_browse_has_next;
    bool store_search_input;
    unsigned int task_action_selected;
    unsigned int settings_selected;
    unsigned int settings_item_selected;
    unsigned int app_detail_page;
    unsigned int app_permission_offset;
    unsigned int app_action_selected;
    unsigned int device_page;
    unsigned int network_page;
    bool app_list_truncated;
    bool store_list_truncated;
    bool store_catalog_stale;
    bool store_browse_stale;
    bool store_search_stale;
    bool store_activity;
    bool store_catalog_observed;
    bool store_update_all_selected;
    bool store_detail;
    bool store_today_available;
    bool store_today_collection_open;
    bool app_detail;
    bool app_uninstall_confirm;
    bool settings_detail;
    enum cp0_ui_store_status store_status;
    enum cp0_ui_store_status store_browse_status;
    enum cp0_ui_store_status store_search_status;
    enum cp0_ui_store_detail_status store_detail_status;
    unsigned int dialog_selected;
    bool power_dialog;
    bool settings_available;
    bool settings_confirm;
    bool settings_confirm_recovery;
    bool settings_confirm_metrics;
    bool store_install_prompt;
    enum cp0_ui_store_preflight_error store_preflight_error;
    uint8_t store_preflight_app_count;
    uint8_t store_preflight_new_permissions;
    uint8_t store_preflight_denied_permissions;
    uint64_t store_preflight_required_bytes;
    uint64_t store_preflight_available_bytes;
    bool wifi_enabled;
    bool airplane_mode;
    bool muted;
    bool key_sounds;
    bool camera_mirror;
    bool local_simulation;
    unsigned int brightness_percent;
    unsigned int volume_percent;
    unsigned int theme;
    unsigned int screen_timeout;
    unsigned int camera_resolution;
    unsigned int camera_rotation;
    bool system_action_overlay;
    bool help_overlay;
    unsigned int system_action_kind;
    unsigned int system_action_ticks;
    enum cp0_ui_screenshot_status screenshot_status;
    bool developer_mode;
    bool developer_mode_allowed;
    bool recovery_mode;
    bool recovery_mode_allowed;
    bool store_install_allowed;
    bool auto_update_available;
    bool auto_update_enabled;
    bool auto_update_policy_allowed;
    bool auto_update_charging;
    bool auto_update_unmetered_network;
    bool auto_update_due;
    bool auto_update_checking;
    bool metrics_available;
    bool metrics_enabled;
    bool metrics_policy_allowed;
    bool metrics_configured;
    bool metrics_pending;
    bool app_launch_restricted;
    enum cp0_ui_authority settings_authority;
    uint8_t denied_permission_count;
    bool permission_prompt;
    bool document_prompt;
    bool notification_banner;
    bool network_online;
    int battery_percent;
    bool device_available;
    int temperature_millicelsius;
    bool battery_present;
    bool battery_voltage_available;
    bool battery_current_available;
    int64_t battery_voltage_microvolts;
    int64_t battery_current_microamps;
    unsigned int battery_status;
    unsigned int i2c_bus_state;
    unsigned int display_state;
    unsigned int keyboard_state;
    unsigned int audio_state;
    unsigned int camera_state;
    uint64_t uptime_seconds;
    uint64_t memory_total_bytes;
    uint64_t memory_available_bytes;
    uint64_t storage_total_bytes;
    uint64_t storage_available_bytes;
    char device_model[33];
    char os_version[33];
    bool network_available;
    bool network_link_up;
    char network_interface[17];
    char network_ipv4[16];
    char clock_text[6];
    uint64_t prompt_id;
    unsigned int prompt_selected;
    char prompt_app_name[CP0_UI_PROMPT_APP_NAME_MAX + 1];
    char prompt_permission[CP0_UI_PROMPT_PERMISSION_MAX + 1];
    char prompt_reason[CP0_UI_PROMPT_REASON_MAX + 1];
    uint64_t document_prompt_id;
    unsigned int document_selected;
    unsigned int document_count;
    char document_app_name[CP0_UI_PROMPT_APP_NAME_MAX + 1];
    struct cp0_ui_document documents[CP0_UI_MAX_DOCUMENTS];
    uint64_t notification_id;
    char notification_app_name[CP0_UI_NOTIFICATION_APP_NAME_MAX + 1];
    char notification_title[CP0_UI_NOTIFICATION_TITLE_MAX + 1];
    char notification_body[CP0_UI_NOTIFICATION_BODY_MAX + 1];
    uint8_t store_completion_count;
    char store_completion_app_name[CP0_UI_APP_NAME_MAX + 1];
    char store_completion_version[CP0_UI_STORE_VERSION_MAX + 1];
    enum cp0_ui_store_state store_activity_state;
    char store_search_query[CP0_UI_STORE_SEARCH_QUERY_MAX + 1];
    char store_recent_queries[CP0_UI_STORE_RECENT_MAX]
                             [CP0_UI_STORE_SEARCH_QUERY_MAX + 1];
    uint8_t store_screenshot_count;
    uint16_t store_icon_width;
    uint16_t store_icon_height;
    bool store_icon_available;
    bool store_screenshot_available;
    bool store_screenshot_loading;
    char store_detail_app_id[CP0_UI_APP_ID_MAX + 1];
    char store_detail_version[CP0_UI_STORE_VERSION_MAX + 1];
    char store_developer[CP0_UI_STORE_DEVELOPER_MAX + 1];
    char store_category[CP0_UI_STORE_CATEGORY_MAX + 1];
    char store_age_rating[CP0_UI_STORE_AGE_RATING_MAX + 1];
    char store_description[CP0_UI_STORE_DESCRIPTION_MAX + 1];
    char store_release_notes[CP0_UI_STORE_RELEASE_NOTES_MAX + 1];
    char store_today_headline[CP0_UI_STORE_EDITORIAL_HEADLINE_MAX + 1];
    const uint32_t *store_icon_pixels;
    const uint32_t *store_screenshot_pixels;
    struct cp0_ui_app apps[CP0_UI_MAX_APPS];
    struct cp0_ui_store_app store_apps[CP0_UI_MAX_APPS];
    struct cp0_ui_store_app store_page_apps[CP0_UI_STORE_SEARCH_PAGE_MAX];
    struct cp0_ui_store_app store_today_featured;
    struct cp0_ui_store_editorial_collection_state
        store_today_collections[CP0_UI_STORE_EDITORIAL_COLLECTION_MAX];
    size_t store_today_collection_count;
};

void cp0_ui_init(struct cp0_ui *ui);
void cp0_ui_set_status(struct cp0_ui *ui, const char *clock_text,
                       bool network_online, int battery_percent);
void cp0_ui_set_device_info(struct cp0_ui *ui,
                            const struct cp0_ui_device_info *info);
void cp0_ui_set_network_info(struct cp0_ui *ui,
                             const struct cp0_ui_network_info *info);
void cp0_ui_set_device_settings(
    struct cp0_ui *ui, enum cp0_ui_authority authority,
    bool developer_mode, bool developer_mode_allowed, bool recovery_mode,
    bool recovery_mode_allowed, bool store_install_allowed,
    bool app_launch_restricted, uint8_t denied_permission_count);
void cp0_ui_set_auto_update(
    struct cp0_ui *ui, bool available, bool enabled, bool policy_allowed,
    bool charging, bool unmetered_network, bool due, bool checking);
void cp0_ui_set_metrics(struct cp0_ui *ui, bool available, bool enabled,
                        bool policy_allowed, bool configured, bool pending);
void cp0_ui_add_app(struct cp0_ui *ui, uint32_t token, const char *app_id);
void cp0_ui_sync_app_catalog(struct cp0_ui *ui,
                             const struct cp0_ui_catalog_app *apps,
                             size_t app_count, bool truncated);
void cp0_ui_set_app_display_mode(struct cp0_ui *ui, uint32_t token,
                                 bool immersive);
void cp0_ui_remove_app(struct cp0_ui *ui, uint32_t token);
void cp0_ui_set_app_state(struct cp0_ui *ui, const char *app_id,
                          enum cp0_ui_app_state state);
void cp0_ui_set_store_status(struct cp0_ui *ui,
                             enum cp0_ui_store_status status);
void cp0_ui_sync_store_catalog(
    struct cp0_ui *ui, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count, bool truncated, bool stale);
void cp0_ui_sync_store_today(
    struct cp0_ui *ui, const struct cp0_ui_store_editorial *editorial);
void cp0_ui_sync_store_browse(
    struct cp0_ui *ui, uint16_t offset, uint16_t total, bool has_next,
    uint16_t next_offset, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count, bool stale);
void cp0_ui_set_store_browse_status(struct cp0_ui *ui,
                                    enum cp0_ui_store_status status);
void cp0_ui_sync_store_search(
    struct cp0_ui *ui, const char *query, uint16_t offset, uint16_t total,
    bool has_next, uint16_t next_offset,
    const struct cp0_ui_store_catalog_app *apps, size_t app_count, bool stale);
void cp0_ui_set_store_search_status(struct cp0_ui *ui,
                                    enum cp0_ui_store_status status);
bool cp0_ui_store_accepts_text(const struct cp0_ui *ui);
enum cp0_ui_event cp0_ui_store_input_ascii(struct cp0_ui *ui, char character);
enum cp0_ui_event cp0_ui_store_backspace(struct cp0_ui *ui);
const char *cp0_ui_store_search_query(const struct cp0_ui *ui);
uint16_t cp0_ui_store_search_offset(const struct cp0_ui *ui);
uint16_t cp0_ui_store_browse_offset(const struct cp0_ui *ui);
void cp0_ui_set_store_app_state(struct cp0_ui *ui, const char *app_id,
                                enum cp0_ui_store_state state,
                                uint8_t progress_percent);
size_t cp0_ui_collect_store_update_batch(const struct cp0_ui *ui,
                                         const char *app_ids[],
                                         size_t app_capacity);
bool cp0_ui_take_store_completion(
    struct cp0_ui *ui, struct cp0_ui_store_completion *completion);
void cp0_ui_show_store_install_prompt(
    struct cp0_ui *ui, uint8_t app_count, uint8_t new_permissions,
    uint8_t denied_permissions, uint64_t required_bytes,
    uint64_t available_bytes);
void cp0_ui_show_store_preflight_error(
    struct cp0_ui *ui, enum cp0_ui_store_preflight_error error);
const char *cp0_ui_selected_store_app_id(const struct cp0_ui *ui);
enum cp0_ui_store_state cp0_ui_selected_store_app_state(
    const struct cp0_ui *ui);
const char *cp0_ui_selected_store_app_version(const struct cp0_ui *ui);
uint8_t cp0_ui_selected_store_screenshot(const struct cp0_ui *ui);
void cp0_ui_set_store_details(
    struct cp0_ui *ui, const char *app_id, const char *version,
    const char *developer, const char *category, const char *age_rating,
    const char *description, const char *release_notes,
    uint8_t screenshot_count);
void cp0_ui_set_store_details_unavailable(struct cp0_ui *ui,
                                          const char *app_id,
                                          const char *version);
void cp0_ui_set_store_icon(struct cp0_ui *ui, const char *app_id,
                           const char *version, const uint32_t *pixels,
                           uint16_t width, uint16_t height);
void cp0_ui_set_store_screenshot(struct cp0_ui *ui, const char *app_id,
                                 const char *version, uint8_t index,
                                 const uint32_t *pixels, uint16_t width,
                                 uint16_t height);
void cp0_ui_set_store_screenshot_unavailable(struct cp0_ui *ui,
                                             const char *app_id,
                                             const char *version,
                                             uint8_t index);
const char *cp0_ui_selected_app_id(const struct cp0_ui *ui);
enum cp0_ui_app_state cp0_ui_selected_app_state(const struct cp0_ui *ui);
uint32_t cp0_ui_selected_app_token(const struct cp0_ui *ui);
bool cp0_ui_selected_app_is_immersive(const struct cp0_ui *ui);
bool cp0_ui_app_is_immersive(const struct cp0_ui *ui, uint32_t token);
bool cp0_ui_show_permission(struct cp0_ui *ui, uint64_t prompt_id,
                            const char *app_name, const char *permission,
                            const char *reason);
void cp0_ui_clear_permission(struct cp0_ui *ui);
bool cp0_ui_show_documents(struct cp0_ui *ui, uint64_t prompt_id,
                           const char *app_name,
                           const struct cp0_ui_document_option *documents,
                           size_t document_count);
void cp0_ui_clear_documents(struct cp0_ui *ui);
const char *cp0_ui_selected_document_id(const struct cp0_ui *ui);
bool cp0_ui_show_notification(struct cp0_ui *ui, uint64_t notification_id,
                              const char *app_name, const char *title,
                              const char *body);
void cp0_ui_clear_notification(struct cp0_ui *ui);
void cp0_ui_set_local_simulation(struct cp0_ui *ui, bool enabled);
void cp0_ui_set_screenshot_status(struct cp0_ui *ui,
                                  enum cp0_ui_screenshot_status status);
bool cp0_ui_tick(struct cp0_ui *ui);
enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action);
void cp0_ui_render(const struct cp0_ui *ui, uint32_t *pixels, int width,
                   int height, int stride_pixels);

#endif
