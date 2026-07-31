#define _DEFAULT_SOURCE

#include "cp0_system_info.h"

#include <arpa/inet.h>
#include <dirent.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/statvfs.h>

static bool read_document(const char *path, char *output, size_t capacity)
{
    FILE *input;
    size_t length;
    if (output == NULL || capacity < 2)
        return false;
    input = fopen(path, "r");
    if (input == NULL)
        return false;
    length = fread(output, 1, capacity - 1, input);
    fclose(input);
    if (length == 0)
        return false;
    output[length] = '\0';
    return true;
}

static bool read_text(const char *path, char *output, size_t capacity)
{
    if (!read_document(path, output, capacity))
        return false;
    output[strcspn(output, "\r\n")] = '\0';
    return true;
}

static bool parse_meminfo(const char *document, uint64_t *total_bytes,
                          uint64_t *available_bytes)
{
    unsigned long long total_kib = 0;
    unsigned long long available_kib = 0;
    bool saw_total = false;
    bool saw_available = false;
    const char *line = document;
    if (document == NULL || total_bytes == NULL || available_bytes == NULL)
        return false;
    while (*line != '\0') {
        if (sscanf(line, "MemTotal: %llu kB", &total_kib) == 1) {
            saw_total = true;
        } else if (sscanf(line, "MemAvailable: %llu kB", &available_kib) == 1) {
            saw_available = true;
        }
        line = strchr(line, '\n');
        if (line == NULL)
            break;
        line++;
    }
    if (!saw_total || !saw_available || total_kib == 0 ||
        available_kib > total_kib)
        return false;
    *total_bytes = (uint64_t)total_kib * 1024U;
    *available_bytes = (uint64_t)available_kib * 1024U;
    return true;
}

static bool parse_uptime(const char *document, uint64_t *uptime_seconds)
{
    double seconds;
    if (document == NULL || uptime_seconds == NULL ||
        sscanf(document, "%lf", &seconds) != 1 || seconds < 0.0)
        return false;
    *uptime_seconds = (uint64_t)seconds;
    return true;
}

#ifdef CP0_SYSTEM_INFO_TEST
bool cp0_system_info_parse_meminfo(const char *document, uint64_t *total_bytes,
                                   uint64_t *available_bytes)
{
    return parse_meminfo(document, total_bytes, available_bytes);
}

bool cp0_system_info_parse_uptime(const char *document,
                                  uint64_t *uptime_seconds)
{
    return parse_uptime(document, uptime_seconds);
}
#endif

static void collect_device(struct cp0_system_info *info)
{
    char text[4096];
    struct statvfs storage;

    if (read_document("/proc/meminfo", text, sizeof(text)) &&
        parse_meminfo(text, &info->memory_total_bytes,
                      &info->memory_available_bytes))
        info->device_available = true;
    if (read_text("/proc/uptime", text, sizeof(text)))
        parse_uptime(text, &info->uptime_seconds);
    if (statvfs("/", &storage) == 0) {
        info->storage_total_bytes =
            (uint64_t)storage.f_blocks * (uint64_t)storage.f_frsize;
        info->storage_available_bytes =
            (uint64_t)storage.f_bavail * (uint64_t)storage.f_frsize;
    }
    if (!read_text("/sys/firmware/devicetree/base/model", info->model,
                   sizeof(info->model)))
        snprintf(info->model, sizeof(info->model), "CARDPUTER ZERO");

    if (read_document("/etc/os-release", text, sizeof(text))) {
        const char *version = strstr(text, "PRETTY_NAME=");
        if (version != NULL) {
            char parsed[64];
            version += strlen("PRETTY_NAME=");
            size_t length = strcspn(version, "\r\n");
            if (length >= sizeof(parsed))
                length = sizeof(parsed) - 1;
            memcpy(parsed, version, length);
            parsed[length] = '\0';
            if (parsed[0] == '"' && length >= 2 && parsed[length - 1] == '"') {
                parsed[length - 1] = '\0';
                version = parsed + 1;
            } else {
                version = parsed;
            }
            snprintf(info->os_version, sizeof(info->os_version), "%.32s",
                     version);
        }
    }
    if (info->os_version[0] == '\0')
        snprintf(info->os_version, sizeof(info->os_version), "UNKNOWN");

    if (read_text("/sys/class/thermal/thermal_zone0/temp", text, sizeof(text))) {
        char *end;
        long value = strtol(text, &end, 10);
        if (end != text && value >= -40000 && value <= 150000)
            info->temperature_millicelsius = (int)value;
    }
}

static void collect_battery(struct cp0_system_info *info)
{
    DIR *directory = opendir("/sys/class/power_supply");
    struct dirent *entry;
    if (directory == NULL)
        return;
    while ((entry = readdir(directory)) != NULL) {
        char path[512];
        char text[32];
        if (entry->d_name[0] == '.')
            continue;
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/type",
                 entry->d_name);
        if (!read_text(path, text, sizeof(text)) || strcmp(text, "Battery") != 0)
            continue;
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/capacity",
                 entry->d_name);
        if (read_text(path, text, sizeof(text))) {
            char *end;
            long value = strtol(text, &end, 10);
            if (end != text && value >= 0 && value <= 100)
                info->battery_percent = (int)value;
        }
        break;
    }
    closedir(directory);
}

static int interface_priority(const char *name)
{
    if (strcmp(name, "wlan0") == 0)
        return 3;
    if (strcmp(name, "eth0") == 0 || strcmp(name, "end0") == 0)
        return 2;
    return 1;
}

static void collect_network(struct cp0_system_info *info)
{
    struct ifaddrs *addresses;
    int selected_score = -1;
    if (getifaddrs(&addresses) != 0)
        return;
    for (struct ifaddrs *item = addresses; item != NULL; item = item->ifa_next) {
        int score;
        bool up;
        bool ipv4;
        if (item->ifa_name == NULL ||
            (item->ifa_flags & IFF_LOOPBACK) != 0)
            continue;
        up = (item->ifa_flags & IFF_UP) != 0;
        ipv4 = item->ifa_addr != NULL && item->ifa_addr->sa_family == AF_INET;
        score = interface_priority(item->ifa_name) + (up ? 4 : 0) +
                (ipv4 && up ? 8 : 0);
        if (selected_score > score)
            continue;
        info->network_available = true;
        info->network_link_up = up;
        info->network_online = up && ipv4;
        snprintf(info->network_interface, sizeof(info->network_interface), "%s",
                 item->ifa_name);
        if (!ipv4 || inet_ntop(
                         AF_INET,
                         &((const struct sockaddr_in *)item->ifa_addr)->sin_addr,
                         info->network_ipv4,
                         sizeof(info->network_ipv4)) == NULL)
            info->network_ipv4[0] = '\0';
        selected_score = score;
    }
    freeifaddrs(addresses);
}

void cp0_system_info_collect(struct cp0_system_info *info)
{
    if (info == NULL)
        return;
    memset(info, 0, sizeof(*info));
    info->battery_percent = -1;
    info->temperature_millicelsius = -1;
    collect_device(info);
    collect_battery(info);
    collect_network(info);
}
