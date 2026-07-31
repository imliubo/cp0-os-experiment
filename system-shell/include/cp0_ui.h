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
};

enum cp0_ui_event {
    CP0_UI_EVENT_NONE,
    CP0_UI_EVENT_SLEEP,
    CP0_UI_EVENT_RESTART,
    CP0_UI_EVENT_OPEN_APP,
    CP0_UI_EVENT_STOP_APP,
    CP0_UI_EVENT_PERMISSION_ONCE,
    CP0_UI_EVENT_PERMISSION_ALWAYS,
    CP0_UI_EVENT_PERMISSION_DENY,
    CP0_UI_EVENT_DOCUMENT_SELECT,
    CP0_UI_EVENT_DOCUMENT_CANCEL,
    CP0_UI_EVENT_STORE_REFRESH,
    CP0_UI_EVENT_STORE_INSTALL,
    CP0_UI_EVENT_DEVELOPER_ENABLE,
    CP0_UI_EVENT_DEVELOPER_DISABLE,
    CP0_UI_EVENT_RECOVERY_ENABLE,
    CP0_UI_EVENT_RECOVERY_DISABLE,
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
    CP0_UI_STORE_INSTALLING,
    CP0_UI_STORE_INSTALLED,
    CP0_UI_STORE_FAILED,
};

enum cp0_ui_store_status {
    CP0_UI_STORE_LOADING,
    CP0_UI_STORE_READY,
    CP0_UI_STORE_UNCONFIGURED,
    CP0_UI_STORE_UNAVAILABLE,
};

struct cp0_ui_catalog_app {
    bool running;
    bool immersive;
    const char *app_id;
    const char *name;
};

struct cp0_ui_app {
    uint32_t token;
    bool installed;
    bool immersive;
    enum cp0_ui_app_state state;
    char app_id[CP0_UI_APP_ID_MAX + 1];
    char name[CP0_UI_APP_NAME_MAX + 1];
};

struct cp0_ui_store_catalog_app {
    uint64_t package_bytes;
    uint16_t permissions;
    uint8_t progress_percent;
    enum cp0_ui_store_state state;
    const char *app_id;
    const char *name;
    const char *version;
    const char *summary;
    const char *installed_version;
};

struct cp0_ui_store_app {
    uint64_t package_bytes;
    uint16_t permissions;
    uint8_t progress_percent;
    enum cp0_ui_store_state state;
    char app_id[CP0_UI_APP_ID_MAX + 1];
    char name[CP0_UI_APP_NAME_MAX + 1];
    char version[CP0_UI_STORE_VERSION_MAX + 1];
    char summary[CP0_UI_STORE_SUMMARY_MAX + 1];
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
    unsigned int task_action_selected;
    unsigned int settings_selected;
    bool app_list_truncated;
    bool store_list_truncated;
    bool store_catalog_stale;
    bool store_detail;
    enum cp0_ui_store_status store_status;
    unsigned int dialog_selected;
    bool power_dialog;
    bool settings_available;
    bool settings_confirm;
    bool settings_confirm_recovery;
    bool developer_mode;
    bool developer_mode_allowed;
    bool recovery_mode;
    bool recovery_mode_allowed;
    bool store_install_allowed;
    bool app_launch_restricted;
    enum cp0_ui_authority settings_authority;
    uint8_t denied_permission_count;
    bool permission_prompt;
    bool document_prompt;
    bool notification_banner;
    bool network_online;
    int battery_percent;
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
    struct cp0_ui_app apps[CP0_UI_MAX_APPS];
    struct cp0_ui_store_app store_apps[CP0_UI_MAX_APPS];
};

void cp0_ui_init(struct cp0_ui *ui);
void cp0_ui_set_status(struct cp0_ui *ui, const char *clock_text,
                       bool network_online, int battery_percent);
void cp0_ui_set_device_settings(
    struct cp0_ui *ui, enum cp0_ui_authority authority,
    bool developer_mode, bool developer_mode_allowed, bool recovery_mode,
    bool recovery_mode_allowed, bool store_install_allowed,
    bool app_launch_restricted, uint8_t denied_permission_count);
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
void cp0_ui_set_store_app_state(struct cp0_ui *ui, const char *app_id,
                                enum cp0_ui_store_state state,
                                uint8_t progress_percent);
const char *cp0_ui_selected_store_app_id(const struct cp0_ui *ui);
enum cp0_ui_store_state cp0_ui_selected_store_app_state(
    const struct cp0_ui *ui);
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
enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action);
void cp0_ui_render(const struct cp0_ui *ui, uint32_t *pixels, int width,
                   int height, int stride_pixels);

#endif
