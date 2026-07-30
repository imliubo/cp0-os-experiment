#ifndef CP0_APPD_CLIENT_H
#define CP0_APPD_CLIENT_H

#include <stdint.h>

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

int cp0_appd_get_permission_prompt(struct cp0_permission_prompt *prompt);
int cp0_appd_resolve_permission(uint64_t prompt_id,
                                enum cp0_permission_choice choice);

#endif
