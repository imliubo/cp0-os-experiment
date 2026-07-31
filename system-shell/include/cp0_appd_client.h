#ifndef CP0_APPD_CLIENT_H
#define CP0_APPD_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_APPD_MAX_APPS 32
#define CP0_APP_ID_BYTES 129
#define CP0_APP_NAME_BYTES 129
#define CP0_APP_VERSION_BYTES 65
#define CP0_NOTIFICATION_APP_NAME_BYTES 129
#define CP0_NOTIFICATION_TITLE_BYTES 129
#define CP0_NOTIFICATION_BODY_BYTES 641
#define CP0_PROMPT_APP_NAME_BYTES 129
#define CP0_PROMPT_PERMISSION_BYTES 64
#define CP0_PROMPT_REASON_BYTES 641
#define CP0_DOCUMENT_MAX 16
#define CP0_DOCUMENT_ID_BYTES 33
#define CP0_DOCUMENT_NAME_BYTES 129

enum cp0_permission_choice {
    CP0_PERMISSION_ALLOW_ONCE,
    CP0_PERMISSION_ALLOW_ALWAYS,
    CP0_PERMISSION_DENY,
};

enum cp0_management_authority {
    CP0_AUTHORITY_PERSONAL,
    CP0_AUTHORITY_PARENT,
    CP0_AUTHORITY_ORGANIZATION,
};

enum cp0_device_mode {
    CP0_DEVICE_MODE_DEVELOPER,
    CP0_DEVICE_MODE_RECOVERY,
};

struct cp0_device_settings {
    enum cp0_management_authority authority;
    bool developer_mode;
    bool developer_mode_allowed;
    bool recovery_mode;
    bool recovery_mode_allowed;
    bool store_install_allowed;
    bool app_launch_restricted;
    uint8_t denied_permission_count;
};

struct cp0_permission_prompt {
    uint64_t prompt_id;
    char app_name[CP0_PROMPT_APP_NAME_BYTES];
    char permission[CP0_PROMPT_PERMISSION_BYTES];
    char reason[CP0_PROMPT_REASON_BYTES];
};

struct cp0_app_summary {
    bool running;
    bool immersive;
    char app_id[CP0_APP_ID_BYTES];
    char name[CP0_APP_NAME_BYTES];
    char version[CP0_APP_VERSION_BYTES];
};

struct cp0_app_list {
    size_t count;
    bool truncated;
    struct cp0_app_summary apps[CP0_APPD_MAX_APPS];
};

struct cp0_notification {
    uint64_t notification_id;
    char app_id[CP0_APP_ID_BYTES];
    char app_name[CP0_NOTIFICATION_APP_NAME_BYTES];
    char title[CP0_NOTIFICATION_TITLE_BYTES];
    char body[CP0_NOTIFICATION_BODY_BYTES];
};

struct cp0_document_summary {
    uint64_t size_bytes;
    char document_id[CP0_DOCUMENT_ID_BYTES];
    char name[CP0_DOCUMENT_NAME_BYTES];
};

struct cp0_document_prompt {
    uint64_t prompt_id;
    size_t document_count;
    char app_name[CP0_PROMPT_APP_NAME_BYTES];
    struct cp0_document_summary documents[CP0_DOCUMENT_MAX];
};

int cp0_appd_list_apps(struct cp0_app_list *list);
int cp0_appd_start_app(const char *app_id);
int cp0_appd_stop_app(const char *app_id);
int cp0_appd_take_notification(struct cp0_notification *notification);
int cp0_appd_get_permission_prompt(struct cp0_permission_prompt *prompt);
int cp0_appd_resolve_permission(uint64_t prompt_id,
                                enum cp0_permission_choice choice);
int cp0_appd_get_document_prompt(struct cp0_document_prompt *prompt);
int cp0_appd_resolve_document(uint64_t prompt_id,
                              const char *document_id);
int cp0_appd_get_device_settings(struct cp0_device_settings *settings);
int cp0_appd_set_device_mode(enum cp0_device_mode mode, bool enabled,
                             struct cp0_device_settings *settings);

#endif
