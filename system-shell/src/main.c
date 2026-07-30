#define _POSIX_C_SOURCE 200809L

#include "cp0_ui.h"
#include "xdg-shell-client-protocol.h"

#include <dirent.h>
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
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *xdg_toplevel;
    struct shell_buffer buffers[BUFFER_COUNT];
    struct cp0_ui ui;
    int timer_fd;
    int width;
    int height;
    bool configured;
    bool has_xrgb;
    bool meta_pressed;
    bool redraw_pending;
};

static volatile sig_atomic_t stop_requested;
static unsigned int shm_serial;

static void shell_redraw(struct shell *shell);

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
        pool, 0, width, height, stride, WL_SHM_FORMAT_XRGB8888);
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

static bool read_line(const char *path, char *value, size_t value_size)
{
    FILE *input = fopen(path, "r");
    if (input == NULL)
        return false;
    bool ok = fgets(value, (int)value_size, input) != NULL;
    fclose(input);
    if (!ok)
        return false;
    value[strcspn(value, "\r\n")] = '\0';
    return true;
}

static int read_battery_percent(void)
{
    DIR *directory = opendir("/sys/class/power_supply");
    if (directory == NULL)
        return -1;

    int result = -1;
    struct dirent *entry;
    while ((entry = readdir(directory)) != NULL) {
        if (entry->d_name[0] == '.')
            continue;
        char path[512];
        char value[32];
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/type",
                 entry->d_name);
        if (!read_line(path, value, sizeof(value)) ||
            strcmp(value, "Battery") != 0)
            continue;
        snprintf(path, sizeof(path), "/sys/class/power_supply/%s/capacity",
                 entry->d_name);
        if (read_line(path, value, sizeof(value)))
            result = (int)strtol(value, NULL, 10);
        break;
    }
    closedir(directory);
    return result;
}

static bool read_network_online(void)
{
    char state[32];
    return read_line("/sys/class/net/wlan0/operstate", state, sizeof(state)) &&
           strcmp(state, "up") == 0;
}

static void update_status(struct shell *shell)
{
    char clock_text[6] = "--:--";
    time_t now = time(NULL);
    struct tm local_time;
    if (localtime_r(&now, &local_time) != NULL)
        strftime(clock_text, sizeof(clock_text), "%H:%M", &local_time);
    cp0_ui_set_status(&shell->ui, clock_text, read_network_online(),
                      read_battery_percent());
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
    wl_surface_attach(shell->surface, buffer->wl_buffer, 0, 0);
    wl_surface_damage(shell->surface, 0, 0, shell->width, shell->height);
    wl_surface_commit(shell->surface);
    buffer->busy = true;
    shell->redraw_pending = false;
}

static void handle_ui_action(struct shell *shell, enum cp0_ui_action action)
{
    enum cp0_ui_event event = cp0_ui_handle_action(&shell->ui, action);
    if (event == CP0_UI_EVENT_SLEEP)
        fprintf(stderr, "system-shell: sleep requested; broker unavailable\n");
    else if (event == CP0_UI_EVENT_RESTART)
        fprintf(stderr, "system-shell: restart requested; broker unavailable\n");
    shell_redraw(shell);
}

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
    if (format == WL_SHM_FORMAT_XRGB8888)
        shell->has_xrgb = true;
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
    (void)version;
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
        shell->wm_base == NULL || shell->seat == NULL || !shell->has_xrgb) {
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
    xdg_toplevel_set_fullscreen(shell->xdg_toplevel, NULL);
    wl_surface_commit(shell->surface);

    shell->timer_fd = timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC | TFD_NONBLOCK);
    if (shell->timer_fd < 0) {
        fprintf(stderr, "system-shell: cannot create status timer: %s\n",
                strerror(errno));
        return false;
    }
    const struct itimerspec timer = {
        .it_value = {.tv_sec = 30},
        .it_interval = {.tv_sec = 30},
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
            if (read(shell->timer_fd, &expirations, sizeof(expirations)) > 0)
                shell_redraw(shell);
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
