#include "cp0_system_info.h"

#include <assert.h>
#include <stdint.h>
#include <string.h>

int main(void)
{
    uint64_t total;
    uint64_t available;
    uint64_t uptime;
    struct cp0_system_info info;

    assert(cp0_system_info_parse_meminfo(
        "MemTotal:         512000 kB\nMemFree: 12000 kB\n"
        "MemAvailable:     345678 kB\n",
        &total, &available));
    assert(total == 512000U * 1024U);
    assert(available == 345678U * 1024U);
    assert(!cp0_system_info_parse_meminfo("MemTotal: 1 kB\n", &total,
                                          &available));
    assert(!cp0_system_info_parse_meminfo(
        "MemTotal: 10 kB\nMemAvailable: 11 kB\n", &total, &available));
    assert(cp0_system_info_parse_uptime("93784.42 100.00\n", &uptime));
    assert(uptime == 93784U);
    assert(!cp0_system_info_parse_uptime("invalid", &uptime));
    assert(!cp0_system_info_parse_uptime("-1.0", &uptime));

    cp0_system_info_collect(&info);
    assert(info.battery_percent >= -1 && info.battery_percent <= 100);
    assert(info.temperature_millicelsius >= -1 &&
           info.temperature_millicelsius <= 150000);
    assert(memchr(info.model, '\0', sizeof(info.model)) != NULL);
    assert(memchr(info.os_version, '\0', sizeof(info.os_version)) != NULL);
    assert(memchr(info.network_interface, '\0',
                  sizeof(info.network_interface)) != NULL);
    assert(memchr(info.network_ipv4, '\0', sizeof(info.network_ipv4)) != NULL);
    if (info.device_available) {
        assert(info.memory_total_bytes > 0);
        assert(info.memory_available_bytes <= info.memory_total_bytes);
    }
    if (info.network_online) {
        assert(info.network_available && info.network_link_up);
        assert(info.network_interface[0] != '\0');
        assert(info.network_ipv4[0] != '\0');
    }
    return 0;
}
