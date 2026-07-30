#ifndef CP0_APPD_CLIENT_H
#define CP0_APPD_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_APPD_MAX_APPS 32
#define CP0_APP_ID_BYTES 129
#define CP0_APP_NAME_BYTES 129
#define CP0_APP_VERSION_BYTES 65
#define CP0_PROMPT_APP_NAME_BYTES 129
#define CP0_PROMPT_PERMISSION_BYTES 64
#define CP0_PROMPT_REASON_BYTES 641

enum cp0_permission_choice {
    CP0_PERMISSION_ALLOW_ONCE,
    CP0_PERMISSION_ALLOW_ALWAYS,
    CP0_PERMISSION_DENY,
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

int cp0_appd_list_apps(struct cp0_app_list *list);
int cp0_appd_start_app(const char *app_id);
int cp0_appd_stop_app(const char *app_id);
int cp0_appd_get_permission_prompt(struct cp0_permission_prompt *prompt);
int cp0_appd_resolve_permission(uint64_t prompt_id,
                                enum cp0_permission_choice choice);

#endif
