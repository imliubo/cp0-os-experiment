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
#include <sys/stat.h>
#include <unistd.h>

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

static bool parse_power_value(const char *document, int64_t minimum,
                              int64_t maximum, int64_t *value)
{
    char *end;
    long long parsed;
    if (document == NULL || value == NULL || minimum > maximum)
        return false;
    parsed = strtoll(document, &end, 10);
    if (end == document || (*end != '\0' && *end != '\n' && *end != '\r') ||
        parsed < minimum || parsed > maximum)
        return false;
    *value = (int64_t)parsed;
    return true;
}

static enum cp0_battery_status parse_battery_status(const char *status)
{
    if (status == NULL)
        return CP0_BATTERY_UNKNOWN;
    if (strcmp(status, "Charging") == 0)
        return CP0_BATTERY_CHARGING;
    if (strcmp(status, "Discharging") == 0)
        return CP0_BATTERY_DISCHARGING;
    if (strcmp(status, "Full") == 0)
        return CP0_BATTERY_FULL;
    if (strcmp(status, "Not charging") == 0)
        return CP0_BATTERY_NOT_CHARGING;
    return CP0_BATTERY_UNKNOWN;
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

bool cp0_system_info_parse_power_value(const char *document, int64_t minimum,
                                       int64_t maximum, int64_t *value)
{
    return parse_power_value(document, minimum, maximum, value);
}

enum cp0_battery_status cp0_system_info_parse_battery_status(
    const char *status)
{
    return parse_battery_status(status);
}
#endif

static enum cp0_bus_state classify_i2c_bus(bool sysfs_present,
                                           bool device_present,
                                           bool device_accessible)
{
    if (!sysfs_present && !device_present)
        return CP0_BUS_UNAVAILABLE;
    if (device_present && device_accessible)
        return CP0_BUS_READY;
    return CP0_BUS_INACCESSIBLE;
}

static bool directory_attribute_matches(const char *directory_path,
                                        const char *entry_prefix,
                                        const char *attribute,
                                        const char *expected)
{
    DIR *directory = opendir(directory_path);
    struct dirent *entry;
    size_t prefix_length;
    bool matched = false;
    if (directory == NULL || entry_prefix == NULL || attribute == NULL ||
        expected == NULL)
        return false;
    prefix_length = strlen(entry_prefix);
    while ((entry = readdir(directory)) != NULL) {
        char path[512];
        char text[128];
        int length;
        if (strncmp(entry->d_name, entry_prefix, prefix_length) != 0)
            continue;
        length = snprintf(path, sizeof(path), "%s/%s/%s", directory_path,
                          entry->d_name, attribute);
        if (length < 0 || (size_t)length >= sizeof(path))
            continue;
        if (read_text(path, text, sizeof(text)) && strcmp(text, expected) == 0) {
            matched = true;
            break;
        }
    }
    closedir(directory);
    return matched;
}

#ifdef CP0_SYSTEM_INFO_TEST
enum cp0_bus_state cp0_system_info_classify_i2c_bus(bool sysfs_present,
                                                     bool device_present,
                                                     bool device_accessible)
{
    return classify_i2c_bus(sysfs_present, device_present, device_accessible);
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
    if (statvfs("/var/lib/cardputerzero", &storage) != 0)
        memset(&storage, 0, sizeof(storage));
    if (storage.f_blocks != 0) {
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
        info->battery_present = true;
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/capacity",
                 entry->d_name);
        if (read_text(path, text, sizeof(text))) {
            char *end;
            long value = strtol(text, &end, 10);
            if (end != text && value >= 0 && value <= 100)
                info->battery_percent = (int)value;
        }
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/status",
                 entry->d_name);
        if (read_text(path, text, sizeof(text)))
            info->battery_status = parse_battery_status(text);
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/voltage_now",
                 entry->d_name);
        if (read_text(path, text, sizeof(text)) &&
            parse_power_value(text, 0, 20000000,
                              &info->battery_voltage_microvolts))
            info->battery_voltage_available = true;
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/current_now",
                 entry->d_name);
        if (read_text(path, text, sizeof(text)) &&
            parse_power_value(text, -10000000, 10000000,
                              &info->battery_current_microamps))
            info->battery_current_available = true;
        break;
    }
    closedir(directory);
}

static enum cp0_capability_state path_capability(const char *path)
{
    struct stat metadata;
    if (stat(path, &metadata) == 0)
        return CP0_CAPABILITY_AVAILABLE;
    return CP0_CAPABILITY_UNAVAILABLE;
}

static void collect_capabilities(struct cp0_system_info *info)
{
    struct stat metadata;
    bool sysfs_present =
        stat("/sys/bus/i2c/devices/i2c-1", &metadata) == 0;
    bool device_present = stat("/dev/i2c-1", &metadata) == 0;
    bool device_accessible =
        device_present && access("/dev/i2c-1", R_OK | W_OK) == 0;
    info->i2c_bus_state = classify_i2c_bus(
        sysfs_present, device_present, device_accessible);
    info->display_state =
        directory_attribute_matches("/sys/class/drm", "card0-SPI-", "status",
                                    "connected")
            ? CP0_CAPABILITY_AVAILABLE
            : CP0_CAPABILITY_UNAVAILABLE;
    info->keyboard_state =
        directory_attribute_matches("/sys/class/input", "input", "name",
                                    "tca8418c")
            ? CP0_CAPABILITY_AVAILABLE
            : CP0_CAPABILITY_UNAVAILABLE;
    info->audio_state =
        directory_attribute_matches("/sys/class/sound", "card", "id",
                                    "ES8389Audio")
            ? CP0_CAPABILITY_AVAILABLE
            : CP0_CAPABILITY_UNAVAILABLE;
    info->camera_state = path_capability("/sys/class/video4linux/video0");
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
    collect_capabilities(info);
}
