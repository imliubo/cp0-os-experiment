#ifndef CP0_SYSTEM_INFO_H
#define CP0_SYSTEM_INFO_H

#include <stdbool.h>
#include <stdint.h>

struct cp0_system_info {
    bool device_available;
    int battery_percent;
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
};

void cp0_system_info_collect(struct cp0_system_info *info);

#ifdef CP0_SYSTEM_INFO_TEST
bool cp0_system_info_parse_meminfo(const char *document, uint64_t *total_bytes,
                                   uint64_t *available_bytes);
bool cp0_system_info_parse_uptime(const char *document,
                                  uint64_t *uptime_seconds);
#endif

#endif
