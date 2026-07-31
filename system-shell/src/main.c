#define _POSIX_C_SOURCE 200809L

#include "cardputerzero-system-shell-client-protocol.h"
#include "cp0_appd_client.h"
#include "cp0_store_client.h"
#include "cp0_system_info.h"
#include "cp0_ui.h"
#include "xdg-shell-client-protocol.h"

#include <errno.h>
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

#define BUFFER_COUNT 2

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

struct shell {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct wl_keyboard *keyboard;
    struct xdg_wm_base *wm_base;
    struct cp0_system_shell_v1 *system_control;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *xdg_toplevel;
    struct shell_buffer buffers[BUFFER_COUNT];
    struct cp0_ui ui;
    uint32_t overlay_mode;
    uint32_t interrupted_overlay_mode;
    uint32_t document_restore_mode;
    uint32_t notification_restore_mode;
    int timer_fd;
    int width;
    int height;
    bool configured;
    bool has_argb;
    bool meta_pressed;
    bool redraw_pending;
    bool has_installed_apps;
    unsigned int catalog_ticks;
    unsigned int store_poll_delay;
    unsigned int notification_ticks;
    struct cp0_app_list installed_apps;
    char pending_activation[CP0_APP_ID_BYTES];
};

static volatile sig_atomic_t stop_requested;
static unsigned int shm_serial;

static void shell_redraw(struct shell *shell);

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
        };
    }
    cp0_ui_sync_app_catalog(&shell->ui, catalog, list.count, list.truncated);
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
        CP0_UI_STORE_DOWNLOADING, CP0_UI_STORE_INSTALLING,
        CP0_UI_STORE_INSTALLED,   CP0_UI_STORE_FAILED,
    };
    return direct_states[app->state];
}

static void poll_store_catalog(struct shell *shell)
{
    struct cp0_store_catalog catalog;
    struct cp0_ui_store_catalog_app apps[CP0_STORE_MAX_APPS];
    int result;

    if (shell->ui.screen != CP0_UI_STORE)
        return;
    result = cp0_store_list(&catalog);
    if (result == CP0_STORE_RESULT_UNCONFIGURED) {
        cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNCONFIGURED);
        return;
    }
    if (result != CP0_STORE_RESULT_OK) {
        cp0_ui_set_store_status(&shell->ui, CP0_UI_STORE_UNAVAILABLE);
        return;
    }
    for (size_t index = 0; index < catalog.count; index++) {
        const struct cp0_app_summary *installed =
            installed_app(shell, catalog.apps[index].app_id);
        apps[index] = (struct cp0_ui_store_catalog_app){
            .package_bytes = catalog.apps[index].package_bytes,
            .permissions = catalog.apps[index].permissions,
            .progress_percent = catalog.apps[index].progress_percent,
            .state = store_ui_state(&catalog.apps[index]),
            .app_id = catalog.apps[index].app_id,
            .name = catalog.apps[index].name,
            .version = catalog.apps[index].version,
            .summary = catalog.apps[index].summary,
            .installed_version = installed == NULL ? NULL : installed->version,
        };
    }
    cp0_ui_sync_store_catalog(&shell->ui, apps, catalog.count,
                              catalog.truncated, catalog.stale);
}

static void poll_notification(struct shell *shell)
{
    struct cp0_notification notification;

    if (shell->ui.permission_prompt || shell->ui.document_prompt ||
        shell->ui.power_dialog || shell->ui.settings_confirm ||
        shell->ui.notification_banner ||
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

static void handle_ui_action(struct shell *shell, enum cp0_ui_action action)
{
    enum cp0_ui_screen previous_screen = shell->ui.screen;
    enum cp0_ui_event event = cp0_ui_handle_action(&shell->ui, action);
    if (event == CP0_UI_EVENT_PERMISSION_ONCE ||
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
        fprintf(stderr, "system-shell: restart requested; broker unavailable\n");
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
            } else {
                cp0_ui_set_app_state(&shell->ui, app_id,
                                     CP0_UI_APP_STARTING);
                snprintf(shell->pending_activation,
                         sizeof(shell->pending_activation), "%s", app_id);
                shell_redraw(shell);
                wl_display_flush(shell->display);
                if (cp0_appd_start_app(app_id) != 0) {
                    shell->pending_activation[0] = '\0';
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
    } else if (event == CP0_UI_EVENT_STOP_APP) {
        char app_id[CP0_APP_ID_BYTES];
        const char *selected_id = cp0_ui_selected_app_id(&shell->ui);
        if (selected_id != NULL &&
            snprintf(app_id, sizeof(app_id), "%s", selected_id) > 0) {
            if (cp0_appd_stop_app(app_id) == 0) {
                if (strcmp(shell->pending_activation, app_id) == 0)
                    shell->pending_activation[0] = '\0';
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
            int result = cp0_store_install(app_id);
            if (result == CP0_STORE_RESULT_OK) {
                cp0_ui_set_store_app_state(&shell->ui, app_id,
                                           CP0_UI_STORE_QUEUED, 0);
                shell->store_poll_delay = 1;
                fprintf(stderr,
                        "system-shell: store install requested for %s\n",
                        app_id);
            } else if (result == CP0_STORE_RESULT_UNCONFIGURED) {
                cp0_ui_set_store_status(&shell->ui,
                                        CP0_UI_STORE_UNCONFIGURED);
            } else if (result != CP0_STORE_RESULT_BUSY) {
                cp0_ui_set_store_app_state(&shell->ui, app_id,
                                           CP0_UI_STORE_FAILED, 0);
                fprintf(stderr, "system-shell: store install failed for %s\n",
                        app_id);
            }
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
            fprintf(stderr, "system-shell: %s mode %s\n",
                    recovery ? "recovery" : "developer",
                    enabled ? "enabled" : "disabled");
        } else {
            fprintf(stderr, "system-shell: %s mode update failed\n",
                    recovery ? "recovery" : "developer");
            poll_device_settings(shell);
        }
    }
    if (previous_screen != CP0_UI_STORE &&
        shell->ui.screen == CP0_UI_STORE) {
        poll_app_catalog(shell);
        poll_store_catalog(shell);
    }
    if (previous_screen != CP0_UI_SETTINGS &&
        shell->ui.screen == CP0_UI_SETTINGS)
        poll_device_settings(shell);
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
    default:
        return;
    }
    cancel_notification(shell, false);
    shell->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    handle_ui_action(shell, ui_action);
}

static void handle_app_added(void *data,
                             struct cp0_system_shell_v1 *system_control,
                             uint32_t token, const char *app_id)
{
    struct shell *shell = data;
    (void)system_control;
    cp0_ui_add_app(&shell->ui, token, app_id);
    fprintf(stderr, "system-shell: app token=%u available\n", token);
    if (strcmp(shell->pending_activation, app_id) == 0) {
        shell->pending_activation[0] = '\0';
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

static const struct cp0_system_shell_v1_listener system_control_listener = {
    .action = handle_system_action,
    .app_added = handle_app_added,
    .app_removed = handle_app_removed,
    .activation_failed = handle_activation_failed,
    .app_display_mode = handle_app_display_mode,
};

static bool translate_key(struct shell *shell, uint32_t key,
                          uint32_t key_state, enum cp0_ui_action *action)
{
    bool pressed = key_state == WL_KEYBOARD_KEY_STATE_PRESSED;
    if (key == KEY_LEFTMETA || key == KEY_RIGHTMETA) {
        shell->meta_pressed = pressed;
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
    case KEY_HOME:
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
    (void)data;
    (void)keyboard;
    (void)format;
    (void)size;
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
    if (translate_key(shell, key, state, &action))
        handle_ui_action(shell, action);
}

static void handle_keyboard_modifiers(void *data, struct wl_keyboard *keyboard,
                                      uint32_t serial,
                                      uint32_t mods_depressed,
                                      uint32_t mods_latched,
                                      uint32_t mods_locked, uint32_t group)
{
    (void)data;
    (void)keyboard;
    (void)serial;
    (void)mods_depressed;
    (void)mods_latched;
    (void)mods_locked;
    (void)group;
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
        shell->system_control = wl_registry_bind(
            registry, name, &cp0_system_shell_v1_interface, 4);
        cp0_system_shell_v1_add_listener(shell->system_control,
                                         &system_control_listener, shell);
    }
}

static void handle_registry_remove(void *data, struct wl_registry *registry,
                                   uint32_t name)
{
    (void)data;
    (void)registry;
    (void)name;
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
}

static int shell_dispatch(struct shell *shell)
{
    int display_fd = wl_display_get_fd(shell->display);

    poll_app_catalog(shell);
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
                poll_permission_prompt(shell);
                poll_document_prompt(shell);
                poll_notification(shell);
                shell->catalog_ticks++;
                if (shell->ui.screen == CP0_UI_STORE) {
                    if (shell->store_poll_delay > 0) {
                        shell->store_poll_delay--;
                    } else {
                        poll_app_catalog(shell);
                        poll_store_catalog(shell);
                    }
                    shell->catalog_ticks = 0;
                } else if (shell->ui.screen == CP0_UI_SETTINGS &&
                           shell->catalog_ticks >= 5) {
                    poll_device_settings(shell);
                    shell->catalog_ticks = 0;
                } else if (shell->catalog_ticks >= 5) {
                    poll_app_catalog(shell);
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
