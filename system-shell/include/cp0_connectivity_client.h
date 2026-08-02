#ifndef CP0_CONNECTIVITY_CLIENT_H
#define CP0_CONNECTIVITY_CLIENT_H

#include <stdbool.h>

enum cp0_connectivity_result {
    CP0_CONNECTIVITY_FAILED = -1,
    CP0_CONNECTIVITY_OK = 0,
    CP0_CONNECTIVITY_UNAVAILABLE = 1,
};

struct cp0_connectivity_state {
    bool available;
    bool wifi_available;
    bool wifi_enabled;
    bool airplane_mode;
};

int cp0_connectivity_get_state(struct cp0_connectivity_state *state);
int cp0_connectivity_set_wifi_enabled(
    bool enabled, struct cp0_connectivity_state *state);
int cp0_connectivity_set_airplane_mode(
    bool enabled, struct cp0_connectivity_state *state);

#endif
