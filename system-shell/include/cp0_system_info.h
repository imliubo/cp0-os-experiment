#ifndef CP0_SYSTEM_INFO_H
#define CP0_SYSTEM_INFO_H

#include <stdbool.h>
#include <stdint.h>

enum cp0_capability_state {
    CP0_CAPABILITY_UNKNOWN,
    CP0_CAPABILITY_UNAVAILABLE,
    CP0_CAPABILITY_AVAILABLE,
};

enum cp0_battery_status {
    CP0_BATTERY_UNKNOWN,
    CP0_BATTERY_CHARGING,
    CP0_BATTERY_DISCHARGING,
    CP0_BATTERY_FULL,
    CP0_BATTERY_NOT_CHARGING,
};

enum cp0_bus_state {
    CP0_BUS_UNKNOWN,
    CP0_BUS_UNAVAILABLE,
    CP0_BUS_INACCESSIBLE,
    CP0_BUS_READY,
};

struct cp0_system_info {
    bool device_available;
    int battery_percent;
    bool battery_present;
    bool battery_voltage_available;
    bool battery_current_available;
    int64_t battery_voltage_microvolts;
    int64_t battery_current_microamps;
    enum cp0_battery_status battery_status;
    int temperature_millicelsius;
    uint64_t uptime_seconds;
    uint64_t memory_total_bytes;
    uint64_t memory_available_bytes;
    uint64_t storage_total_bytes;
    uint64_t storage_available_bytes;
    char model[33];
    char os_version[33];
    bool network_available;
    bool network_online;
    bool network_link_up;
    char network_interface[17];
    char network_ipv4[16];
    enum cp0_bus_state i2c_bus_state;
    enum cp0_capability_state display_state;
    enum cp0_capability_state keyboard_state;
    enum cp0_capability_state audio_state;
    enum cp0_capability_state camera_state;
};

void cp0_system_info_collect(struct cp0_system_info *info);

#ifdef CP0_SYSTEM_INFO_TEST
bool cp0_system_info_parse_meminfo(const char *document, uint64_t *total_bytes,
                                   uint64_t *available_bytes);
bool cp0_system_info_parse_uptime(const char *document,
                                  uint64_t *uptime_seconds);
bool cp0_system_info_parse_power_value(const char *document, int64_t minimum,
                                       int64_t maximum, int64_t *value);
enum cp0_battery_status cp0_system_info_parse_battery_status(
    const char *status);
#endif

#endif
