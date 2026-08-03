#define _POSIX_C_SOURCE 200809L

#include "cardputerzero-system-shell-client-protocol.h"
#include "cp0_appd_client.h"
#include "cp0_audio_settings_client.h"
#include "cp0_connectivity_client.h"
#include "cp0_display_client.h"
#include "cp0_developer_client.h"
#include "cp0_power_client.h"
#include "cp0_provision_client.h"
#include "cp0_screenshot_store.h"
#include "cp0_shell_settings.h"
#include "cp0_store_client.h"
#include "cp0_system_info.h"
#include "cp0_ui.h"
#include "overlay-state.h"
#include "weston-output-capture-client-protocol.h"
#include "xdg-shell-client-protocol.h"

#include <errno.h>
#include <drm_fourcc.h>
#include <fcntl.h>
#include <linux/input-event-codes.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/timerfd.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#define BUFFER_COUNT 2
#define SCREENSHOT_RETRY_MAX 2U
#define CP0_APP_SURFACE_MAX 32U
#define CP0_SHELL_SETTINGS_PATH "/var/lib/cardputerzero/shell/settings.conf"

_Static_assert(CP0_STORE_INSTALL_BATCH_MAX == CP0_UI_STORE_UPDATE_BATCH_MAX,
               "Store batch limits must match");
_Static_assert((uint32_t)CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL ==
                       (uint32_t)CP0_OVERLAY_STATE_FULL &&
                   (uint32_t)CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS ==
                       (uint32_t)CP0_OVERLAY_STATE_STATUS &&
                   (uint32_t)CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN ==
                       (uint32_t)CP0_OVERLAY_STATE_HIDDEN &&
                   (uint32_t)CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION ==
                       (uint32_t)CP0_OVERLAY_STATE_NOTIFICATION,
               "overlay protocol values must match the shared state model");

struct shell;

struct shell_buffer {
    struct shell *shell;
    struct wl_buffer *wl_buffer;
    uint32_t *pixels;
    size_t size;
    int width;
    int height;
    bool busy;
};

struct screenshot_buffer {
    struct wl_buffer *wl_buffer;
    uint32_t *pixels;
    size_t size;
};

struct app_surface_binding {
    uint32_t token;
    uint32_t account_uid;
    char app_id[CP0_APP_ID_BYTES];
};

struct shell {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct wl_keyboard *keyboard;
    struct xdg_wm_base *wm_base;
    struct cp0_system_shell_v1 *system_control;
    struct weston_capture_v1 *capture_factory;
    struct weston_capture_source_v1 *capture_source;
    struct wl_output *capture_output;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *xdg_toplevel;
    struct shell_buffer buffers[BUFFER_COUNT];
    struct cp0_ui ui;
    uint32_t overlay_mode;
    uint32_t interrupted_overlay_mode;
    uint32_t document_restore_mode;
    uint32_t notification_restore_mode;
    uint32_t system_action_restore_mode;
    uint32_t screenshot_restore_mode;
    uint32_t capture_factory_name;
    uint32_t capture_output_name;
    uint32_t capture_format;
    int32_t capture_width;
    int32_t capture_height;
    unsigned int capture_retries;
    bool capture_busy;
    struct screenshot_buffer screenshot_buffer;
    int timer_fd;
    bool provision_retry_pending;
    int width;
    int height;
    bool configured;
    bool has_argb;
    bool meta_pressed;
    bool shift_pressed;
    bool shift_modifier_active;
    uint32_t shift_modifier_mask;
    bool redraw_pending;
    bool has_installed_apps;
    unsigned int catalog_ticks;
    unsigned int store_poll_delay;
    unsigned int notification_ticks;
    uint64_t store_notification_serial;
    uint64_t store_catalog_sequence;
    struct cp0_store_install_preflight store_preflight;
    struct cp0_app_list installed_apps;
    struct cp0_task_list task_list;
    uint32_t store_icon_pixels[CP0_STORE_ICON_MAX_PIXELS];
    uint32_t store_screenshot_pixels[CP0_STORE_SCREENSHOT_PIXELS];
    char pending_activation[CP0_APP_ID_BYTES];
    uint32_t pending_activation_uid;
    struct app_surface_binding app_surfaces[CP0_APP_SURFACE_MAX];
    size_t app_surface_count;
};

static volatile sig_atomic_t stop_requested;
static unsigned int shm_serial;

static void shell_redraw(struct shell *shell);
static void begin_screenshot(struct shell *shell);
static void maybe_create_capture_source(struct shell *shell);
static void poll_app_catalog(struct shell *shell);
static void poll_task_catalog(struct shell *shell);
static void poll_store_catalog(struct shell *shell);
static void poll_display_state(struct shell *shell);
static void poll_audio_output_state(struct shell *shell);
static void poll_connectivity_state(struct shell *shell);

static void apply_provision_network_status(
    struct shell *shell, const struct cp0_provision_status *status)
{
    cp0_ui_setup_set_network_status(
        &shell->ui, status->network_manager_available,
        status->ethernet_connected, status->ethernet_ipv4,
        status->wifi_available, status->wifi_connected, status->wifi_ipv4);
}

static struct app_surface_binding *surface_binding(struct shell *shell,
                                                   uint32_t token)
{
    for (size_t index = 0; index < shell->app_surface_count; index++) {
        if (shell->app_surfaces[index].token == token)
            return &shell->app_surfaces[index];
    }
    return NULL;
}

static struct app_surface_binding *remember_surface(struct shell *shell,
                                                    uint32_t token,
                                                    const char *app_id)
{
    struct app_surface_binding *binding = surface_binding(shell, token);
    if (binding == NULL) {
        if (shell->app_surface_count == CP0_APP_SURFACE_MAX)
            return NULL;
        binding = &shell->app_surfaces[shell->app_surface_count++];
        memset(binding, 0, sizeof(*binding));
        binding->token = token;
    }
    if (app_id != NULL)
        snprintf(binding->app_id, sizeof(binding->app_id), "%s", app_id);
    return binding;
}

static void forget_surface(struct shell *shell, uint32_t token)
{
    for (size_t index = 0; index < shell->app_surface_count; index++) {
        if (shell->app_surfaces[index].token != token)
            continue;
        if (index + 1 < shell->app_surface_count) {
            memmove(&shell->app_surfaces[index], &shell->app_surfaces[index + 1],
                    (shell->app_surface_count - index - 1) *
                        sizeof(shell->app_surfaces[0]));
        }
        shell->app_surface_count--;
        memset(&shell->app_surfaces[shell->app_surface_count], 0,
               sizeof(shell->app_surfaces[0]));
        return;
    }
}

static uint32_t token_for_account_uid(const struct shell *shell,
                                      uint32_t account_uid)
{
    if (account_uid == 0)
        return 0;
    for (size_t index = shell->app_surface_count; index > 0; index--) {
        if (shell->app_surfaces[index - 1].account_uid == account_uid)
            return shell->app_surfaces[index - 1].token;
    }
    return 0;
}

static void apply_provision_status(struct shell *shell,
                                   const struct cp0_provision_status *status,
                                   bool show_complete)
{
    cp0_ui_setup_resume(&shell->ui, (unsigned int)status->phase,
                        status->hostname, status->display_name,
                        status->username, status->ssh_enabled);
    apply_provision_network_status(shell, status);
    if (strcmp(status->locale, "zh_CN.UTF-8") == 0)
        shell->ui.setup_language = 1;
    static const char *countries[] = {"CN", "US", "GB", "DE", "JP"};
    static const char *timezones[] = {
        "Asia/Shanghai", "America/Los_Angeles", "Europe/London",
        "Europe/Berlin", "Asia/Tokyo",
    };
    for (unsigned int index = 0; index < 5; index++) {
        if (strcmp(status->country, countries[index]) == 0)
            shell->ui.setup_country = index;
        if (strcmp(status->timezone, timezones[index]) == 0)
            shell->ui.setup_timezone = index;
    }
    if (status->network_kind == CP0_PROVISION_NETWORK_ETHERNET)
        shell->ui.setup_network = 0;
    else if (status->network_kind == CP0_PROVISION_NETWORK_WIFI)
        shell->ui.setup_network = 1;
    else if (status->network_kind == CP0_PROVISION_NETWORK_OFFLINE)
        shell->ui.setup_network = 2;
    if (status->phase == CP0_PROVISION_COMPLETE && !show_complete)
        shell->ui.setup_active = false;
}

static bool reconcile_provisioning(struct shell *shell, bool show_complete)
{
    struct cp0_provision_status status = {0};
    char error[CP0_PROVISION_ERROR_MAX + 1] = {0};
    int result = cp0_provision_get_status(&status, error);

    shell->provision_retry_pending = result == CP0_PROVISION_UNAVAILABLE;
    if (result == CP0_PROVISION_OK) {
        apply_provision_status(shell, &status, show_complete);
        return true;
    }
    if (result == CP0_PROVISION_REPAIR_REQUIRED) {
        cp0_ui_setup_begin(&shell->ui, CP0_UI_SETUP_REPAIR);
        return true;
    }
    return false;
}

static void initialize_provisioning(struct shell *shell)
{
    struct cp0_provision_status status = {0};
    char error[CP0_PROVISION_ERROR_MAX + 1] = {0};
    int result = cp0_provision_get_status(&status, error);
    if (result == CP0_PROVISION_OK) {
        shell->provision_retry_pending = false;
        apply_provision_status(shell, &status, false);
        return;
    }
    shell->provision_retry_pending = result == CP0_PROVISION_UNAVAILABLE;
    cp0_ui_setup_begin(&shell->ui,
                       result == CP0_PROVISION_REPAIR_REQUIRED
                           ? CP0_UI_SETUP_REPAIR
                           : CP0_UI_SETUP_ERROR);
    snprintf(shell->ui.setup_error, sizeof(shell->ui.setup_error), "%s",
             error[0] != '\0' ? error : "Provisioning service is unavailable");
}

static void begin_normal_shell(struct shell *shell)
{
    shell->ui.setup_active = false;
    shell->ui.screen = CP0_UI_HOME;
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                          shell->overlay_mode);
    poll_app_catalog(shell);
    poll_task_catalog(shell);
    poll_store_catalog(shell);
    poll_display_state(shell);
    poll_audio_output_state(shell);
    poll_connectivity_state(shell);
}

static void handle_setup_event(struct shell *shell, enum cp0_ui_event event)
{
    struct cp0_provision_status status = {0};
    struct cp0_provision_wifi_list wifi = {0};
    char error[CP0_PROVISION_ERROR_MAX + 1] = {0};
    int result = CP0_PROVISION_FAILED;

    if (event == CP0_UI_EVENT_SETUP_RETRY) {
        result = cp0_provision_get_status(&status, error);
        shell->provision_retry_pending = result == CP0_PROVISION_UNAVAILABLE;
        if (result == CP0_PROVISION_OK) {
            shell->provision_retry_pending = false;
            apply_provision_status(shell, &status, true);
        } else if (result == CP0_PROVISION_REPAIR_REQUIRED) {
            shell->provision_retry_pending = false;
            cp0_ui_setup_begin(&shell->ui, CP0_UI_SETUP_REPAIR);
        } else {
            cp0_ui_setup_result(&shell->ui, event, false, error);
        }
        return;
    }
    if (event == CP0_UI_EVENT_SETUP_START) {
        result = cp0_provision_get_status(&status, error);
        if (result == CP0_PROVISION_OK &&
            status.phase == CP0_PROVISION_COMPLETE)
            begin_normal_shell(shell);
        else
            cp0_ui_setup_result(&shell->ui, event, false,
                                error[0] != '\0' ? error
                                                 : "Setup is not complete");
        return;
    }
    switch (event) {
    case CP0_UI_EVENT_SETUP_SET_REGION:
        cp0_ui_setup_set_busy(&shell->ui, "SAVING REGION",
                              "APPLYING DEVICE AND TIME SETTINGS");
        break;
    case CP0_UI_EVENT_SETUP_SET_OWNER:
        cp0_ui_setup_set_busy(&shell->ui, "CREATING OWNER",
                              "PREPARING PRIVATE OWNER STORAGE");
        break;
    case CP0_UI_EVENT_SETUP_SET_PASSWORD:
        cp0_ui_setup_set_busy(&shell->ui, "SECURING PASSWORD",
                              "GENERATING A YESCRYPT PASSWORD HASH");
        break;
    case CP0_UI_EVENT_SETUP_LIST_WIFI:
        cp0_ui_setup_set_busy(&shell->ui, "SCANNING WI-FI",
                              "SEARCHING FOR NEARBY NETWORKS");
        break;
    case CP0_UI_EVENT_SETUP_CONNECT_WIFI:
        cp0_ui_setup_set_busy(&shell->ui, "CONNECTING WI-FI",
                              "AUTHENTICATING AND REQUESTING AN IP");
        break;
    default:
        break;
    }
    if (shell->ui.setup_busy || event == CP0_UI_EVENT_SETUP_COMMIT) {
        shell_redraw(shell);
        wl_display_flush(shell->display);
    }
    switch (event) {
    case CP0_UI_EVENT_SETUP_SET_REGION:
        result = cp0_provision_set_region(
            cp0_ui_setup_locale(&shell->ui),
            cp0_ui_setup_country_code(&shell->ui),
            cp0_ui_setup_timezone_name(&shell->ui), shell->ui.setup_hostname,
            &status, error);
        break;
    case CP0_UI_EVENT_SETUP_SET_OWNER:
        result = cp0_provision_set_owner(
            shell->ui.setup_display_name, shell->ui.setup_username, &status,
            error);
        break;
    case CP0_UI_EVENT_SETUP_SET_PASSWORD:
        result = cp0_provision_set_password(shell->ui.setup_password, &status,
                                            error);
        memset(shell->ui.setup_password_confirm, 0,
               sizeof(shell->ui.setup_password_confirm));
        break;
    case CP0_UI_EVENT_SETUP_LIST_WIFI:
        result = cp0_provision_list_wifi(&wifi, error);
        if (result == CP0_PROVISION_OK) {
            struct cp0_ui_setup_wifi options[CP0_UI_SETUP_WIFI_MAX];
            size_t count = wifi.count;
            if (count > CP0_UI_SETUP_WIFI_MAX)
                count = CP0_UI_SETUP_WIFI_MAX;
            for (size_t index = 0; index < count; index++) {
                options[index] = (struct cp0_ui_setup_wifi){
                    .security = (unsigned int)wifi.networks[index].security,
                    .signal_percent = wifi.networks[index].signal_percent,
                    .connected = wifi.networks[index].connected,
                    .ssid = wifi.networks[index].ssid,
                };
            }
            cp0_ui_setup_set_wifi(&shell->ui, options, count);
            return;
        }
        break;
    case CP0_UI_EVENT_SETUP_CONNECT_WIFI: {
        unsigned int selected = shell->ui.setup_wifi_selected;
        if (selected >= shell->ui.setup_wifi_count) {
            snprintf(error, sizeof(error), "Select a Wi-Fi network");
            break;
        }
        result = cp0_provision_connect_wifi(
            shell->ui.setup_wifi_ssids[selected],
            (enum cp0_provision_wifi_security)
                shell->ui.setup_wifi_security[selected],
            shell->ui.setup_wifi_password, false, &status, error);
        break;
    }
    case CP0_UI_EVENT_SETUP_USE_ETHERNET:
        result = cp0_provision_use_ethernet(&status, error);
        break;
    case CP0_UI_EVENT_SETUP_USE_OFFLINE:
        result = cp0_provision_use_offline(&status, error);
        break;
    case CP0_UI_EVENT_SETUP_SET_SSH:
        result = cp0_provision_set_ssh_enabled(
            shell->ui.setup_ssh_enabled, &status, error);
        break;
    case CP0_UI_EVENT_SETUP_COMMIT:
        result = cp0_provision_commit(&status, error);
        break;
    default:
        return;
    }
    shell->provision_retry_pending = result == CP0_PROVISION_UNAVAILABLE;
    if (result == CP0_PROVISION_OK) {
        /* The daemon's durable phase is authoritative after every mutation. */
        apply_provision_status(shell, &status, true);
        return;
    }
    if ((result == CP0_PROVISION_INVALID_STATE ||
         result == CP0_PROVISION_REPAIR_REQUIRED) &&
        reconcile_provisioning(shell, true))
        return;
    cp0_ui_setup_result(&shell->ui, event, false, error);
}

static void retry_unavailable_provisioning(struct shell *shell)
{
    struct cp0_provision_status status = {0};
    char error[CP0_PROVISION_ERROR_MAX + 1] = {0};

    if (!shell->provision_retry_pending || !shell->ui.setup_active)
        return;
    int result = cp0_provision_get_status(&status, error);
    if (result == CP0_PROVISION_UNAVAILABLE)
        return;
    shell->provision_retry_pending = false;
    if (result == CP0_PROVISION_OK)
        apply_provision_status(shell, &status, true);
    else if (result == CP0_PROVISION_REPAIR_REQUIRED)
        cp0_ui_setup_begin(&shell->ui, CP0_UI_SETUP_REPAIR);
    else
        cp0_ui_setup_result(&shell->ui, CP0_UI_EVENT_SETUP_RETRY, false,
                            error);
}

static void cancel_notification(struct shell *shell, bool restore_mode)
{
    if (!shell->ui.notification_banner)
        return;
    cp0_ui_clear_notification(&shell->ui);
    shell->notification_ticks = 0;
    if (restore_mode &&
        shell->overlay_mode ==
            CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION) {
        shell->overlay_mode = shell->notification_restore_mode;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
    }
}

static void cancel_system_action(struct shell *shell, bool restore_mode)
{
    if (!shell->ui.system_action_overlay)
        return;
    shell->ui.system_action_overlay = false;
    shell->ui.system_action_ticks = 0;
    if (restore_mode &&
        shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION) {
        shell->overlay_mode = shell->system_action_restore_mode;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
    }
}

static void poll_permission_prompt(struct shell *shell)
{
    struct cp0_permission_prompt prompt;
    int result;

    if (shell->ui.document_prompt)
        return;
    result = cp0_appd_get_permission_prompt(&prompt);
    if (shell->ui.permission_prompt && result == 0) {
        cp0_ui_clear_permission(&shell->ui);
        shell->overlay_mode = shell->interrupted_overlay_mode;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
        shell_redraw(shell);
        return;
    }
    if (shell->ui.permission_prompt && result == 1 &&
        prompt.prompt_id == shell->ui.prompt_id)
        return;
    if (result != 1)
        return;
    cancel_notification(shell, true);
    cancel_system_action(shell, true);
    shell->interrupted_overlay_mode = shell->overlay_mode;
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    if (!cp0_ui_show_permission(&shell->ui, prompt.prompt_id, prompt.app_name,
                                prompt.permission, prompt.reason))
        return;
    cp0_system_shell_v1_set_overlay_mode(
        shell->system_control, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL);
    fprintf(stderr, "system-shell: permission prompt=%llu visible\n",
            (unsigned long long)prompt.prompt_id);
    shell_redraw(shell);
}

static void poll_document_prompt(struct shell *shell)
{
    struct cp0_document_prompt prompt;
    struct cp0_ui_document_option documents[CP0_DOCUMENT_MAX];
    int result;

    if (shell->ui.permission_prompt)
        return;
    result = cp0_appd_get_document_prompt(&prompt);
    if (shell->ui.document_prompt && result == 0) {
        cp0_ui_clear_documents(&shell->ui);
        shell->overlay_mode = shell->document_restore_mode;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
        shell_redraw(shell);
        return;
    }
    if (shell->ui.document_prompt && result == 1 &&
        prompt.prompt_id == shell->ui.document_prompt_id)
        return;
    if (result != 1)
        return;
    for (size_t index = 0; index < prompt.document_count; index++) {
        documents[index] = (struct cp0_ui_document_option){
            .size_bytes = prompt.documents[index].size_bytes,
            .document_id = prompt.documents[index].document_id,
            .name = prompt.documents[index].name,
        };
    }
    cancel_notification(shell, true);
    cancel_system_action(shell, true);
    if (!shell->ui.document_prompt)
        shell->document_restore_mode = shell->overlay_mode;
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    if (!cp0_ui_show_documents(&shell->ui, prompt.prompt_id, prompt.app_name,
                               documents, prompt.document_count))
        return;
    cp0_system_shell_v1_set_overlay_mode(
        shell->system_control, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL);
    fprintf(stderr, "system-shell: document prompt=%llu visible\n",
            (unsigned long long)prompt.prompt_id);
    shell_redraw(shell);
}

static void poll_app_catalog(struct shell *shell)
{
    struct cp0_app_list list;
    struct cp0_ui_catalog_app catalog[CP0_APPD_MAX_APPS];

    if (cp0_appd_list_apps(&list) != 0)
        return;
    shell->installed_apps = list;
    shell->has_installed_apps = true;
    for (size_t index = 0; index < list.count; index++) {
        catalog[index] = (struct cp0_ui_catalog_app){
            .running = list.apps[index].running,
            .immersive = list.apps[index].immersive,
            .app_id = list.apps[index].app_id,
            .name = list.apps[index].name,
            .version = list.apps[index].version,
            .permissions = list.apps[index].permissions,
            .installed_at_unix_seconds =
                list.apps[index].installed_at_unix_seconds,
            .package_bytes = list.apps[index].package_bytes,
            .data_bytes = list.apps[index].data_bytes,
        };
    }
    cp0_ui_sync_app_catalog(&shell->ui, catalog, list.count, list.truncated);
}

static void poll_task_catalog(struct shell *shell)
{
    struct cp0_task_list list;
    struct cp0_ui_catalog_task catalog[CP0_APPD_MAX_TASKS];

    if (cp0_appd_list_tasks(&list) != 0)
        return;
    shell->task_list = list;
    for (size_t index = 0; index < list.count; index++) {
        enum cp0_ui_task_state state;
        switch (shell->task_list.tasks[index].state) {
        case CP0_TASK_FOREGROUND:
            state = CP0_UI_TASK_FOREGROUND;
            break;
        case CP0_TASK_BACKGROUND:
            state = CP0_UI_TASK_BACKGROUND;
            break;
        case CP0_TASK_FROZEN:
            state = CP0_UI_TASK_FROZEN;
            break;
        case CP0_TASK_CHECKPOINTED:
            state = CP0_UI_TASK_CHECKPOINTED;
            break;
        case CP0_TASK_CRASHED:
            state = CP0_UI_TASK_CRASHED;
            break;
        default:
            return;
        }
        catalog[index] = (struct cp0_ui_catalog_task){
            .task_id = shell->task_list.tasks[index].task_id,
            .account_uid = shell->task_list.tasks[index].account_uid,
            .created_sequence =
                shell->task_list.tasks[index].created_sequence,
            .last_activated_sequence =
                shell->task_list.tasks[index].last_activated_sequence,
            .runtime_generation =
                shell->task_list.tasks[index].runtime_generation,
            .thumbnail_generation =
                shell->task_list.tasks[index].thumbnail_generation,
            .state = state,
            .immersive = shell->task_list.tasks[index].immersive,
            .checkpoint_available =
                shell->task_list.tasks[index].checkpoint_available,
            .app_id = shell->task_list.tasks[index].app_id,
            .name = shell->task_list.tasks[index].name,
            .version = shell->task_list.tasks[index].version,
        };
    }
    cp0_ui_sync_task_catalog(&shell->ui, catalog, list.count);
}

static void apply_device_settings(
    struct shell *shell, const struct cp0_device_settings *settings)
{
    enum cp0_ui_authority authority;
    switch (settings->authority) {
    case CP0_AUTHORITY_PERSONAL:
        authority = CP0_UI_AUTHORITY_PERSONAL;
        break;
    case CP0_AUTHORITY_PARENT:
        authority = CP0_UI_AUTHORITY_PARENT;
        break;
    case CP0_AUTHORITY_ORGANIZATION:
        authority = CP0_UI_AUTHORITY_ORGANIZATION;
        break;
    default:
        return;
    }
    cp0_ui_set_device_settings(
        &shell->ui, authority, settings->developer_mode,
        settings->developer_mode_allowed, settings->recovery_mode,
        settings->recovery_mode_allowed, settings->store_install_allowed,
        settings->app_launch_restricted, settings->denied_permission_count);
}

static void poll_device_settings(struct shell *shell)
{
    struct cp0_device_settings settings;
    if (cp0_appd_get_device_settings(&settings) == 0)
        apply_device_settings(shell, &settings);
}

static void poll_developer_access(struct shell *shell)
{
    struct cp0_developer_access access;
    struct cp0_ui_developer_host hosts[CP0_DEVELOPER_MAX_HOSTS];
    if (cp0_developer_list(&access) != 0) {
        shell->ui.developer_access_available = false;
        shell->ui.developer_pairing_open = false;
        return;
    }
    for (size_t index = 0; index < access.host_count; index++) {
        hosts[index].label = access.hosts[index].label;
        hosts[index].ssh_fingerprint = access.hosts[index].ssh_fingerprint;
    }
    cp0_ui_set_developer_access(&shell->ui, access.pairing_open, hosts,
                                access.host_count);
}

static void apply_display_state(struct shell *shell,
                                const struct cp0_display_state *state)
{
    cp0_ui_set_display_state(&shell->ui, state->available,
                             state->brightness_percent);
}

static void poll_display_state(struct shell *shell)
{
    struct cp0_display_state state = {0};
    int result = cp0_display_get_state(&state);
    if (result == CP0_DISPLAY_OK)
        apply_display_state(shell, &state);
    else
        cp0_ui_set_display_state(&shell->ui, false, 0);
}

static void apply_audio_output_state(
    struct shell *shell, const struct cp0_audio_output_state *state)
{
    cp0_ui_set_audio_output_state(&shell->ui, state->available,
                                  state->volume_percent, state->muted);
}

static void poll_audio_output_state(struct shell *shell)
{
    struct cp0_audio_output_state state = {0};
    int result = cp0_audio_get_output_state(&state);
    if (result == CP0_AUDIO_SETTINGS_OK)
        apply_audio_output_state(shell, &state);
    else
        cp0_ui_set_audio_output_state(&shell->ui, false, 0, false);
}

static void apply_connectivity_state(
    struct shell *shell, const struct cp0_connectivity_state *state)
{
    cp0_ui_set_connectivity_state(
        &shell->ui, state->available, state->wifi_available,
        state->wifi_enabled, state->airplane_mode);
}

static void poll_connectivity_state(struct shell *shell)
{
    struct cp0_connectivity_state state = {0};
    int result = cp0_connectivity_get_state(&state);
    if (result == CP0_CONNECTIVITY_OK)
        apply_connectivity_state(shell, &state);
    else
        cp0_ui_set_connectivity_state(&shell->ui, false, false, false, false);
}

static unsigned int screen_timeout_seconds(unsigned int selection)
{
    static const unsigned int values[] = {30U, 60U, 300U, 0U};
    return selection < CP0_SHELL_TIMEOUT_COUNT ? values[selection] : 60U;
}

static void load_shell_preferences(struct shell *shell)
{
    struct cp0_shell_settings settings;
    cp0_shell_settings_defaults(&settings);
    if (!cp0_shell_settings_load(CP0_SHELL_SETTINGS_PATH, &settings))
        fprintf(stderr, "system-shell: using default shell preferences\n");
    cp0_ui_set_preferences(&shell->ui, settings.theme,
                           settings.screen_timeout, settings.key_sounds);
}

static void save_shell_preferences(struct shell *shell)
{
    const struct cp0_shell_settings settings = {
        .theme = shell->ui.theme,
        .screen_timeout = shell->ui.screen_timeout,
        .key_sounds = shell->ui.key_sounds,
    };
    if (!cp0_shell_settings_save(CP0_SHELL_SETTINGS_PATH, &settings))
        fprintf(stderr, "system-shell: cannot persist shell preferences\n");
}

static void apply_screen_timeout(struct shell *shell)
{
    unsigned int seconds = screen_timeout_seconds(shell->ui.screen_timeout);
    if (shell->system_control == NULL ||
        wl_proxy_get_version((struct wl_proxy *)shell->system_control) < 6U)
        return;
    cp0_system_shell_v1_set_idle_timeout(shell->system_control, seconds);
    fprintf(stderr, "system-shell: display idle timeout=%u seconds\n", seconds);
}

static void apply_auto_update_status(
    struct shell *shell, const struct cp0_store_auto_update_status *status)
{
    cp0_ui_set_auto_update(
        &shell->ui, true, status->enabled, status->policy_allowed,
        status->charging, status->unmetered_network, status->due,
        status->checking);
}

static void poll_auto_update_status(struct shell *shell)
{
    struct cp0_store_auto_update_status status;
    if (cp0_store_get_auto_update(&status) == CP0_STORE_RESULT_OK)
        apply_auto_update_status(shell, &status);
    else
        cp0_ui_set_auto_update(&shell->ui, false, false, false, false,
                               false, false, false);
}

static void apply_metrics_status(
    struct shell *shell, const struct cp0_store_metrics_status *status)
{
    cp0_ui_set_metrics(&shell->ui, true, status->enabled,
                       status->policy_allowed, status->configured,
                       status->pending);
}

static void poll_metrics_status(struct shell *shell)
{
    struct cp0_store_metrics_status status;
    if (cp0_store_get_metrics(&status) == CP0_STORE_RESULT_OK)
        apply_metrics_status(shell, &status);
    else
        cp0_ui_set_metrics(&shell->ui, false, false, false, false, false);
}

static const struct cp0_app_summary *installed_app(
    const struct shell *shell, const char *app_id)
{
    if (!shell->has_installed_apps)
        return NULL;
    for (size_t index = 0; index < shell->installed_apps.count; index++) {
        if (strcmp(shell->installed_apps.apps[index].app_id, app_id) == 0)
            return &shell->installed_apps.apps[index];
    }
    return NULL;
}

static enum cp0_ui_store_state store_ui_state(
    const struct cp0_store_app_summary *app)
{
    static const enum cp0_ui_store_state direct_states[] = {
        CP0_UI_STORE_AVAILABLE,   CP0_UI_STORE_QUEUED,
        CP0_UI_STORE_DOWNLOADING, CP0_UI_STORE_PAUSED,
        CP0_UI_STORE_INSTALLING,  CP0_UI_STORE_INSTALLED,
        CP0_UI_STORE_CANCELED,    CP0_UI_STORE_FAILED,
    };
    return direct_states[app->state];
}

static enum cp0_ui_store_failure_reason store_ui_failure_reason(
    const struct cp0_store_app_summary *app)
{
    static const enum cp0_ui_store_failure_reason direct_reasons[] = {
        CP0_UI_STORE_FAILURE_NONE,
        CP0_UI_STORE_FAILURE_NETWORK,
        CP0_UI_STORE_FAILURE_STORAGE,
        CP0_UI_STORE_FAILURE_VERIFICATION,
        CP0_UI_STORE_FAILURE_INSTALLER,
        CP0_UI_STORE_FAILURE_CATALOG_CHANGED,
        CP0_UI_STORE_FAILURE_INTERNAL,
    };
    return direct_reasons[app->failure_reason];
}

static struct cp0_ui_store_catalog_app store_ui_catalog_app(
    const struct shell *shell, const struct cp0_store_app_summary *app)
{
    const struct cp0_app_summary *installed =
        installed_app(shell, app->app_id);
    return (struct cp0_ui_store_catalog_app){
        .package_bytes = app->package_bytes,
        .permissions = app->permissions,
        .progress_percent = app->progress_percent,
        .state = store_ui_state(app),
        .failure_reason = store_ui_failure_reason(app),
        .app_id = app->app_id,
        .name = app->name,
        .version = app->version,
        .summary = app->summary,
        .installed_version = installed == NULL ? NULL : installed->version,
        .installed_permissions =
            installed == NULL ? 0 : installed->permissions,
    };
}

static void poll_store_today(struct shell *shell, uint64_t expected_sequence)
{
    struct cp0_store_today today;
    struct cp0_ui_store_catalog_app featured;
    struct cp0_ui_store_catalog_app collection_apps
        [CP0_STORE_EDITORIAL_COLLECTION_MAX]
        [CP0_STORE_EDITORIAL_COLLECTION_APP_MAX];
    struct cp0_ui_store_editorial_collection collections
        [CP0_STORE_EDITORIAL_COLLECTION_MAX];
    struct cp0_ui_store_editorial editorial;

    if (cp0_store_today(&today) != CP0_STORE_RESULT_OK ||
        !today.has_editorial || today.sequence != expected_sequence) {
        cp0_ui_sync_store_today(&shell->ui, NULL);
        return;
    }
    featured = store_ui_catalog_app(shell, &today.featured);
    for (size_t collection = 0; collection < today.collection_count;
         collection++) {
        for (size_t app = 0; app < today.collections[collection].count; app++)
            collection_apps[collection][app] = store_ui_catalog_app(
                shell, &today.collections[collection].apps[app]);
        collections[collection] = (struct cp0_ui_store_editorial_collection){
            .title = today.collections[collection].title,
            .apps = collection_apps[collection],
            .app_count = today.collections[collection].count,
        };
    }
    editorial = (struct cp0_ui_store_editorial){
        .headline = today.headline,
        .featured = &featured,
        .collections = collections,
        .collection_count = today.collection_count,
    };
    cp0_ui_sync_store_today(&shell->ui, &editorial);
}

static void poll_store_browse(struct shell *shell)
{
    struct cp0_store_browse_results results;
    struct cp0_ui_store_catalog_app apps[CP0_STORE_BROWSE_MAX_APPS];
    uint16_t offset = cp0_ui_store_browse_offset(&shell->ui);
    int result = cp0_store_browse(offset, CP0_STORE_BROWSE_MAX_APPS, &results);

    if (result == CP0_STORE_RESULT_UNCONFIGURED) {
        cp0_ui_set_store_browse_status(&shell->ui,
                                       CP0_UI_STORE_UNCONFIGURED);
        return;
    }
    if (result != CP0_STORE_RESULT_OK) {
        if (result != CP0_STORE_RESULT_BUSY)
            cp0_ui_set_store_browse_status(&shell->ui,
                                           CP0_UI_STORE_UNAVAILABLE);
        return;
    }
    if (results.offset > results.total) {
        shell->ui.store_browse_offset =
            results.total == 0
                ? 0
                : (uint16_t)(((results.total - 1U) /
                              CP0_STORE_BROWSE_MAX_APPS) *
                             CP0_STORE_BROWSE_MAX_APPS);
        cp0_ui_set_store_browse_status(&shell->ui, CP0_UI_STORE_LOADING);
        return;
    }
    shell->store_catalog_sequence = results.sequence;
    for (size_t index = 0; index < results.count; index++)
        apps[index] = store_ui_catalog_app(shell, &results.apps[index]);
    cp0_ui_sync_store_browse(
        &shell->ui, results.offset, results.total, results.has_next,
        results.next_offset, apps, results.count, results.stale);
}

static void poll_store_catalog(struct shell *shell)
{
    struct cp0_store_catalog catalog;
    struct cp0_ui_store_catalog_app apps[CP0_STORE_MAX_APPS];
    int result;

    result = cp0_store_list(&catalog);
    if (result == CP0_STORE_RESULT_UNCONFIGURED) {
        cp0_ui_sync_store_today(&shell->ui, NULL);
        cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNCONFIGURED);
        cp0_ui_set_store_browse_status(&shell->ui,
                                       CP0_UI_STORE_UNCONFIGURED);
        return;
    }
    if (result != CP0_STORE_RESULT_OK) {
        cp0_ui_sync_store_today(&shell->ui, NULL);
        cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNAVAILABLE);
        cp0_ui_set_store_browse_status(&shell->ui,
                                       CP0_UI_STORE_UNAVAILABLE);
        return;
    }
    shell->store_catalog_sequence = catalog.sequence;
    for (size_t index = 0; index < catalog.count; index++)
        apps[index] = store_ui_catalog_app(shell, &catalog.apps[index]);
    cp0_ui_sync_store_catalog(&shell->ui, apps, catalog.count,
                              catalog.truncated, catalog.stale);
    poll_store_today(shell, catalog.sequence);
    if (shell->ui.store_section == CP0_UI_STORE_APPS)
        poll_store_browse(shell);
}

static void poll_store_search(struct shell *shell)
{
    struct cp0_store_search_results results;
    struct cp0_ui_store_catalog_app apps[CP0_STORE_SEARCH_MAX_APPS];
    const char *query = cp0_ui_store_search_query(&shell->ui);
    uint16_t offset = cp0_ui_store_search_offset(&shell->ui);
    int result;

    if (query == NULL || query[0] == '\0')
        return;
    result = cp0_store_search(query, offset, CP0_STORE_SEARCH_MAX_APPS,
                              &results);
    if (result == CP0_STORE_RESULT_UNCONFIGURED) {
        cp0_ui_set_store_search_status(&shell->ui,
                                       CP0_UI_STORE_UNCONFIGURED);
        return;
    }
    if (result != CP0_STORE_RESULT_OK) {
        if (result != CP0_STORE_RESULT_BUSY)
            cp0_ui_set_store_search_status(&shell->ui,
                                           CP0_UI_STORE_UNAVAILABLE);
        return;
    }
    for (size_t index = 0; index < results.count; index++)
        apps[index] = store_ui_catalog_app(shell, &results.apps[index]);
    cp0_ui_sync_store_search(
        &shell->ui, results.query, results.offset, results.total,
        results.has_next, results.next_offset, apps, results.count,
        results.stale);
}

static bool selected_store_identity(struct shell *shell,
                                    char app_id[CP0_STORE_APP_ID_BYTES],
                                    char version[CP0_STORE_VERSION_BYTES])
{
    const char *selected_id = cp0_ui_selected_store_app_id(&shell->ui);
    const char *selected_version =
        cp0_ui_selected_store_app_version(&shell->ui);
    return selected_id != NULL && selected_version != NULL &&
           snprintf(app_id, CP0_STORE_APP_ID_BYTES, "%s", selected_id) > 0 &&
           snprintf(version, CP0_STORE_VERSION_BYTES, "%s", selected_version) >
               0;
}

static void load_store_details(struct shell *shell)
{
    static const char *categories[] = {
        "DEVELOPER TOOLS", "EDUCATION", "ENTERTAINMENT", "GAMES",
        "HARDWARE",        "MEDIA",     "PRODUCTIVITY",  "UTILITIES",
    };
    static const char *ratings[] = {"4+", "9+", "12+", "17+"};
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    struct cp0_store_app_details details;
    struct cp0_store_image_metadata metadata;

    if (!selected_store_identity(shell, app_id, version))
        return;
    int result = cp0_store_get_details(app_id, version, &details);
    if (result == CP0_STORE_RESULT_OK &&
        details.category <= CP0_STORE_CATEGORY_UTILITIES &&
        details.age_rating <= CP0_STORE_AGE_17_PLUS) {
        cp0_ui_set_store_details(
            &shell->ui, app_id, version, details.developer,
            categories[details.category], ratings[details.age_rating],
            details.description, details.release_notes,
            details.screenshot_count);
    } else {
        cp0_ui_set_store_details_unavailable(&shell->ui, app_id, version);
    }
    if (cp0_store_get_icon(app_id, version, shell->store_icon_pixels,
                           CP0_STORE_ICON_MAX_PIXELS, &metadata) ==
        CP0_STORE_RESULT_OK)
        cp0_ui_set_store_icon(&shell->ui, app_id, version,
                              shell->store_icon_pixels,
                              metadata.width, metadata.height);
}

static void load_store_screenshot(struct shell *shell)
{
    char app_id[CP0_STORE_APP_ID_BYTES];
    char version[CP0_STORE_VERSION_BYTES];
    uint8_t index = cp0_ui_selected_store_screenshot(&shell->ui);
    struct cp0_store_image_metadata metadata;

    if (!selected_store_identity(shell, app_id, version)) {
        return;
    }
    if (cp0_store_get_screenshot(app_id, version, index,
                                 shell->store_screenshot_pixels,
                                 CP0_STORE_SCREENSHOT_PIXELS, &metadata) ==
            CP0_STORE_RESULT_OK) {
        cp0_ui_set_store_screenshot(
            &shell->ui, app_id, version, index,
            shell->store_screenshot_pixels, metadata.width, metadata.height);
    } else {
        cp0_ui_set_store_screenshot_unavailable(&shell->ui, app_id, version,
                                                index);
    }
}

static void poll_notification(struct shell *shell)
{
    struct cp0_notification notification;

    if (shell->ui.permission_prompt || shell->ui.document_prompt ||
        shell->ui.power_dialog || shell->ui.settings_confirm ||
        shell->ui.notification_banner || shell->ui.system_action_overlay ||
        cp0_appd_take_notification(&notification) != 1)
        return;
    if (!cp0_ui_show_notification(&shell->ui, notification.notification_id,
                                  notification.app_name, notification.title,
                                  notification.body))
        return;
    shell->notification_restore_mode = shell->overlay_mode;
    shell->notification_ticks = 4;
    if (shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS ||
        shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN) {
        shell->overlay_mode =
            CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
    }
    fprintf(stderr, "system-shell: notification=%llu visible\n",
            (unsigned long long)notification.notification_id);
    shell_redraw(shell);
}

static void show_store_completion_notification(struct shell *shell)
{
    struct cp0_ui_store_completion completion;
    char title[CP0_UI_NOTIFICATION_TITLE_MAX + 1];
    char body[CP0_UI_NOTIFICATION_BODY_MAX + 1];
    const char *app_name;

    if (shell->ui.permission_prompt || shell->ui.document_prompt ||
        shell->ui.power_dialog || shell->ui.settings_confirm ||
        shell->ui.store_install_prompt || shell->ui.notification_banner ||
        shell->ui.system_action_overlay)
        return;
    if (!cp0_ui_take_store_completion(&shell->ui, &completion))
        return;
    if (completion.count == 1) {
        app_name = completion.app_name;
        snprintf(title, sizeof(title), "INSTALL COMPLETE");
        snprintf(body, sizeof(body), "VERSION %s IS READY", completion.version);
    } else {
        app_name = "App Store";
        snprintf(title, sizeof(title), "%u UPDATES INSTALLED",
                 (unsigned int)completion.count);
        snprintf(body, sizeof(body), "ALL UPDATES ARE READY");
    }
    uint64_t notification_id = UINT64_MAX - shell->store_notification_serial++;
    if (!cp0_ui_show_notification(&shell->ui, notification_id, app_name, title,
                                  body))
        return;
    shell->notification_restore_mode = shell->overlay_mode;
    shell->notification_ticks = 4;
    if (shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS ||
        shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN) {
        shell->overlay_mode =
            CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
    }
    fprintf(stderr, "system-shell: store completion notification=%llu visible\n",
            (unsigned long long)notification_id);
}

static void update_notification_timer(struct shell *shell)
{
    if (!shell->ui.notification_banner || shell->notification_ticks == 0)
        return;
    shell->notification_ticks--;
    if (shell->notification_ticks == 0) {
        uint64_t notification_id = shell->ui.notification_id;
        cancel_notification(shell, true);
        fprintf(stderr, "system-shell: notification=%llu expired\n",
                (unsigned long long)notification_id);
    }
}

static int create_anonymous_file(size_t size)
{
    char name[64];
    int fd = -1;

    for (int attempt = 0; attempt < 100; attempt++) {
        snprintf(name, sizeof(name), "/cp0-shell-%ld-%u", (long)getpid(),
                 shm_serial++);
        fd = shm_open(name, O_RDWR | O_CREAT | O_EXCL, 0600);
        if (fd >= 0) {
            shm_unlink(name);
            break;
        }
        if (errno != EEXIST)
            return -1;
    }
    if (fd < 0)
        return -1;
    if (ftruncate(fd, (off_t)size) < 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static void destroy_buffer(struct shell_buffer *buffer)
{
    if (buffer->wl_buffer != NULL)
        wl_buffer_destroy(buffer->wl_buffer);
    if (buffer->pixels != NULL)
        munmap(buffer->pixels, buffer->size);
    buffer->wl_buffer = NULL;
    buffer->pixels = NULL;
    buffer->size = 0;
    buffer->width = 0;
    buffer->height = 0;
    buffer->busy = false;
}

static void handle_buffer_release(void *data, struct wl_buffer *wl_buffer)
{
    struct shell_buffer *buffer = data;
    (void)wl_buffer;
    buffer->busy = false;
    if (buffer->shell->redraw_pending)
        shell_redraw(buffer->shell);
}

static const struct wl_buffer_listener buffer_listener = {
    .release = handle_buffer_release,
};

static bool create_buffer(struct shell *shell, struct shell_buffer *buffer,
                          int width, int height)
{
    int stride = width * 4;
    size_t size = (size_t)stride * (size_t)height;
    int fd = create_anonymous_file(size);
    if (fd < 0) {
        fprintf(stderr, "system-shell: cannot create SHM file: %s\n",
                strerror(errno));
        return false;
    }

    void *data = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (data == MAP_FAILED) {
        fprintf(stderr, "system-shell: cannot map SHM buffer: %s\n",
                strerror(errno));
        close(fd);
        return false;
    }

    struct wl_shm_pool *pool = wl_shm_create_pool(shell->shm, fd, (int)size);
    struct wl_buffer *wl_buffer = wl_shm_pool_create_buffer(
        pool, 0, width, height, stride, WL_SHM_FORMAT_ARGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);

    buffer->shell = shell;
    buffer->wl_buffer = wl_buffer;
    buffer->pixels = data;
    buffer->size = size;
    buffer->width = width;
    buffer->height = height;
    buffer->busy = false;
    wl_buffer_add_listener(wl_buffer, &buffer_listener, buffer);
    return true;
}

static void destroy_screenshot_buffer(struct screenshot_buffer *buffer)
{
    if (buffer->wl_buffer != NULL)
        wl_buffer_destroy(buffer->wl_buffer);
    if (buffer->pixels != NULL)
        munmap(buffer->pixels, buffer->size);
    memset(buffer, 0, sizeof(*buffer));
}

static uint32_t screenshot_base_overlay_mode(const struct shell *shell)
{
    if (shell->overlay_mode !=
        CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION)
        return shell->overlay_mode;
    if (shell->ui.notification_banner)
        return shell->notification_restore_mode;
    if (shell->ui.system_action_overlay)
        return shell->system_action_restore_mode;
    return CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
}

static void show_screenshot_status(struct shell *shell,
                                   enum cp0_ui_screenshot_status status)
{
    cancel_notification(shell, false);
    cancel_system_action(shell, false);
    shell->system_action_restore_mode = shell->screenshot_restore_mode;
    cp0_ui_set_screenshot_status(&shell->ui, status);
    if (shell->screenshot_restore_mode ==
        CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL) {
        shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    } else {
        shell->overlay_mode =
            CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION;
        cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                              shell->overlay_mode);
    }
    shell_redraw(shell);
}

static bool create_screenshot_buffer(struct shell *shell)
{
    struct screenshot_buffer *buffer = &shell->screenshot_buffer;
    const int stride = (int)CP0_SCREENSHOT_WIDTH * 4;
    const size_t size = (size_t)stride * CP0_SCREENSHOT_HEIGHT;
    struct wl_shm_pool *pool;
    uint32_t shm_format;
    int fd;

    if (shell->capture_format == DRM_FORMAT_XRGB8888)
        shm_format = WL_SHM_FORMAT_XRGB8888;
    else if (shell->capture_format == DRM_FORMAT_ARGB8888)
        shm_format = WL_SHM_FORMAT_ARGB8888;
    else
        return false;
    destroy_screenshot_buffer(buffer);
    fd = create_anonymous_file(size);
    if (fd < 0)
        return false;
    buffer->pixels = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd,
                          0);
    if (buffer->pixels == MAP_FAILED) {
        buffer->pixels = NULL;
        close(fd);
        return false;
    }
    pool = wl_shm_create_pool(shell->shm, fd, (int)size);
    buffer->wl_buffer = wl_shm_pool_create_buffer(
        pool, 0, (int)CP0_SCREENSHOT_WIDTH, (int)CP0_SCREENSHOT_HEIGHT,
        stride, shm_format);
    wl_shm_pool_destroy(pool);
    close(fd);
    if (buffer->wl_buffer == NULL) {
        munmap(buffer->pixels, size);
        buffer->pixels = NULL;
        return false;
    }
    buffer->size = size;
    return true;
}

static bool issue_screenshot_capture(struct shell *shell)
{
    if (shell->capture_source == NULL || shell->capture_width !=
            (int32_t)CP0_SCREENSHOT_WIDTH ||
        shell->capture_height != (int32_t)CP0_SCREENSHOT_HEIGHT ||
        (shell->capture_format != DRM_FORMAT_XRGB8888 &&
         shell->capture_format != DRM_FORMAT_ARGB8888) ||
        !create_screenshot_buffer(shell))
        return false;
    weston_capture_source_v1_capture(shell->capture_source,
                                     shell->screenshot_buffer.wl_buffer);
    shell->capture_busy = true;
    return true;
}

static void finish_screenshot_capture(struct shell *shell, bool saved)
{
    shell->capture_busy = false;
    destroy_screenshot_buffer(&shell->screenshot_buffer);
    show_screenshot_status(shell, saved ? CP0_UI_SCREENSHOT_SAVED
                                       : CP0_UI_SCREENSHOT_FAILED);
}

static void handle_capture_format(void *data,
                                  struct weston_capture_source_v1 *source,
                                  uint32_t format)
{
    struct shell *shell = data;
    (void)source;
    shell->capture_format = format;
}

static void handle_capture_size(void *data,
                                struct weston_capture_source_v1 *source,
                                int32_t width, int32_t height)
{
    struct shell *shell = data;
    (void)source;
    shell->capture_width = width;
    shell->capture_height = height;
}

static void handle_capture_complete(void *data,
                                    struct weston_capture_source_v1 *source)
{
    struct shell *shell = data;
    char name[CP0_SCREENSHOT_NAME_MAX];
    int result;
    (void)source;

    result = cp0_screenshot_store_save(
        CP0_SCREENSHOT_DIRECTORY, shell->screenshot_buffer.pixels,
        CP0_SCREENSHOT_WIDTH * CP0_SCREENSHOT_HEIGHT, name);
    if (result == 0)
        fprintf(stderr, "system-shell: screenshot saved as %s\n", name);
    else
        fprintf(stderr, "system-shell: screenshot save failed: %s\n",
                strerror(errno));
    finish_screenshot_capture(shell, result == 0);
}

static void handle_capture_retry(void *data,
                                 struct weston_capture_source_v1 *source)
{
    struct shell *shell = data;
    (void)source;

    shell->capture_busy = false;
    destroy_screenshot_buffer(&shell->screenshot_buffer);
    shell->capture_retries++;
    if (shell->capture_retries > SCREENSHOT_RETRY_MAX ||
        !issue_screenshot_capture(shell)) {
        fprintf(stderr, "system-shell: screenshot retry failed\n");
        finish_screenshot_capture(shell, false);
    }
}

static void handle_capture_failed(void *data,
                                  struct weston_capture_source_v1 *source,
                                  const char *message)
{
    struct shell *shell = data;
    (void)source;
    fprintf(stderr, "system-shell: screenshot capture failed: %s\n",
            message == NULL ? "unknown error" : message);
    finish_screenshot_capture(shell, false);
}

static const struct weston_capture_source_v1_listener capture_listener = {
    .format = handle_capture_format,
    .size = handle_capture_size,
    .complete = handle_capture_complete,
    .retry = handle_capture_retry,
    .failed = handle_capture_failed,
};

static void maybe_create_capture_source(struct shell *shell)
{
    if (shell->capture_source != NULL || shell->capture_factory == NULL ||
        shell->capture_output == NULL)
        return;
    shell->capture_source = weston_capture_v1_create(
        shell->capture_factory, shell->capture_output,
        WESTON_CAPTURE_V1_SOURCE_FRAMEBUFFER);
    if (shell->capture_source != NULL) {
        shell->capture_format = 0;
        shell->capture_width = 0;
        shell->capture_height = 0;
        weston_capture_source_v1_add_listener(shell->capture_source,
                                               &capture_listener, shell);
    }
}

static void begin_screenshot(struct shell *shell)
{
    if (shell->capture_busy) {
        fprintf(stderr, "system-shell: screenshot capture already in progress\n");
        return;
    }
    shell->screenshot_restore_mode = screenshot_base_overlay_mode(shell);
    shell->capture_retries = 0;
    if (shell->capture_source == NULL || shell->capture_width == 0 ||
        shell->capture_height == 0 || shell->capture_format == 0) {
        fprintf(stderr, "system-shell: screenshot capture unavailable\n");
        show_screenshot_status(shell, CP0_UI_SCREENSHOT_UNAVAILABLE);
        return;
    }
    if (!issue_screenshot_capture(shell)) {
        fprintf(stderr,
                "system-shell: screenshot capture contract unsupported\n");
        show_screenshot_status(shell, CP0_UI_SCREENSHOT_UNAVAILABLE);
    }
}

static void update_status(struct shell *shell)
{
    char clock_text[6] = "--:--";
    struct cp0_system_info info;
    struct cp0_ui_device_info device;
    struct cp0_ui_network_info network;
    time_t now = time(NULL);
    struct tm local_time;
    if (localtime_r(&now, &local_time) != NULL)
        strftime(clock_text, sizeof(clock_text), "%H:%M", &local_time);
    cp0_system_info_collect(&info);
    device = (struct cp0_ui_device_info){
        .available = info.device_available,
        .battery_percent = info.battery_percent,
        .temperature_millicelsius = info.temperature_millicelsius,
        .battery_present = info.battery_present,
        .battery_voltage_available = info.battery_voltage_available,
        .battery_current_available = info.battery_current_available,
        .battery_voltage_microvolts = info.battery_voltage_microvolts,
        .battery_current_microamps = info.battery_current_microamps,
        .battery_status = info.battery_status,
        .i2c_bus_state = info.i2c_bus_state,
        .display_state = info.display_state,
        .keyboard_state = info.keyboard_state,
        .audio_state = info.audio_state,
        .camera_state = info.camera_state,
        .uptime_seconds = info.uptime_seconds,
        .memory_total_bytes = info.memory_total_bytes,
        .memory_available_bytes = info.memory_available_bytes,
        .storage_total_bytes = info.storage_total_bytes,
        .storage_available_bytes = info.storage_available_bytes,
        .model = info.model,
        .os_version = info.os_version,
    };
    network = (struct cp0_ui_network_info){
        .available = info.network_available,
        .online = info.network_online,
        .link_up = info.network_link_up,
        .interface_name = info.network_interface,
        .ipv4_address = info.network_ipv4,
    };
    cp0_ui_set_status(&shell->ui, clock_text, info.network_online,
                      info.battery_percent);
    cp0_ui_set_device_info(&shell->ui, &device);
    cp0_ui_set_network_info(&shell->ui, &network);
}

static struct shell_buffer *next_buffer(struct shell *shell)
{
    for (size_t index = 0; index < BUFFER_COUNT; index++) {
        struct shell_buffer *buffer = &shell->buffers[index];
        if (buffer->busy)
            continue;
        if (buffer->wl_buffer != NULL &&
            (buffer->width != shell->width || buffer->height != shell->height))
            destroy_buffer(buffer);
        if (buffer->wl_buffer == NULL &&
            !create_buffer(shell, buffer, shell->width, shell->height))
            return NULL;
        return buffer;
    }
    return NULL;
}

static void shell_redraw(struct shell *shell)
{
    if (!shell->configured)
        return;
    struct shell_buffer *buffer = next_buffer(shell);
    if (buffer == NULL) {
        shell->redraw_pending = true;
        return;
    }

    update_status(shell);
    cp0_ui_render(&shell->ui, buffer->pixels, shell->width, shell->height,
                  shell->width);
    for (int y = 0; y < shell->height; y++) {
        for (int x = 0; x < shell->width; x++) {
            uint32_t *pixel = &buffer->pixels[y * shell->width + x];
            if (shell->overlay_mode ==
                    CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS &&
                y >= 21) {
                *pixel = 0;
            } else if (shell->overlay_mode ==
                           CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION &&
                       y >= CP0_UI_NOTIFICATION_BOTTOM) {
                *pixel = 0;
            } else {
                *pixel |= 0xff000000u;
            }
        }
    }

    struct wl_region *input_region =
        wl_compositor_create_region(shell->compositor);
    if (input_region != NULL) {
        if (shell->overlay_mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL)
            wl_region_add(input_region, 0, 0, shell->width, shell->height);
        else if (shell->overlay_mode ==
                 CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS)
            wl_region_add(input_region, 0, 0, shell->width, 21);
        wl_surface_set_input_region(shell->surface, input_region);
        wl_region_destroy(input_region);
    }
    wl_surface_attach(shell->surface, buffer->wl_buffer, 0, 0);
    wl_surface_damage(shell->surface, 0, 0, shell->width, shell->height);
    wl_surface_commit(shell->surface);
    buffer->busy = true;
    shell->redraw_pending = false;
}

static const struct cp0_ui_store_app *store_ui_app(
    const struct cp0_ui *ui, const char *app_id)
{
    for (unsigned int index = 0; index < ui->store_count; index++)
        if (strcmp(ui->store_apps[index].app_id, app_id) == 0)
            return &ui->store_apps[index];
    unsigned int page_count = ui->store_section == CP0_UI_STORE_APPS
                                  ? ui->store_browse_count
                                  : ui->store_search_count;
    for (unsigned int index = 0; index < page_count; index++)
        if (strcmp(ui->store_page_apps[index].app_id, app_id) == 0)
            return &ui->store_page_apps[index];
    return NULL;
}

static uint8_t permission_count(uint16_t permissions)
{
    uint8_t count = 0;
    for (unsigned int bit = 0; bit < 8; bit++)
        count += (permissions & (1U << bit)) != 0;
    return count;
}

static void show_store_preflight_error(struct shell *shell, int result)
{
    enum cp0_ui_store_preflight_error error = CP0_UI_STORE_PREFLIGHT_UNAVAILABLE;
    if (result == CP0_STORE_RESULT_POLICY_RESTRICTED)
        error = CP0_UI_STORE_PREFLIGHT_POLICY;
    else if (result == CP0_STORE_RESULT_INSUFFICIENT_STORAGE)
        error = CP0_UI_STORE_PREFLIGHT_STORAGE;
    else if (result == CP0_STORE_RESULT_CATALOG_CHANGED)
        error = CP0_UI_STORE_PREFLIGHT_CATALOG;
    cp0_ui_show_store_preflight_error(&shell->ui, error);
}

static int submit_store_preflight(struct shell *shell)
{
    const struct cp0_store_install_preflight *preflight =
        &shell->store_preflight;
    const char *app_ids[CP0_STORE_INSTALL_BATCH_MAX];
    if (preflight->count == 0 ||
        preflight->count > CP0_STORE_INSTALL_BATCH_MAX ||
        preflight->authorization_id == 0)
        return CP0_STORE_RESULT_ERROR;
    for (size_t index = 0; index < preflight->count; index++)
        app_ids[index] = preflight->apps[index].app_id;
    int result = preflight->count == 1
                     ? cp0_store_install(app_ids[0],
                                         preflight->authorization_id)
                     : cp0_store_install_batch(app_ids, preflight->count,
                                               preflight->authorization_id);
    if (result == CP0_STORE_RESULT_OK) {
        for (size_t index = 0; index < preflight->count; index++)
            cp0_ui_set_store_app_state(&shell->ui, app_ids[index],
                                       CP0_UI_STORE_QUEUED, 0);
        shell->store_poll_delay = 1;
        fprintf(stderr, "system-shell: authorized Store install for %zu apps\n",
                preflight->count);
    } else if (result != CP0_STORE_RESULT_BUSY) {
        show_store_preflight_error(shell, result);
        fprintf(stderr,
                "system-shell: authorized Store install failed for %zu apps\n",
                preflight->count);
    }
    if (result != CP0_STORE_RESULT_BUSY)
        memset(&shell->store_preflight, 0, sizeof(shell->store_preflight));
    return result;
}

static void begin_store_preflight(struct shell *shell,
                                  const char *const app_ids[],
                                  size_t app_count)
{
    struct cp0_store_install_preflight preflight;
    uint16_t new_permissions = 0;
    uint16_t denied_permissions = 0;
    int result = cp0_store_preflight_install(
        shell->store_catalog_sequence, app_ids, app_count, &preflight);
    if (result != CP0_STORE_RESULT_OK) {
        show_store_preflight_error(shell, result);
        return;
    }
    for (size_t index = 0; index < preflight.count; index++) {
        const struct cp0_ui_store_app *app =
            store_ui_app(&shell->ui, preflight.apps[index].app_id);
        if (app == NULL || strcmp(app->version, preflight.apps[index].version) != 0 ||
            app->permissions != preflight.apps[index].permissions) {
            cp0_ui_show_store_preflight_error(&shell->ui,
                                              CP0_UI_STORE_PREFLIGHT_CATALOG);
            return;
        }
        new_permissions |=
            preflight.apps[index].permissions & ~app->installed_permissions;
        denied_permissions |= preflight.apps[index].policy_denied_permissions;
    }
    shell->store_preflight = preflight;
    if (new_permissions == 0 && denied_permissions == 0) {
        (void)submit_store_preflight(shell);
        return;
    }
    cp0_ui_show_store_install_prompt(
        &shell->ui, (uint8_t)preflight.count,
        permission_count(new_permissions), permission_count(denied_permissions),
        preflight.required_bytes, preflight.available_bytes);
}

static void handle_ui_action(struct shell *shell, enum cp0_ui_action action)
{
    enum cp0_ui_screen previous_screen = shell->ui.screen;
    if (action == CP0_UI_SHOW_TASKS)
        poll_task_catalog(shell);
    enum cp0_ui_event event = cp0_ui_handle_action(&shell->ui, action);
    if (event >= CP0_UI_EVENT_SETUP_SET_REGION &&
        event <= CP0_UI_EVENT_SETUP_START) {
        handle_setup_event(shell, event);
    } else if (event == CP0_UI_EVENT_PERMISSION_ONCE ||
        event == CP0_UI_EVENT_PERMISSION_ALWAYS ||
        event == CP0_UI_EVENT_PERMISSION_DENY) {
        static const enum cp0_permission_choice choices[] = {
            CP0_PERMISSION_ALLOW_ONCE,
            CP0_PERMISSION_ALLOW_ALWAYS,
            CP0_PERMISSION_DENY,
        };
        unsigned int choice =
            event == CP0_UI_EVENT_PERMISSION_ONCE
                ? 0
                : (event == CP0_UI_EVENT_PERMISSION_ALWAYS ? 1 : 2);
        uint64_t prompt_id = shell->ui.prompt_id;
        if (cp0_appd_resolve_permission(prompt_id, choices[choice]) == 0) {
            cp0_ui_clear_permission(&shell->ui);
            shell->overlay_mode = shell->interrupted_overlay_mode;
            cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                                  shell->overlay_mode);
            fprintf(stderr, "system-shell: permission prompt=%llu resolved\n",
                    (unsigned long long)prompt_id);
        } else {
            fprintf(stderr,
                    "system-shell: permission prompt=%llu resolution failed\n",
                    (unsigned long long)prompt_id);
        }
    } else if (event == CP0_UI_EVENT_DOCUMENT_SELECT ||
               event == CP0_UI_EVENT_DOCUMENT_CANCEL) {
        uint64_t prompt_id = shell->ui.document_prompt_id;
        const char *document_id =
            event == CP0_UI_EVENT_DOCUMENT_SELECT
                ? cp0_ui_selected_document_id(&shell->ui)
                : NULL;
        if (cp0_appd_resolve_document(prompt_id, document_id) == 0) {
            cp0_ui_clear_documents(&shell->ui);
            shell->overlay_mode = shell->document_restore_mode;
            cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                                  shell->overlay_mode);
            fprintf(stderr, "system-shell: document prompt=%llu resolved\n",
                    (unsigned long long)prompt_id);
        } else {
            fprintf(stderr,
                    "system-shell: document prompt=%llu resolution failed\n",
                    (unsigned long long)prompt_id);
        }
    } else if (event == CP0_UI_EVENT_SLEEP) {
        shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
        cp0_system_shell_v1_sleep_display(shell->system_control);
    } else if (event == CP0_UI_EVENT_RESTART) {
        if (cp0_power_request(CP0_POWER_RESTART) != 0)
            fprintf(stderr, "system-shell: restart request failed\n");
    } else if (event == CP0_UI_EVENT_POWER_OFF) {
        if (cp0_power_request(CP0_POWER_OFF) != 0)
            fprintf(stderr, "system-shell: power off request failed\n");
    } else if (event == CP0_UI_EVENT_OPEN_APP) {
        char app_id[CP0_APP_ID_BYTES];
        const char *selected_id = cp0_ui_selected_app_id(&shell->ui);
        uint32_t token = cp0_ui_selected_app_token(&shell->ui);
        if (token != 0) {
            shell->overlay_mode = cp0_ui_selected_app_is_immersive(&shell->ui)
                                      ? CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN
                                      : CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS;
            cp0_system_shell_v1_activate_app(shell->system_control, token);
        } else if (selected_id != NULL &&
                   snprintf(app_id, sizeof(app_id), "%s", selected_id) > 0) {
            if (cp0_ui_selected_app_state(&shell->ui) == CP0_UI_APP_RUNNING ||
                cp0_ui_selected_app_state(&shell->ui) == CP0_UI_APP_STARTING) {
                cp0_ui_set_app_state(&shell->ui, app_id,
                                     CP0_UI_APP_STARTING);
                snprintf(shell->pending_activation,
                         sizeof(shell->pending_activation), "%s", app_id);
                shell->pending_activation_uid = 0;
            } else {
                cp0_ui_set_app_state(&shell->ui, app_id,
                                     CP0_UI_APP_STARTING);
                snprintf(shell->pending_activation,
                         sizeof(shell->pending_activation), "%s", app_id);
                shell->pending_activation_uid = 0;
                shell_redraw(shell);
                wl_display_flush(shell->display);
                if (cp0_appd_start_app(app_id) != 0) {
                    shell->pending_activation[0] = '\0';
                    shell->pending_activation_uid = 0;
                    cp0_ui_set_app_state(&shell->ui, app_id,
                                         CP0_UI_APP_FAILED);
                    fprintf(stderr,
                            "system-shell: application %s start failed\n",
                            app_id);
                } else {
                    fprintf(stderr,
                            "system-shell: application %s start requested\n",
                            app_id);
                }
            }
        }
    } else if (event == CP0_UI_EVENT_ACTIVATE_TASK) {
        uint64_t task_id = cp0_ui_selected_task_id(&shell->ui);
        uint32_t account_uid =
            cp0_ui_selected_task_account_uid(&shell->ui);
        const char *selected_id = cp0_ui_selected_task_app_id(&shell->ui);
        char app_id[CP0_APP_ID_BYTES] = {0};
        bool immersive = cp0_ui_selected_task_is_immersive(&shell->ui);
        uint64_t runtime_generation = 0;
        if (task_id != 0 && selected_id != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected_id) > 0 &&
            cp0_appd_activate_task(task_id, &runtime_generation) == 0) {
            uint32_t token = token_for_account_uid(shell, account_uid);
            shell->overlay_mode = immersive
                                      ? CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN
                                      : CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS;
            if (token != 0) {
                shell->pending_activation[0] = '\0';
                shell->pending_activation_uid = 0;
                cp0_system_shell_v1_activate_app(shell->system_control, token);
            } else {
                snprintf(shell->pending_activation,
                         sizeof(shell->pending_activation), "%s", app_id);
                shell->pending_activation_uid = account_uid;
            }
            poll_task_catalog(shell);
            fprintf(stderr,
                    "system-shell: task=%llu runtime=%llu activation requested\n",
                    (unsigned long long)task_id,
                    (unsigned long long)runtime_generation);
        } else {
            fprintf(stderr, "system-shell: task activation failed\n");
            poll_task_catalog(shell);
        }
    } else if (event == CP0_UI_EVENT_CLOSE_TASK) {
        uint64_t task_id = cp0_ui_selected_task_id(&shell->ui);
        const char *selected_id = cp0_ui_selected_task_app_id(&shell->ui);
        char app_id[CP0_APP_ID_BYTES] = {0};
        if (selected_id != NULL)
            snprintf(app_id, sizeof(app_id), "%s", selected_id);
        if (task_id != 0 && cp0_appd_close_task(task_id) == 0) {
            if (app_id[0] != '\0' &&
                strcmp(shell->pending_activation, app_id) == 0) {
                shell->pending_activation[0] = '\0';
                shell->pending_activation_uid = 0;
            }
            poll_task_catalog(shell);
            poll_app_catalog(shell);
            fprintf(stderr, "system-shell: task=%llu closed\n",
                    (unsigned long long)task_id);
        } else {
            fprintf(stderr, "system-shell: task close failed\n");
            poll_task_catalog(shell);
        }
    } else if (event == CP0_UI_EVENT_STOP_APP) {
        char app_id[CP0_APP_ID_BYTES];
        const char *selected_id = cp0_ui_selected_app_id(&shell->ui);
        if (selected_id != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected_id) > 0) {
            if (cp0_appd_stop_app(app_id) == 0) {
                if (strcmp(shell->pending_activation, app_id) == 0) {
                    shell->pending_activation[0] = '\0';
                    shell->pending_activation_uid = 0;
                }
                cp0_ui_set_app_state(&shell->ui, app_id,
                                     CP0_UI_APP_STOPPED);
                shell->overlay_mode =
                    CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
                cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                                      shell->overlay_mode);
                fprintf(stderr,
                        "system-shell: application %s stop requested\n",
                        app_id);
            } else {
                fprintf(stderr,
                        "system-shell: application %s stop failed\n", app_id);
            }
        }
    } else if (event == CP0_UI_EVENT_UNINSTALL_APP) {
        char app_id[CP0_APP_ID_BYTES];
        const char *selected_id = cp0_ui_selected_app_id(&shell->ui);
        if (selected_id != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected_id) > 0) {
            if (cp0_appd_uninstall_app(app_id) == 0) {
                shell->ui.app_detail = false;
                poll_app_catalog(shell);
                fprintf(stderr, "system-shell: application %s uninstalled; private data retained\n",
                        app_id);
            } else {
                fprintf(stderr, "system-shell: application %s uninstall failed\n",
                        app_id);
            }
        }
    } else if (event == CP0_UI_EVENT_WIFI_ENABLE ||
               event == CP0_UI_EVENT_WIFI_DISABLE ||
               event == CP0_UI_EVENT_AIRPLANE_ENABLE ||
               event == CP0_UI_EVENT_AIRPLANE_DISABLE) {
        struct cp0_connectivity_state state = {0};
        bool airplane = event == CP0_UI_EVENT_AIRPLANE_ENABLE ||
                        event == CP0_UI_EVENT_AIRPLANE_DISABLE;
        bool enabled = event == CP0_UI_EVENT_WIFI_ENABLE ||
                       event == CP0_UI_EVENT_AIRPLANE_ENABLE;
        int result = airplane
                         ? cp0_connectivity_set_airplane_mode(enabled, &state)
                         : cp0_connectivity_set_wifi_enabled(enabled, &state);
        if (result == CP0_CONNECTIVITY_OK) {
            apply_connectivity_state(shell, &state);
            fprintf(stderr,
                    "system-shell: %s changed to %s\n",
                    airplane ? "airplane mode" : "Wi-Fi",
                    enabled ? "enabled" : "disabled");
        } else {
            fprintf(stderr, "system-shell: connectivity update failed\n");
            poll_connectivity_state(shell);
        }
    } else if (event == CP0_UI_EVENT_BRIGHTNESS_DOWN ||
               event == CP0_UI_EVENT_BRIGHTNESS_UP) {
        struct cp0_display_state state = {0};
        enum cp0_display_direction direction =
            event == CP0_UI_EVENT_BRIGHTNESS_DOWN ? CP0_DISPLAY_DECREASE
                                                  : CP0_DISPLAY_INCREASE;
        int result = cp0_display_adjust_brightness(direction, &state);
        if (result == CP0_DISPLAY_OK) {
            apply_display_state(shell, &state);
            fprintf(stderr, "system-shell: brightness changed to %u%%\n",
                    state.brightness_percent);
        } else {
            cp0_ui_set_display_state(&shell->ui, false, 0);
            fprintf(stderr, "system-shell: brightness control unavailable\n");
        }
    } else if (event == CP0_UI_EVENT_THEME_PREVIOUS ||
               event == CP0_UI_EVENT_THEME_NEXT) {
        shell->ui.theme =
            (shell->ui.theme +
             (event == CP0_UI_EVENT_THEME_PREVIOUS ? 2U : 1U)) %
            CP0_SHELL_THEME_COUNT;
        save_shell_preferences(shell);
        fprintf(stderr, "system-shell: theme changed to %u\n", shell->ui.theme);
    } else if (event == CP0_UI_EVENT_TIMEOUT_PREVIOUS ||
               event == CP0_UI_EVENT_TIMEOUT_NEXT) {
        shell->ui.screen_timeout =
            (shell->ui.screen_timeout +
             (event == CP0_UI_EVENT_TIMEOUT_PREVIOUS ? 3U : 1U)) %
            CP0_SHELL_TIMEOUT_COUNT;
        save_shell_preferences(shell);
        apply_screen_timeout(shell);
    } else if (event == CP0_UI_EVENT_KEY_SOUNDS_TOGGLE) {
        shell->ui.key_sounds = !shell->ui.key_sounds;
        save_shell_preferences(shell);
        fprintf(stderr, "system-shell: key sounds %s\n",
                shell->ui.key_sounds ? "enabled" : "disabled");
    } else if (event == CP0_UI_EVENT_VOLUME_DOWN ||
               event == CP0_UI_EVENT_VOLUME_UP ||
               event == CP0_UI_EVENT_MUTE) {
        struct cp0_audio_output_state state = {0};
        int result;
        if (event == CP0_UI_EVENT_MUTE) {
            result = cp0_audio_set_output_muted(!shell->ui.muted, &state);
        } else {
            enum cp0_audio_settings_direction direction =
                event == CP0_UI_EVENT_VOLUME_DOWN
                    ? CP0_AUDIO_SETTINGS_DECREASE
                    : CP0_AUDIO_SETTINGS_INCREASE;
            result = cp0_audio_adjust_output_volume(direction, &state);
        }
        if (result == CP0_AUDIO_SETTINGS_OK) {
            apply_audio_output_state(shell, &state);
            fprintf(stderr,
                    "system-shell: audio output changed to %u%% muted=%s\n",
                    state.volume_percent, state.muted ? "true" : "false");
        } else {
            cp0_ui_set_audio_output_state(&shell->ui, false, 0, false);
            fprintf(stderr, "system-shell: audio output control unavailable\n");
        }
    } else if (event == CP0_UI_EVENT_MEDIA_PLAY_PAUSE ||
               event == CP0_UI_EVENT_MEDIA_PREVIOUS ||
               event == CP0_UI_EVENT_MEDIA_NEXT) {
        enum cp0_media_action action =
            event == CP0_UI_EVENT_MEDIA_PLAY_PAUSE
                ? CP0_MEDIA_ACTION_PLAY_PAUSE
                : (event == CP0_UI_EVENT_MEDIA_PREVIOUS
                       ? CP0_MEDIA_ACTION_PREVIOUS
                       : CP0_MEDIA_ACTION_NEXT);
        char app_id[CP0_APP_ID_BYTES];
        int result = cp0_appd_dispatch_media_action(action, app_id);
        enum cp0_ui_media_status status =
            result == CP0_MEDIA_DISPATCH_SENT
                ? CP0_UI_MEDIA_SENT
                : (result == CP0_MEDIA_DISPATCH_UNAVAILABLE
                       ? CP0_UI_MEDIA_UNAVAILABLE
                       : (result == CP0_MEDIA_DISPATCH_BUSY
                              ? CP0_UI_MEDIA_BUSY
                              : CP0_UI_MEDIA_FAILED));
        cp0_ui_set_media_status(&shell->ui, status);
        if (result == CP0_MEDIA_DISPATCH_SENT)
            fprintf(stderr, "system-shell: media action sent to %s\n", app_id);
    } else if (event == CP0_UI_EVENT_SCREENSHOT) {
        shell->ui.system_action_overlay = false;
        shell->ui.system_action_ticks = 0;
        begin_screenshot(shell);
    } else if (event == CP0_UI_EVENT_STORE_DETAILS) {
        shell_redraw(shell);
        wl_display_flush(shell->display);
        load_store_details(shell);
        fprintf(stderr, "system-shell: store details loaded\n");
    } else if (event == CP0_UI_EVENT_STORE_SCREENSHOT) {
        shell_redraw(shell);
        wl_display_flush(shell->display);
        load_store_screenshot(shell);
        fprintf(stderr, "system-shell: store screenshot loaded\n");
    } else if (event == CP0_UI_EVENT_STORE_REFRESH) {
        int result = cp0_store_refresh();
        if (result == CP0_STORE_RESULT_OK) {
            cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_LOADING);
            shell->store_poll_delay = 1;
            fprintf(stderr, "system-shell: store refresh requested\n");
        } else if (result == CP0_STORE_RESULT_UNCONFIGURED) {
            cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNCONFIGURED);
        } else if (result != CP0_STORE_RESULT_BUSY) {
            cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNAVAILABLE);
            fprintf(stderr, "system-shell: store refresh failed\n");
        }
    } else if (event == CP0_UI_EVENT_STORE_INSTALL) {
        char app_id[CP0_STORE_APP_ID_BYTES];
        const char *selected = cp0_ui_selected_store_app_id(&shell->ui);
        if (selected != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected) > 0) {
            const char *app_ids[] = {app_id};
            begin_store_preflight(shell, app_ids, 1);
        }
    } else if (event == CP0_UI_EVENT_STORE_UPDATE_ALL) {
        const char *app_ids[CP0_STORE_INSTALL_BATCH_MAX];
        size_t app_count = cp0_ui_collect_store_update_batch(
            &shell->ui, app_ids, CP0_STORE_INSTALL_BATCH_MAX);
        if (app_count > 0)
            begin_store_preflight(shell, app_ids, app_count);
    } else if (event == CP0_UI_EVENT_STORE_INSTALL_CONFIRM) {
        (void)submit_store_preflight(shell);
    } else if (event == CP0_UI_EVENT_STORE_PAUSE ||
               event == CP0_UI_EVENT_STORE_RESUME ||
               event == CP0_UI_EVENT_STORE_CANCEL) {
        char app_id[CP0_STORE_APP_ID_BYTES];
        const char *selected = cp0_ui_selected_store_app_id(&shell->ui);
        enum cp0_store_control_action action =
            event == CP0_UI_EVENT_STORE_PAUSE
                ? CP0_STORE_CONTROL_PAUSE
                : (event == CP0_UI_EVENT_STORE_RESUME
                       ? CP0_STORE_CONTROL_RESUME
                       : CP0_STORE_CONTROL_CANCEL);
        if (selected != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected) > 0) {
            int result = cp0_store_control(app_id, action);
            if (result == CP0_STORE_RESULT_OK) {
                if (action == CP0_STORE_CONTROL_RESUME)
                    cp0_ui_set_store_app_state(&shell->ui, app_id,
                                               CP0_UI_STORE_QUEUED, 0);
                shell->store_poll_delay = 1;
                fprintf(stderr,
                        "system-shell: store control %u requested for %s\n",
                        (unsigned int)action, app_id);
            } else if (result != CP0_STORE_RESULT_BUSY) {
                shell->store_poll_delay = 1;
                if (result == CP0_STORE_RESULT_POLICY_RESTRICTED ||
                    result == CP0_STORE_RESULT_INSUFFICIENT_STORAGE ||
                    result == CP0_STORE_RESULT_CATALOG_CHANGED)
                    show_store_preflight_error(shell, result);
                fprintf(stderr,
                        "system-shell: store control %u failed for %s\n",
                        (unsigned int)action, app_id);
            }
        }
    } else if (event == CP0_UI_EVENT_STORE_SEARCH) {
        poll_store_search(shell);
    } else if (event == CP0_UI_EVENT_STORE_BROWSE) {
        poll_store_browse(shell);
    } else if (event == CP0_UI_EVENT_DEVELOPER_OPEN_PAIRING) {
        uint16_t remaining_seconds;
        if (cp0_developer_open_pairing(600, &remaining_seconds) == 0) {
            (void)remaining_seconds;
            poll_developer_access(shell);
            fprintf(stderr,
                    "system-shell: developer pairing window opened for 10 minutes\n");
        } else {
            fprintf(stderr,
                    "system-shell: developer pairing window could not be opened\n");
            poll_developer_access(shell);
        }
    } else if (event == CP0_UI_EVENT_DEVELOPER_UNPAIR ||
               event == CP0_UI_EVENT_DEVELOPER_UNPAIR_ALL) {
        uint8_t remaining;
        const char *fingerprint =
            cp0_ui_selected_developer_fingerprint(&shell->ui);
        int result = event == CP0_UI_EVENT_DEVELOPER_UNPAIR_ALL
                         ? cp0_developer_unpair_all(&remaining)
                         : cp0_developer_unpair(fingerprint, &remaining);
        if (result == 0) {
            fprintf(stderr,
                    "system-shell: developer authorization revoked; %u remain\n",
                    (unsigned int)remaining);
            poll_developer_access(shell);
        } else {
            fprintf(stderr,
                    "system-shell: developer authorization revocation failed\n");
            poll_developer_access(shell);
        }
    } else if (event == CP0_UI_EVENT_DEVELOPER_ENABLE ||
               event == CP0_UI_EVENT_DEVELOPER_DISABLE ||
               event == CP0_UI_EVENT_RECOVERY_ENABLE ||
               event == CP0_UI_EVENT_RECOVERY_DISABLE) {
        bool recovery = event == CP0_UI_EVENT_RECOVERY_ENABLE ||
                        event == CP0_UI_EVENT_RECOVERY_DISABLE;
        bool enabled = event == CP0_UI_EVENT_DEVELOPER_ENABLE ||
                       event == CP0_UI_EVENT_RECOVERY_ENABLE;
        struct cp0_device_settings settings;
        enum cp0_device_mode mode = recovery ? CP0_DEVICE_MODE_RECOVERY
                                             : CP0_DEVICE_MODE_DEVELOPER;
        if (cp0_appd_set_device_mode(mode, enabled, &settings) == 0) {
            apply_device_settings(shell, &settings);
            if (!recovery)
                poll_developer_access(shell);
            fprintf(stderr, "system-shell: %s mode %s\n",
                    recovery ? "recovery" : "developer",
                    enabled ? "enabled" : "disabled");
        } else {
            fprintf(stderr, "system-shell: %s mode update failed\n",
                    recovery ? "recovery" : "developer");
            poll_device_settings(shell);
        }
    } else if (event == CP0_UI_EVENT_AUTO_UPDATE_ENABLE ||
               event == CP0_UI_EVENT_AUTO_UPDATE_DISABLE) {
        bool enabled = event == CP0_UI_EVENT_AUTO_UPDATE_ENABLE;
        struct cp0_store_auto_update_status status;
        if (cp0_store_set_auto_update(enabled, &status) ==
            CP0_STORE_RESULT_OK) {
            apply_auto_update_status(shell, &status);
            fprintf(stderr, "system-shell: automatic app updates %s\n",
                    enabled ? "enabled" : "disabled");
        } else {
            fprintf(stderr,
                    "system-shell: automatic app update setting failed\n");
            poll_auto_update_status(shell);
        }
    } else if (event == CP0_UI_EVENT_METRICS_ENABLE ||
               event == CP0_UI_EVENT_METRICS_DISABLE) {
        bool enabled = event == CP0_UI_EVENT_METRICS_ENABLE;
        struct cp0_store_metrics_status status;
        if (cp0_store_set_metrics(enabled, &status) == CP0_STORE_RESULT_OK) {
            apply_metrics_status(shell, &status);
            fprintf(stderr, "system-shell: aggregate app metrics %s\n",
                    enabled ? "enabled" : "disabled and cleared");
        } else {
            fprintf(stderr,
                    "system-shell: aggregate app metrics setting failed\n");
            poll_metrics_status(shell);
        }
    }
    if (previous_screen != CP0_UI_STORE &&
        shell->ui.screen == CP0_UI_STORE) {
        poll_app_catalog(shell);
        poll_store_catalog(shell);
    }
    if (previous_screen != CP0_UI_TASKS &&
        shell->ui.screen == CP0_UI_TASKS)
        poll_task_catalog(shell);
    if (previous_screen != CP0_UI_SETTINGS &&
        shell->ui.screen == CP0_UI_SETTINGS) {
        poll_display_state(shell);
        poll_audio_output_state(shell);
        poll_connectivity_state(shell);
        poll_device_settings(shell);
        poll_developer_access(shell);
        poll_auto_update_status(shell);
        poll_metrics_status(shell);
    }
    shell_redraw(shell);
}

static void handle_system_action(void *data,
                                 struct cp0_system_shell_v1 *system_control,
                                 uint32_t action)
{
    struct shell *shell = data;
    enum cp0_ui_action ui_action;
    (void)system_control;

    switch (action) {
    case CP0_SYSTEM_SHELL_V1_ACTION_HOME:
        ui_action = CP0_UI_GO_HOME;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_BACK:
        ui_action = CP0_UI_BACK;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_TASKS:
        ui_action = CP0_UI_SHOW_TASKS;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_POWER:
        ui_action = CP0_UI_SHOW_POWER;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_BRIGHTNESS_DOWN:
        ui_action = CP0_UI_BRIGHTNESS_DOWN;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_BRIGHTNESS_UP:
        ui_action = CP0_UI_BRIGHTNESS_UP;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_MUTE:
        ui_action = CP0_UI_MUTE;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_VOLUME_DOWN:
        ui_action = CP0_UI_VOLUME_DOWN;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_VOLUME_UP:
        ui_action = CP0_UI_VOLUME_UP;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_MEDIA_PLAY_PAUSE:
        ui_action = CP0_UI_MEDIA_PLAY_PAUSE;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_MEDIA_PREVIOUS:
        ui_action = CP0_UI_MEDIA_PREVIOUS;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_MEDIA_NEXT:
        ui_action = CP0_UI_MEDIA_NEXT;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_HELP:
        ui_action = CP0_UI_HELP;
        break;
    case CP0_SYSTEM_SHELL_V1_ACTION_SCREENSHOT:
        ui_action = CP0_UI_SCREENSHOT;
        break;
    default:
        return;
    }
    if (shell->ui.setup_active) {
        if (action != CP0_SYSTEM_SHELL_V1_ACTION_SCREENSHOT)
            handle_ui_action(shell, ui_action);
        return;
    }
    if (action == CP0_SYSTEM_SHELL_V1_ACTION_SCREENSHOT) {
        begin_screenshot(shell);
        return;
    }
    cancel_notification(shell, true);
    if (action > CP0_SYSTEM_SHELL_V1_ACTION_POWER &&
        action != CP0_SYSTEM_SHELL_V1_ACTION_HELP) {
        enum cp0_overlay_state base = cp0_overlay_transient_base(
            shell->ui.system_action_overlay, shell->overlay_mode,
            shell->system_action_restore_mode);
        cancel_system_action(shell, false);
        shell->system_action_restore_mode = base;
        shell->overlay_mode = cp0_overlay_transient_target(base);
    } else {
        cancel_system_action(shell, false);
        shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    }
    cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                          shell->overlay_mode);
    handle_ui_action(shell, ui_action);
}

static void handle_app_added(void *data,
                             struct cp0_system_shell_v1 *system_control,
                             uint32_t token, const char *app_id)
{
    struct shell *shell = data;
    (void)system_control;
    remember_surface(shell, token, app_id);
    cp0_ui_add_app(&shell->ui, token, app_id);
    fprintf(stderr, "system-shell: app token=%u available\n", token);
    if (shell->pending_activation_uid == 0 &&
        strcmp(shell->pending_activation, app_id) == 0) {
        shell->pending_activation[0] = '\0';
        shell->pending_activation_uid = 0;
        shell->overlay_mode = cp0_ui_app_is_immersive(&shell->ui, token)
                                  ? CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN
                                  : CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS;
        cp0_system_shell_v1_activate_app(shell->system_control, token);
        fprintf(stderr, "system-shell: app token=%u auto-activated\n", token);
    }
    shell_redraw(shell);
}

static void handle_app_removed(void *data,
                               struct cp0_system_shell_v1 *system_control,
                               uint32_t token)
{
    struct shell *shell = data;
    (void)system_control;
    if (shell->ui.notification_banner) {
        cancel_notification(shell, false);
        shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    }
    forget_surface(shell, token);
    cp0_ui_remove_app(&shell->ui, token);
    fprintf(stderr, "system-shell: app token=%u removed\n", token);
    shell_redraw(shell);
}

static void handle_activation_failed(
    void *data, struct cp0_system_shell_v1 *system_control, uint32_t token)
{
    struct shell *shell = data;
    (void)system_control;
    cp0_ui_remove_app(&shell->ui, token);
    shell->pending_activation[0] = '\0';
    shell->pending_activation_uid = 0;
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    fprintf(stderr, "system-shell: app token=%u activation failed\n", token);
    shell_redraw(shell);
}

static void handle_app_display_mode(
    void *data, struct cp0_system_shell_v1 *system_control, uint32_t token,
    uint32_t mode)
{
    struct shell *shell = data;
    (void)system_control;
    cp0_ui_set_app_display_mode(
        &shell->ui, token,
        mode == CP0_SYSTEM_SHELL_V1_DISPLAY_MODE_IMMERSIVE);
}

static void handle_app_identity(
    void *data, struct cp0_system_shell_v1 *system_control, uint32_t token,
    uint32_t account_uid)
{
    struct shell *shell = data;
    struct app_surface_binding *binding = surface_binding(shell, token);
    (void)system_control;

    if (binding == NULL || account_uid == 0)
        return;
    binding->account_uid = account_uid;
    if (shell->pending_activation_uid == account_uid &&
        strcmp(shell->pending_activation, binding->app_id) == 0) {
        shell->pending_activation[0] = '\0';
        shell->pending_activation_uid = 0;
        cp0_system_shell_v1_activate_app(shell->system_control, token);
        fprintf(stderr,
                "system-shell: app token=%u authenticated and activated\n",
                token);
    }
}

static const struct cp0_system_shell_v1_listener system_control_listener = {
    .action = handle_system_action,
    .app_added = handle_app_added,
    .app_removed = handle_app_removed,
    .activation_failed = handle_activation_failed,
    .app_display_mode = handle_app_display_mode,
    .app_identity = handle_app_identity,
};

static bool translate_key(struct shell *shell, uint32_t key,
                          uint32_t key_state, enum cp0_ui_action *action)
{
    bool pressed = key_state == WL_KEYBOARD_KEY_STATE_PRESSED;
    if (key == KEY_LEFTMETA || key == KEY_RIGHTMETA) {
        shell->meta_pressed = pressed;
        return false;
    }
    if (key == KEY_LEFTSHIFT || key == KEY_RIGHTSHIFT) {
        shell->shift_pressed = pressed;
        return false;
    }
    if (!pressed)
        return false;

    switch (key) {
    case KEY_UP:
        *action = CP0_UI_UP;
        return true;
    case KEY_DOWN:
        *action = CP0_UI_DOWN;
        return true;
    case KEY_LEFT:
        *action = CP0_UI_LEFT;
        return true;
    case KEY_RIGHT:
        *action = CP0_UI_RIGHT;
        return true;
    case KEY_ENTER:
    case KEY_KPENTER:
        *action = CP0_UI_ACCEPT;
        return true;
    case KEY_ESC:
    case KEY_BACKSPACE:
        *action = CP0_UI_BACK;
        return true;
    case KEY_HOMEPAGE:
    case KEY_F1:
        *action = CP0_UI_GO_HOME;
        return true;
    case KEY_F2:
        *action = CP0_UI_BACK;
        return true;
    case KEY_F3:
        *action = CP0_UI_SHOW_TASKS;
        return true;
    case KEY_POWER:
    case KEY_F4:
        *action = CP0_UI_SHOW_POWER;
        return true;
    case KEY_F5:
        *action = CP0_UI_LEFT;
        return true;
    case KEY_F6:
        *action = CP0_UI_RIGHT;
        return true;
    case KEY_F:
        *action = CP0_UI_UP;
        return true;
    case KEY_Z:
        *action = CP0_UI_LEFT;
        return true;
    case KEY_X:
        *action = CP0_UI_DOWN;
        return true;
    case KEY_C:
        *action = CP0_UI_RIGHT;
        return true;
    case KEY_H:
        if (shell->meta_pressed) {
            *action = CP0_UI_GO_HOME;
            return true;
        }
        break;
    case KEY_B:
        if (shell->meta_pressed) {
            *action = CP0_UI_BACK;
            return true;
        }
        break;
    case KEY_TAB:
        if (shell->meta_pressed) {
            *action = CP0_UI_SHOW_TASKS;
            return true;
        }
        break;
    default:
        break;
    }
    return false;
}

static void handle_keyboard_keymap(void *data, struct wl_keyboard *keyboard,
                                   uint32_t format, int fd, uint32_t size)
{
    struct shell *shell = data;
    struct xkb_context *context = NULL;
    struct xkb_keymap *keymap = NULL;
    void *mapping = MAP_FAILED;
    xkb_mod_index_t shift_index;
    (void)keyboard;

    shell->shift_modifier_mask = 0;
    shell->shift_modifier_active = false;
    if (format != WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1 || size == 0)
        goto cleanup;
    mapping = mmap(NULL, size, PROT_READ, MAP_PRIVATE, fd, 0);
    if (mapping == MAP_FAILED)
        goto cleanup;
    context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    if (context == NULL)
        goto cleanup;
    keymap = xkb_keymap_new_from_string(context, mapping,
                                         XKB_KEYMAP_FORMAT_TEXT_V1,
                                         XKB_KEYMAP_COMPILE_NO_FLAGS);
    if (keymap == NULL)
        goto cleanup;
    shift_index = xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_SHIFT);
    if (shift_index != XKB_MOD_INVALID && shift_index < 32U)
        shell->shift_modifier_mask = 1U << shift_index;

cleanup:
    if (keymap != NULL)
        xkb_keymap_unref(keymap);
    if (context != NULL)
        xkb_context_unref(context);
    if (mapping != MAP_FAILED)
        munmap(mapping, size);
    close(fd);
}

static void handle_keyboard_enter(void *data, struct wl_keyboard *keyboard,
                                  uint32_t serial, struct wl_surface *surface,
                                  struct wl_array *keys)
{
    (void)keyboard;
    (void)serial;
    (void)surface;
    (void)keys;
    shell_redraw(data);
}

static void handle_keyboard_leave(void *data, struct wl_keyboard *keyboard,
                                  uint32_t serial, struct wl_surface *surface)
{
    struct shell *shell = data;
    (void)keyboard;
    (void)serial;
    (void)surface;
    shell->meta_pressed = false;
    shell->shift_pressed = false;
    shell->shift_modifier_active = false;
}

static bool shell_shift_active(const struct shell *shell)
{
    return shell->shift_pressed || shell->shift_modifier_active;
}

static void handle_keyboard_key(void *data, struct wl_keyboard *keyboard,
                                uint32_t serial, uint32_t time, uint32_t key,
                                uint32_t state)
{
    struct shell *shell = data;
    enum cp0_ui_action action;
    (void)keyboard;
    (void)serial;
    (void)time;
    bool pressed = state == WL_KEYBOARD_KEY_STATE_PRESSED;
    if (key == KEY_LEFTSHIFT || key == KEY_RIGHTSHIFT) {
        shell->shift_pressed = pressed;
        return;
    }
    if (pressed && !shell->ui.setup_active && key != KEY_LEFTMETA &&
        key != KEY_RIGHTMETA &&
        shell->ui.key_sounds && shell->ui.volume_available &&
        !shell->ui.muted)
        (void)cp0_audio_play_key_click();
    if (pressed && cp0_ui_setup_accepts_text(&shell->ui)) {
        bool handled = false;
        if (key == KEY_BACKSPACE)
            handled = cp0_ui_setup_backspace(&shell->ui);
        else {
            char character = cp0_ui_key_character(key, shell_shift_active(shell));
            if (character != '\0')
                handled = cp0_ui_setup_input_ascii(&shell->ui, character);
        }
        if (handled) {
            shell_redraw(shell);
            return;
        }
    }
    if (pressed && cp0_ui_store_accepts_text(&shell->ui)) {
        enum cp0_ui_event event = CP0_UI_EVENT_NONE;
        bool handled = false;
        if (key == KEY_BACKSPACE &&
            cp0_ui_store_search_query(&shell->ui)[0] != '\0') {
            event = cp0_ui_store_backspace(&shell->ui);
            handled = true;
        } else {
            char character = cp0_ui_key_character(key, shell_shift_active(shell));
            if (character != '\0') {
                event = cp0_ui_store_input_ascii(&shell->ui, character);
                handled = true;
            }
        }
        if (handled) {
            if (event == CP0_UI_EVENT_STORE_SEARCH)
                poll_store_search(shell);
            shell_redraw(shell);
            return;
        }
    }
    if (translate_key(shell, key, state, &action))
        handle_ui_action(shell, action);
}

static void handle_keyboard_modifiers(void *data, struct wl_keyboard *keyboard,
                                      uint32_t serial,
                                      uint32_t mods_depressed,
                                      uint32_t mods_latched,
                                      uint32_t mods_locked, uint32_t group)
{
    struct shell *shell = data;
    (void)keyboard;
    (void)serial;
    (void)mods_latched;
    (void)mods_locked;
    (void)group;
    shell->shift_modifier_active =
        shell->shift_modifier_mask != 0 &&
        (mods_depressed & shell->shift_modifier_mask) != 0;
}

static const struct wl_keyboard_listener keyboard_listener = {
    .keymap = handle_keyboard_keymap,
    .enter = handle_keyboard_enter,
    .leave = handle_keyboard_leave,
    .key = handle_keyboard_key,
    .modifiers = handle_keyboard_modifiers,
};

static void handle_seat_capabilities(void *data, struct wl_seat *seat,
                                     enum wl_seat_capability capabilities)
{
    struct shell *shell = data;
    if ((capabilities & WL_SEAT_CAPABILITY_KEYBOARD) != 0 &&
        shell->keyboard == NULL) {
        shell->keyboard = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(shell->keyboard, &keyboard_listener, shell);
    } else if ((capabilities & WL_SEAT_CAPABILITY_KEYBOARD) == 0 &&
               shell->keyboard != NULL) {
        wl_keyboard_destroy(shell->keyboard);
        shell->keyboard = NULL;
    }
}

static const struct wl_seat_listener seat_listener = {
    .capabilities = handle_seat_capabilities,
};

static void handle_shm_format(void *data, struct wl_shm *shm, uint32_t format)
{
    struct shell *shell = data;
    (void)shm;
    if (format == WL_SHM_FORMAT_ARGB8888)
        shell->has_argb = true;
}

static const struct wl_shm_listener shm_listener = {
    .format = handle_shm_format,
};

static void handle_wm_base_ping(void *data, struct xdg_wm_base *wm_base,
                                uint32_t serial)
{
    (void)data;
    xdg_wm_base_pong(wm_base, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = {
    .ping = handle_wm_base_ping,
};

static void handle_registry_global(void *data, struct wl_registry *registry,
                                   uint32_t name, const char *interface,
                                   uint32_t version)
{
    struct shell *shell = data;
    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        shell->compositor = wl_registry_bind(
            registry, name, &wl_compositor_interface, 1);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        shell->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
        wl_shm_add_listener(shell->shm, &shm_listener, shell);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        shell->seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
        wl_seat_add_listener(shell->seat, &seat_listener, shell);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        shell->wm_base = wl_registry_bind(registry, name,
                                          &xdg_wm_base_interface, 1);
        xdg_wm_base_add_listener(shell->wm_base, &wm_base_listener, shell);
    } else if (strcmp(interface, cp0_system_shell_v1_interface.name) == 0 &&
               version >= 4) {
        uint32_t bind_version = version < 7 ? version : 7;
        shell->system_control = wl_registry_bind(
            registry, name, &cp0_system_shell_v1_interface, bind_version);
        cp0_system_shell_v1_add_listener(shell->system_control,
                                         &system_control_listener, shell);
    } else if (strcmp(interface, wl_output_interface.name) == 0 &&
               shell->capture_output == NULL) {
        uint32_t bind_version = version < 4 ? version : 4;
        shell->capture_output =
            wl_registry_bind(registry, name, &wl_output_interface,
                             bind_version);
        shell->capture_output_name = name;
        maybe_create_capture_source(shell);
    } else if (strcmp(interface, weston_capture_v1_interface.name) == 0) {
        shell->capture_factory = wl_registry_bind(
            registry, name, &weston_capture_v1_interface, 1);
        shell->capture_factory_name = name;
        maybe_create_capture_source(shell);
    }
}

static void handle_registry_remove(void *data, struct wl_registry *registry,
                                   uint32_t name)
{
    struct shell *shell = data;
    (void)registry;
    if (name == shell->capture_output_name) {
        if (shell->capture_source != NULL)
            weston_capture_source_v1_destroy(shell->capture_source);
        shell->capture_source = NULL;
        destroy_screenshot_buffer(&shell->screenshot_buffer);
        shell->capture_busy = false;
        if (shell->capture_output != NULL) {
            if (wl_output_get_version(shell->capture_output) >=
                WL_OUTPUT_RELEASE_SINCE_VERSION)
                wl_output_release(shell->capture_output);
            else
                wl_output_destroy(shell->capture_output);
        }
        shell->capture_output = NULL;
        shell->capture_output_name = 0;
        shell->capture_width = 0;
        shell->capture_height = 0;
        shell->capture_format = 0;
    } else if (name == shell->capture_factory_name) {
        if (shell->capture_source != NULL)
            weston_capture_source_v1_destroy(shell->capture_source);
        shell->capture_source = NULL;
        destroy_screenshot_buffer(&shell->screenshot_buffer);
        shell->capture_busy = false;
        if (shell->capture_factory != NULL)
            weston_capture_v1_destroy(shell->capture_factory);
        shell->capture_factory = NULL;
        shell->capture_factory_name = 0;
        shell->capture_width = 0;
        shell->capture_height = 0;
        shell->capture_format = 0;
    }
}

static const struct wl_registry_listener registry_listener = {
    .global = handle_registry_global,
    .global_remove = handle_registry_remove,
};

static void handle_xdg_surface_configure(void *data,
                                         struct xdg_surface *xdg_surface,
                                         uint32_t serial)
{
    struct shell *shell = data;
    xdg_surface_ack_configure(xdg_surface, serial);
    shell->configured = true;
    shell_redraw(shell);
}

static const struct xdg_surface_listener xdg_surface_listener = {
    .configure = handle_xdg_surface_configure,
};

static void handle_toplevel_configure(void *data,
                                      struct xdg_toplevel *xdg_toplevel,
                                      int32_t width, int32_t height,
                                      struct wl_array *states)
{
    struct shell *shell = data;
    (void)xdg_toplevel;
    (void)states;
    if (width > 0 && height > 0) {
        shell->width = width;
        shell->height = height;
    }
}

static void handle_toplevel_close(void *data,
                                  struct xdg_toplevel *xdg_toplevel)
{
    (void)data;
    (void)xdg_toplevel;
    stop_requested = 1;
}

static const struct xdg_toplevel_listener toplevel_listener = {
    .configure = handle_toplevel_configure,
    .close = handle_toplevel_close,
};

static bool shell_connect(struct shell *shell)
{
    shell->timer_fd = -1;
    shell->width = CP0_UI_WIDTH;
    shell->height = CP0_UI_HEIGHT;
    cp0_ui_init(&shell->ui);
    load_shell_preferences(shell);
    initialize_provisioning(shell);
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    shell->display = wl_display_connect(NULL);
    if (shell->display == NULL) {
        fprintf(stderr, "system-shell: cannot connect to Wayland display\n");
        return false;
    }

    shell->registry = wl_display_get_registry(shell->display);
    wl_registry_add_listener(shell->registry, &registry_listener, shell);
    if (wl_display_roundtrip(shell->display) < 0 ||
        wl_display_roundtrip(shell->display) < 0)
        return false;
    if (shell->compositor == NULL || shell->shm == NULL ||
        shell->wm_base == NULL || shell->seat == NULL ||
        shell->system_control == NULL || !shell->has_argb) {
        fprintf(stderr, "system-shell: required Wayland globals unavailable\n");
        return false;
    }
    apply_screen_timeout(shell);

    shell->surface = wl_compositor_create_surface(shell->compositor);
    shell->xdg_surface =
        xdg_wm_base_get_xdg_surface(shell->wm_base, shell->surface);
    xdg_surface_add_listener(shell->xdg_surface, &xdg_surface_listener, shell);
    shell->xdg_toplevel = xdg_surface_get_toplevel(shell->xdg_surface);
    xdg_toplevel_add_listener(shell->xdg_toplevel, &toplevel_listener, shell);
    xdg_toplevel_set_title(shell->xdg_toplevel, "CardputerZero System Shell");
    xdg_toplevel_set_app_id(shell->xdg_toplevel, "os.cardputerzero.shell");
    cp0_system_shell_v1_register_surface(shell->system_control,
                                         shell->surface);
    xdg_toplevel_set_fullscreen(shell->xdg_toplevel, NULL);
    wl_surface_commit(shell->surface);

    shell->timer_fd = timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC | TFD_NONBLOCK);
    if (shell->timer_fd < 0) {
        fprintf(stderr, "system-shell: cannot create status timer: %s\n",
                strerror(errno));
        return false;
    }
    const struct itimerspec timer = {
        .it_value = {.tv_sec = 1},
        .it_interval = {.tv_sec = 1},
    };
    if (timerfd_settime(shell->timer_fd, 0, &timer, NULL) < 0) {
        fprintf(stderr, "system-shell: cannot arm status timer: %s\n",
                strerror(errno));
        return false;
    }
    return true;
}

static void shell_destroy(struct shell *shell)
{
    if (shell->timer_fd >= 0)
        close(shell->timer_fd);
    for (size_t index = 0; index < BUFFER_COUNT; index++)
        destroy_buffer(&shell->buffers[index]);
    if (shell->capture_source != NULL)
        weston_capture_source_v1_destroy(shell->capture_source);
    destroy_screenshot_buffer(&shell->screenshot_buffer);
    if (shell->capture_factory != NULL)
        weston_capture_v1_destroy(shell->capture_factory);
    if (shell->capture_output != NULL) {
        if (wl_output_get_version(shell->capture_output) >=
            WL_OUTPUT_RELEASE_SINCE_VERSION)
            wl_output_release(shell->capture_output);
        else
            wl_output_destroy(shell->capture_output);
    }
    if (shell->xdg_toplevel != NULL)
        xdg_toplevel_destroy(shell->xdg_toplevel);
    if (shell->xdg_surface != NULL)
        xdg_surface_destroy(shell->xdg_surface);
    if (shell->surface != NULL)
        wl_surface_destroy(shell->surface);
    if (shell->keyboard != NULL)
        wl_keyboard_destroy(shell->keyboard);
    if (shell->seat != NULL)
        wl_seat_destroy(shell->seat);
    if (shell->wm_base != NULL)
        xdg_wm_base_destroy(shell->wm_base);
    if (shell->system_control != NULL)
        cp0_system_shell_v1_destroy(shell->system_control);
    if (shell->shm != NULL)
        wl_shm_destroy(shell->shm);
    if (shell->compositor != NULL)
        wl_compositor_destroy(shell->compositor);
    if (shell->registry != NULL)
        wl_registry_destroy(shell->registry);
    if (shell->display != NULL) {
        wl_display_flush(shell->display);
        wl_display_disconnect(shell->display);
    }
    cp0_ui_deinit(&shell->ui);
}

static int shell_dispatch(struct shell *shell)
{
    int display_fd = wl_display_get_fd(shell->display);

    if (!shell->ui.setup_active) {
        poll_app_catalog(shell);
        poll_task_catalog(shell);
        poll_store_catalog(shell);
        poll_display_state(shell);
        poll_audio_output_state(shell);
        poll_connectivity_state(shell);
    }
    shell_redraw(shell);

    while (!stop_requested) {
        while (wl_display_prepare_read(shell->display) != 0) {
            if (wl_display_dispatch_pending(shell->display) < 0)
                return -1;
        }

        short display_events = POLLIN;
        if (wl_display_flush(shell->display) < 0) {
            if (errno != EAGAIN) {
                wl_display_cancel_read(shell->display);
                return -1;
            }
            display_events |= POLLOUT;
        }

        struct pollfd descriptors[] = {
            {.fd = display_fd, .events = display_events},
            {.fd = shell->timer_fd, .events = POLLIN},
        };
        int result = poll(descriptors, 2, -1);
        if (result < 0) {
            wl_display_cancel_read(shell->display);
            if (errno == EINTR && stop_requested)
                return 0;
            if (errno == EINTR)
                continue;
            return -1;
        }

        if ((descriptors[0].revents & POLLIN) != 0) {
            if (wl_display_read_events(shell->display) < 0)
                return -1;
        } else {
            wl_display_cancel_read(shell->display);
        }
        if ((descriptors[0].revents & POLLOUT) != 0 &&
            wl_display_flush(shell->display) < 0 && errno != EAGAIN)
            return -1;
        if ((descriptors[0].revents & (POLLERR | POLLHUP | POLLNVAL)) != 0)
            return -1;
        if (wl_display_dispatch_pending(shell->display) < 0)
            return -1;

        if ((descriptors[1].revents & POLLIN) != 0) {
            uint64_t expirations;
            if (read(shell->timer_fd, &expirations, sizeof(expirations)) > 0) {
                update_notification_timer(shell);
                retry_unavailable_provisioning(shell);
                if (cp0_ui_tick(&shell->ui) &&
                    shell->overlay_mode ==
                        CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_NOTIFICATION &&
                    !shell->ui.notification_banner) {
                    shell->overlay_mode = shell->system_action_restore_mode;
                    cp0_system_shell_v1_set_overlay_mode(shell->system_control,
                                                          shell->overlay_mode);
                }
                if (shell->ui.setup_active) {
                    shell_redraw(shell);
                    continue;
                }
                poll_permission_prompt(shell);
                poll_document_prompt(shell);
                show_store_completion_notification(shell);
                poll_notification(shell);
                shell->catalog_ticks++;
                if (shell->ui.screen == CP0_UI_STORE ||
                    shell->ui.store_activity) {
                    if (shell->store_poll_delay > 0) {
                        shell->store_poll_delay--;
                    } else {
                        poll_app_catalog(shell);
                        poll_store_catalog(shell);
                        show_store_completion_notification(shell);
                    }
                    shell->catalog_ticks = 0;
                } else if (shell->ui.screen == CP0_UI_TASKS) {
                    poll_task_catalog(shell);
                    shell->catalog_ticks = 0;
                } else if (shell->catalog_ticks >= 5) {
                    poll_app_catalog(shell);
                    poll_store_catalog(shell);
                    show_store_completion_notification(shell);
                    if (shell->ui.screen == CP0_UI_SETTINGS) {
                        poll_display_state(shell);
                        poll_audio_output_state(shell);
                        poll_connectivity_state(shell);
                        poll_device_settings(shell);
                        poll_auto_update_status(shell);
                        poll_metrics_status(shell);
                    }
                    shell->catalog_ticks = 0;
                }
                shell_redraw(shell);
            }
        }
    }
    return 0;
}

static void handle_signal(int signal_number)
{
    (void)signal_number;
    stop_requested = 1;
}

int main(void)
{
    struct shell shell = {0};
    struct sigaction action = {
        .sa_handler = handle_signal,
    };
    sigemptyset(&action.sa_mask);
    sigaction(SIGINT, &action, NULL);
    sigaction(SIGTERM, &action, NULL);

    if (!shell_connect(&shell)) {
        shell_destroy(&shell);
        return EXIT_FAILURE;
    }

    int result = shell_dispatch(&shell);
    if (result < 0)
        fprintf(stderr, "system-shell: Wayland dispatch failed: %s\n",
                strerror(errno));
    shell_destroy(&shell);
    return result < 0 && !stop_requested ? EXIT_FAILURE : EXIT_SUCCESS;
}
