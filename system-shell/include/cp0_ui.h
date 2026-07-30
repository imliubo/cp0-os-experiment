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

enum cp0_ui_screen {
    CP0_UI_HOME,
    CP0_UI_APPS,
    CP0_UI_DEVICE,
    CP0_UI_NETWORK,
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
};

enum cp0_ui_app_state {
    CP0_UI_APP_STOPPED,
    CP0_UI_APP_STARTING,
    CP0_UI_APP_RUNNING,
    CP0_UI_APP_FAILED,
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

struct cp0_ui {
    enum cp0_ui_screen screen;
    unsigned int selected;
    unsigned int app_selected;
    unsigned int app_count;
    unsigned int task_action_selected;
    bool app_list_truncated;
    unsigned int dialog_selected;
    bool power_dialog;
    bool permission_prompt;
    bool notification_banner;
    bool network_online;
    int battery_percent;
    char clock_text[6];
    uint64_t prompt_id;
    unsigned int prompt_selected;
    char prompt_app_name[CP0_UI_PROMPT_APP_NAME_MAX + 1];
    char prompt_permission[CP0_UI_PROMPT_PERMISSION_MAX + 1];
    char prompt_reason[CP0_UI_PROMPT_REASON_MAX + 1];
    uint64_t notification_id;
    char notification_app_name[CP0_UI_NOTIFICATION_APP_NAME_MAX + 1];
    char notification_title[CP0_UI_NOTIFICATION_TITLE_MAX + 1];
    char notification_body[CP0_UI_NOTIFICATION_BODY_MAX + 1];
    struct cp0_ui_app apps[CP0_UI_MAX_APPS];
};

void cp0_ui_init(struct cp0_ui *ui);
void cp0_ui_set_status(struct cp0_ui *ui, const char *clock_text,
                       bool network_online, int battery_percent);
void cp0_ui_add_app(struct cp0_ui *ui, uint32_t token, const char *app_id);
void cp0_ui_sync_app_catalog(struct cp0_ui *ui,
                             const struct cp0_ui_catalog_app *apps,
                             size_t app_count, bool truncated);
void cp0_ui_set_app_display_mode(struct cp0_ui *ui, uint32_t token,
                                 bool immersive);
void cp0_ui_remove_app(struct cp0_ui *ui, uint32_t token);
void cp0_ui_set_app_state(struct cp0_ui *ui, const char *app_id,
                          enum cp0_ui_app_state state);
const char *cp0_ui_selected_app_id(const struct cp0_ui *ui);
enum cp0_ui_app_state cp0_ui_selected_app_state(const struct cp0_ui *ui);
uint32_t cp0_ui_selected_app_token(const struct cp0_ui *ui);
bool cp0_ui_selected_app_is_immersive(const struct cp0_ui *ui);
bool cp0_ui_app_is_immersive(const struct cp0_ui *ui, uint32_t token);
bool cp0_ui_show_permission(struct cp0_ui *ui, uint64_t prompt_id,
                            const char *app_name, const char *permission,
                            const char *reason);
void cp0_ui_clear_permission(struct cp0_ui *ui);
bool cp0_ui_show_notification(struct cp0_ui *ui, uint64_t notification_id,
                              const char *app_name, const char *title,
                              const char *body);
void cp0_ui_clear_notification(struct cp0_ui *ui);
enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action);
void cp0_ui_render(const struct cp0_ui *ui, uint32_t *pixels, int width,
                   int height, int stride_pixels);

#endif
