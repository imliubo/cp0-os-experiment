#ifndef CP0_PROVISION_CLIENT_H
#define CP0_PROVISION_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define CP0_PROVISION_TEXT_MAX 128
#define CP0_PROVISION_HOSTNAME_MAX 63
#define CP0_PROVISION_USERNAME_MAX 32
#define CP0_PROVISION_SSID_MAX 32
#define CP0_PROVISION_ERROR_MAX 160
#define CP0_PROVISION_WIFI_MAX 64
#define CP0_PROVISION_IPV4_MAX 15

enum cp0_provision_result {
    CP0_PROVISION_FAILED = -1,
    CP0_PROVISION_OK = 0,
    CP0_PROVISION_UNAVAILABLE = 1,
    CP0_PROVISION_INVALID_STATE = 2,
    CP0_PROVISION_INVALID_VALUE = 3,
    CP0_PROVISION_REPAIR_REQUIRED = 4,
};

enum cp0_provision_phase {
    CP0_PROVISION_UNPROVISIONED,
    CP0_PROVISION_REGION,
    CP0_PROVISION_OWNER,
    CP0_PROVISION_PASSWORD_READY,
    CP0_PROVISION_NETWORK,
    CP0_PROVISION_REMOTE_ACCESS,
    CP0_PROVISION_REVIEW,
    CP0_PROVISION_COMMITTING,
    CP0_PROVISION_COMPLETE,
    CP0_PROVISION_REPAIR,
};

enum cp0_provision_network_kind {
    CP0_PROVISION_NETWORK_NONE,
    CP0_PROVISION_NETWORK_ETHERNET,
    CP0_PROVISION_NETWORK_WIFI,
    CP0_PROVISION_NETWORK_OFFLINE,
};

enum cp0_provision_wifi_security {
    CP0_PROVISION_WIFI_OPEN,
    CP0_PROVISION_WIFI_WPA2,
    CP0_PROVISION_WIFI_WPA3,
    CP0_PROVISION_WIFI_UNSUPPORTED,
};

struct cp0_provision_status {
    enum cp0_provision_phase phase;
    enum cp0_provision_network_kind network_kind;
    bool password_configured;
    bool ssh_enabled;
    bool network_manager_available;
    bool ethernet_connected;
    bool wifi_available;
    bool wifi_connected;
    char locale[CP0_PROVISION_TEXT_MAX + 1];
    char country[3];
    char timezone[CP0_PROVISION_TEXT_MAX + 1];
    char hostname[CP0_PROVISION_HOSTNAME_MAX + 1];
    char display_name[CP0_PROVISION_TEXT_MAX + 1];
    char username[CP0_PROVISION_USERNAME_MAX + 1];
    char network_ssid[CP0_PROVISION_SSID_MAX * 4 + 1];
    char ethernet_ipv4[CP0_PROVISION_IPV4_MAX + 1];
    char wifi_ipv4[CP0_PROVISION_IPV4_MAX + 1];
};

struct cp0_provision_wifi_network {
    enum cp0_provision_wifi_security security;
    uint8_t signal_percent;
    bool connected;
    char ssid[CP0_PROVISION_SSID_MAX * 4 + 1];
};

struct cp0_provision_wifi_list {
    size_t count;
    struct cp0_provision_wifi_network networks[CP0_PROVISION_WIFI_MAX];
};

int cp0_provision_get_status(struct cp0_provision_status *status,
                             char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_set_region(const char *locale, const char *country,
                             const char *timezone, const char *hostname,
                             struct cp0_provision_status *status,
                             char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_set_owner(const char *display_name, const char *username,
                            struct cp0_provision_status *status,
                            char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_set_password(
    char *password, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_list_wifi(struct cp0_provision_wifi_list *list,
                            char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_connect_wifi(
    const char *ssid, enum cp0_provision_wifi_security security,
    char *password, bool hidden, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_use_ethernet(struct cp0_provision_status *status,
                               char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_use_offline(struct cp0_provision_status *status,
                              char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_set_ssh_enabled(
    bool enabled, struct cp0_provision_status *status,
    char error[CP0_PROVISION_ERROR_MAX + 1]);
int cp0_provision_commit(struct cp0_provision_status *status,
                         char error[CP0_PROVISION_ERROR_MAX + 1]);

#endif
