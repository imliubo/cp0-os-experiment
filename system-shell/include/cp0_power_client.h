#ifndef CP0_POWER_CLIENT_H
#define CP0_POWER_CLIENT_H

enum cp0_power_action {
    CP0_POWER_RESTART,
    CP0_POWER_OFF,
};

int cp0_power_request(enum cp0_power_action action);

#endif
