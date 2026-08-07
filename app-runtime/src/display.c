#include "display.h"
#include "broker_client.h"
#include "frame_pacing.h"
#include "input_ascii.h"
#include "input_queue.h"
#include "keyboard_state.h"
#include "pixels.h"

#include <errno.h>
#include <fcntl.h>
#include <linux/memfd.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

#include "wayland-client.h"
#include "xdg-shell-client-protocol.h"

#define CP0_BUFFER_COUNT 2U
#define CP0_FRAME_PIXELS ((size_t)CP0_DISPLAY_WIDTH * CP0_DISPLAY_HEIGHT)
#define CP0_FRAME_BYTES (CP0_FRAME_PIXELS * sizeof(uint32_t))

struct cp0_display_buffer {
    struct wl_buffer *object;
    uint32_t *pixels;
    bool busy;
};

struct cp0_display_state {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct wl_keyboard *keyboard;
    struct xdg_wm_base *wm_base;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *toplevel;
    struct cp0_display_buffer buffers[CP0_BUFFER_COUNT];
    void *mapping;
    size_t mapping_bytes;
    uint32_t *shadow;
    uint16_t content_height;
    uint16_t content_offset_y;
    struct cp0_input_queue input_queue;
    struct cp0_frame_pacer frame_pacer;
    struct cp0_keyboard_state keyboard_state;
    bool has_xrgb8888;
    bool configured;
    bool failed;
    bool first_frame;
    bool keyboard_focused;
};

static struct cp0_display_state state;

static bool valid_app_id(const char *app_id) {
    size_t index;
    size_t length;

    if (app_id == NULL)
        return false;
    length = strlen(app_id);
    if (length == 0U || length > 128U)
        return false;
    for (index = 0; index < length; index++) {
        unsigned char byte = (unsigned char)app_id[index];
        if (!((byte >= 'a' && byte <= 'z') ||
              (byte >= '0' && byte <= '9') || byte == '.' || byte == '-'))
            return false;
    }
    return true;
}

static void handle_registry_global(void *data, struct wl_registry *registry,
                                   uint32_t name, const char *interface,
                                   uint32_t version) {
    struct cp0_display_state *display = data;

    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        uint32_t bind_version = version < 4U ? version : 4U;
        display->compositor = wl_registry_bind(
            registry, name, &wl_compositor_interface, bind_version);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        display->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        display->wm_base =
            wl_registry_bind(registry, name, &xdg_wm_base_interface, 1);
    } else if (strcmp(interface, wl_seat_interface.name) == 0) {
        uint32_t bind_version = version < 5U ? version : 5U;
        display->seat =
            wl_registry_bind(registry, name, &wl_seat_interface, bind_version);
    }
}

static void handle_registry_global_remove(void *data,
                                          struct wl_registry *registry,
                                          uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = handle_registry_global,
    .global_remove = handle_registry_global_remove,
};

static void handle_shm_format(void *data, struct wl_shm *shm,
                              uint32_t format) {
    struct cp0_display_state *display = data;
    (void)shm;

    if (format == WL_SHM_FORMAT_XRGB8888)
        display->has_xrgb8888 = true;
}

static const struct wl_shm_listener shm_listener = {
    .format = handle_shm_format,
};

static void handle_keyboard_keymap(void *data, struct wl_keyboard *keyboard,
                                   uint32_t format, int32_t descriptor,
                                   uint32_t size) {
    (void)data;
    (void)keyboard;
    (void)format;
    (void)size;
    if (descriptor >= 0)
        close(descriptor);
}

static void handle_keyboard_enter(void *data, struct wl_keyboard *keyboard,
                                  uint32_t serial, struct wl_surface *surface,
                                  struct wl_array *keys) {
    struct cp0_display_state *display = data;
    uint32_t *key;
    (void)keyboard;
    (void)serial;

    if (surface != display->surface)
        return;
    cp0_input_queue_reset(&display->input_queue);
    cp0_keyboard_state_reset(&display->keyboard_state);
    wl_array_for_each(key, keys)
        cp0_keyboard_state_set_key(&display->keyboard_state, *key, true);
    display->keyboard_focused = true;
}

static void handle_keyboard_leave(void *data, struct wl_keyboard *keyboard,
                                  uint32_t serial,
                                  struct wl_surface *surface) {
    struct cp0_display_state *display = data;
    (void)keyboard;
    (void)serial;
    (void)surface;

    display->keyboard_focused = false;
    cp0_keyboard_state_reset(&display->keyboard_state);
    cp0_input_queue_reset(&display->input_queue);
}

static void handle_keyboard_key(void *data, struct wl_keyboard *keyboard,
                                uint32_t serial, uint32_t time, uint32_t key,
                                uint32_t key_state) {
    struct cp0_display_state *display = data;
    bool pressed = key_state == WL_KEYBOARD_KEY_STATE_PRESSED;
    uint8_t modifiers;
    uint8_t character;
    (void)keyboard;
    (void)serial;
    (void)time;

    if (!display->keyboard_focused || key > UINT16_MAX)
        return;
    cp0_keyboard_state_set_key(&display->keyboard_state, key, pressed);
    modifiers = cp0_keyboard_state_modifiers(&display->keyboard_state);
    character = pressed
                    ? cp0_input_ascii_character(
                          key, (modifiers & CP0_MODIFIER_SHIFT) != 0U)
                    : 0U;
    bool delivered = cp0_input_queue_push(&display->input_queue, (uint16_t)key,
                                          pressed, false, modifiers, character);
    if (delivered && pressed && !cp0_keyboard_state_is_modifier_key(key))
        (void)cp0_broker_play_key_click();
}

static void handle_keyboard_modifiers(void *data,
                                      struct wl_keyboard *keyboard,
                                      uint32_t serial, uint32_t depressed,
                                      uint32_t latched, uint32_t locked,
                                      uint32_t group) {
    struct cp0_display_state *display = data;
    (void)keyboard;
    (void)serial;
    (void)latched;
    (void)locked;
    (void)group;
    cp0_keyboard_state_set_depressed(&display->keyboard_state, depressed);
}

static void handle_keyboard_repeat_info(void *data,
                                        struct wl_keyboard *keyboard,
                                        int32_t rate, int32_t delay) {
    (void)data;
    (void)keyboard;
    (void)rate;
    (void)delay;
}

static const struct wl_keyboard_listener keyboard_listener = {
    .keymap = handle_keyboard_keymap,
    .enter = handle_keyboard_enter,
    .leave = handle_keyboard_leave,
    .key = handle_keyboard_key,
    .modifiers = handle_keyboard_modifiers,
    .repeat_info = handle_keyboard_repeat_info,
};

static void handle_seat_capabilities(void *data, struct wl_seat *seat,
                                     uint32_t capabilities) {
    struct cp0_display_state *display = data;

    if ((capabilities & WL_SEAT_CAPABILITY_KEYBOARD) != 0U &&
        display->keyboard == NULL) {
        display->keyboard = wl_seat_get_keyboard(seat);
        wl_keyboard_add_listener(display->keyboard, &keyboard_listener,
                                 display);
    } else if ((capabilities & WL_SEAT_CAPABILITY_KEYBOARD) == 0U &&
               display->keyboard != NULL) {
        wl_keyboard_destroy(display->keyboard);
        display->keyboard = NULL;
        display->keyboard_focused = false;
        cp0_keyboard_state_reset(&display->keyboard_state);
        cp0_input_queue_reset(&display->input_queue);
    }
}

static void handle_seat_name(void *data, struct wl_seat *seat,
                             const char *name) {
    (void)data;
    (void)seat;
    (void)name;
}

static const struct wl_seat_listener seat_listener = {
    .capabilities = handle_seat_capabilities,
    .name = handle_seat_name,
};

static void handle_wm_base_ping(void *data, struct xdg_wm_base *wm_base,
                                uint32_t serial) {
    (void)data;
    xdg_wm_base_pong(wm_base, serial);
}

static const struct xdg_wm_base_listener wm_base_listener = {
    .ping = handle_wm_base_ping,
};

static void handle_xdg_surface_configure(void *data,
                                         struct xdg_surface *xdg_surface,
                                         uint32_t serial) {
    struct cp0_display_state *display = data;

    xdg_surface_ack_configure(xdg_surface, serial);
    display->configured = true;
}

static const struct xdg_surface_listener xdg_surface_listener = {
    .configure = handle_xdg_surface_configure,
};

static void handle_toplevel_configure(void *data,
                                      struct xdg_toplevel *toplevel,
                                      int32_t width, int32_t height,
                                      struct wl_array *states) {
    struct cp0_display_state *display = data;
    (void)toplevel;
    (void)states;

    if ((width != 0 && width != (int32_t)CP0_DISPLAY_WIDTH) ||
        (height != 0 && height != (int32_t)CP0_DISPLAY_HEIGHT)) {
        fprintf(stderr, "app-runtime: compositor configured %dx%d, expected "
                        "%ux%u\n",
                width, height, CP0_DISPLAY_WIDTH, CP0_DISPLAY_HEIGHT);
        display->failed = true;
    }
}

static void handle_toplevel_close(void *data, struct xdg_toplevel *toplevel) {
    struct cp0_display_state *display = data;
    (void)toplevel;
    display->failed = true;
}

static const struct xdg_toplevel_listener toplevel_listener = {
    .configure = handle_toplevel_configure,
    .close = handle_toplevel_close,
};

static void handle_buffer_release(void *data, struct wl_buffer *buffer) {
    struct cp0_display_buffer *display_buffer = data;
    (void)buffer;
    display_buffer->busy = false;
}

static const struct wl_buffer_listener buffer_listener = {
    .release = handle_buffer_release,
};

static int create_memfd(void) {
    return (int)syscall(SYS_memfd_create, "cp0-app-frame",
                        MFD_CLOEXEC | MFD_ALLOW_SEALING);
}

static bool create_buffers(struct cp0_display_state *display) {
    struct wl_shm_pool *pool;
    size_t index;
    int descriptor;

    display->mapping_bytes = CP0_FRAME_BYTES * CP0_BUFFER_COUNT;
    descriptor = create_memfd();
    if (descriptor < 0 ||
        ftruncate(descriptor, (off_t)display->mapping_bytes) != 0) {
        if (descriptor >= 0)
            close(descriptor);
        return false;
    }
    display->mapping = mmap(NULL, display->mapping_bytes,
                            PROT_READ | PROT_WRITE, MAP_SHARED, descriptor, 0);
    if (display->mapping == MAP_FAILED) {
        display->mapping = NULL;
        close(descriptor);
        return false;
    }

    pool = wl_shm_create_pool(display->shm, descriptor,
                              (int32_t)display->mapping_bytes);
    close(descriptor);
    if (pool == NULL)
        return false;
    for (index = 0; index < CP0_BUFFER_COUNT; index++) {
        size_t offset = index * CP0_FRAME_BYTES;
        display->buffers[index].pixels =
            (uint32_t *)((uint8_t *)display->mapping + offset);
        display->buffers[index].object = wl_shm_pool_create_buffer(
            pool, (int32_t)offset, (int32_t)CP0_DISPLAY_WIDTH,
            (int32_t)CP0_DISPLAY_HEIGHT,
            (int32_t)(CP0_DISPLAY_WIDTH * sizeof(uint32_t)),
            WL_SHM_FORMAT_XRGB8888);
        if (display->buffers[index].object == NULL) {
            wl_shm_pool_destroy(pool);
            return false;
        }
        wl_buffer_add_listener(display->buffers[index].object,
                               &buffer_listener, &display->buffers[index]);
    }
    wl_shm_pool_destroy(pool);
    return true;
}

bool cp0_display_initialize(const char *app_id, bool immersive) {
    const char *display_title;
    unsigned int roundtrip;

    if (state.display != NULL || !valid_app_id(app_id))
        return false;

    state.content_height =
        immersive ? CP0_DISPLAY_HEIGHT : CP0_STANDARD_CONTENT_HEIGHT;
    state.content_offset_y = immersive ? 0U : CP0_STANDARD_CONTENT_OFFSET_Y;
    state.first_frame = true;
    cp0_input_queue_reset(&state.input_queue);
    state.shadow = calloc(CP0_FRAME_PIXELS, sizeof(uint32_t));
    if (state.shadow == NULL)
        return false;

    state.display = wl_display_connect(NULL);
    if (state.display == NULL)
        goto failure;
    state.registry = wl_display_get_registry(state.display);
    if (state.registry == NULL)
        goto failure;
    wl_registry_add_listener(state.registry, &registry_listener, &state);
    if (wl_display_roundtrip(state.display) < 0 || state.compositor == NULL ||
        state.shm == NULL || state.wm_base == NULL || state.seat == NULL)
        goto failure;
    wl_shm_add_listener(state.shm, &shm_listener, &state);
    wl_seat_add_listener(state.seat, &seat_listener, &state);
    xdg_wm_base_add_listener(state.wm_base, &wm_base_listener, &state);
    if (wl_display_roundtrip(state.display) < 0 || !state.has_xrgb8888 ||
        state.keyboard == NULL || !create_buffers(&state))
        goto failure;

    state.surface = wl_compositor_create_surface(state.compositor);
    if (state.surface == NULL)
        goto failure;
    state.xdg_surface =
        xdg_wm_base_get_xdg_surface(state.wm_base, state.surface);
    if (state.xdg_surface == NULL)
        goto failure;
    xdg_surface_add_listener(state.xdg_surface, &xdg_surface_listener, &state);
    state.toplevel = xdg_surface_get_toplevel(state.xdg_surface);
    if (state.toplevel == NULL)
        goto failure;
    xdg_toplevel_add_listener(state.toplevel, &toplevel_listener, &state);
    xdg_toplevel_set_app_id(state.toplevel, app_id);
    display_title = immersive ? "cardputerzero:immersive"
                              : "cardputerzero:standard";
    xdg_toplevel_set_title(state.toplevel, display_title);
    xdg_toplevel_set_fullscreen(state.toplevel, NULL);
    wl_surface_commit(state.surface);

    for (roundtrip = 0; roundtrip < 4U && !state.configured; roundtrip++) {
        if (wl_display_roundtrip(state.display) < 0)
            goto failure;
    }
    if (!state.configured || state.failed)
        goto failure;
    return true;

failure:
    cp0_display_destroy();
    return false;
}

void cp0_display_destroy(void) {
    size_t index;

    for (index = 0; index < CP0_BUFFER_COUNT; index++) {
        if (state.buffers[index].object != NULL)
            wl_buffer_destroy(state.buffers[index].object);
    }
    if (state.toplevel != NULL)
        xdg_toplevel_destroy(state.toplevel);
    if (state.xdg_surface != NULL)
        xdg_surface_destroy(state.xdg_surface);
    if (state.surface != NULL)
        wl_surface_destroy(state.surface);
    if (state.wm_base != NULL)
        xdg_wm_base_destroy(state.wm_base);
    if (state.keyboard != NULL)
        wl_keyboard_destroy(state.keyboard);
    if (state.seat != NULL)
        wl_seat_destroy(state.seat);
    if (state.shm != NULL)
        wl_shm_destroy(state.shm);
    if (state.compositor != NULL)
        wl_compositor_destroy(state.compositor);
    if (state.registry != NULL)
        wl_registry_destroy(state.registry);
    if (state.display != NULL) {
        (void)wl_display_flush(state.display);
        wl_display_disconnect(state.display);
    }
    if (state.mapping != NULL)
        munmap(state.mapping, state.mapping_bytes);
    free(state.shadow);
    memset(&state, 0, sizeof(state));
}

uint32_t cp0_display_dimensions(void) {
    return (uint32_t)CP0_DISPLAY_WIDTH | ((uint32_t)state.content_height << 16U);
}

static uint16_t read_u16(const uint8_t *bytes) {
    return (uint16_t)bytes[0] | (uint16_t)((uint16_t)bytes[1] << 8U);
}

static bool decode_damage(const uint8_t *bytes, size_t byte_count,
                          struct cp0_damage_rect rectangles[CP0_MAX_DAMAGE_RECTS],
                          size_t *rectangle_count) {
    size_t index;

    if (byte_count % 8U != 0U || byte_count / 8U > CP0_MAX_DAMAGE_RECTS ||
        (byte_count > 0U && bytes == NULL))
        return false;
    *rectangle_count = byte_count / 8U;
    for (index = 0; index < *rectangle_count; index++) {
        const uint8_t *rectangle = &bytes[index * 8U];
        rectangles[index].x = read_u16(&rectangle[0]);
        rectangles[index].y = read_u16(&rectangle[2]);
        rectangles[index].width = read_u16(&rectangle[4]);
        rectangles[index].height = read_u16(&rectangle[6]);
    }
    return cp0_damage_is_valid(rectangles, *rectangle_count,
                               state.content_height);
}

static struct cp0_display_buffer *available_buffer(void) {
    size_t index;

    for (index = 0; index < CP0_BUFFER_COUNT; index++) {
        if (!state.buffers[index].busy)
            return &state.buffers[index];
    }
    return NULL;
}

int cp0_display_present_rgb565(const uint8_t *pixels, size_t pixel_bytes,
                               const uint8_t *damage, size_t damage_bytes) {
    struct cp0_damage_rect rectangles[CP0_MAX_DAMAGE_RECTS];
    struct cp0_display_buffer *buffer;
    size_t rectangle_count;
    size_t expected_bytes =
        (size_t)CP0_DISPLAY_WIDTH * state.content_height * sizeof(uint16_t);
    size_t index;
    struct timespec now;
    uint64_t now_ns;
    bool full_damage;

    if (state.display == NULL || state.failed)
        return -2;
    if (pixels == NULL || pixel_bytes != expected_bytes ||
        !decode_damage(damage, damage_bytes, rectangles, &rectangle_count))
        return -3;
    if (wl_display_dispatch_pending(state.display) < 0)
        return -2;
    buffer = available_buffer();
    if (buffer == NULL)
        return -4;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        return -2;
    now_ns = (uint64_t)now.tv_sec * 1000000000ULL + (uint64_t)now.tv_nsec;
    if (!cp0_frame_pacer_ready(&state.frame_pacer, now_ns))
        return -4;

    full_damage = state.first_frame || rectangle_count == 0U;
    cp0_convert_rgb565(state.shadow, pixels, state.content_height,
                       state.content_offset_y,
                       full_damage ? NULL : rectangles,
                       full_damage ? 0U : rectangle_count);
    memcpy(buffer->pixels, state.shadow, CP0_FRAME_BYTES);
    wl_surface_attach(state.surface, buffer->object, 0, 0);
    if (full_damage) {
        wl_surface_damage_buffer(state.surface, 0, 0,
                                 (int32_t)CP0_DISPLAY_WIDTH,
                                 (int32_t)CP0_DISPLAY_HEIGHT);
    } else {
        for (index = 0; index < rectangle_count; index++) {
            wl_surface_damage_buffer(
                state.surface, (int32_t)rectangles[index].x,
                (int32_t)(rectangles[index].y + state.content_offset_y),
                (int32_t)rectangles[index].width,
                (int32_t)rectangles[index].height);
        }
    }
    buffer->busy = true;
    state.first_frame = false;
    wl_surface_commit(state.surface);
    cp0_frame_pacer_mark_committed(&state.frame_pacer, now_ns);
    if (wl_display_flush(state.display) < 0 && errno != EAGAIN) {
        state.failed = true;
        return -2;
    }
    return 0;
}

int cp0_display_wait(int timeout_milliseconds) {
    struct pollfd descriptor;
    int result;

    if (state.display == NULL || state.failed || timeout_milliseconds < 0 ||
        timeout_milliseconds > 1000)
        return -1;
    while (wl_display_prepare_read(state.display) != 0) {
        if (wl_display_dispatch_pending(state.display) < 0)
            return -1;
    }

    descriptor.fd = wl_display_get_fd(state.display);
    descriptor.events = POLLIN;
    descriptor.revents = 0;
    if (wl_display_flush(state.display) < 0) {
        if (errno != EAGAIN) {
            wl_display_cancel_read(state.display);
            return -1;
        }
        descriptor.events |= POLLOUT;
    }
    result = poll(&descriptor, 1, timeout_milliseconds);
    if (result < 0) {
        wl_display_cancel_read(state.display);
        return errno == EINTR ? 0 : -1;
    }
    if (result > 0 && (descriptor.revents & POLLIN) != 0) {
        if (wl_display_read_events(state.display) < 0)
            return -1;
    } else {
        wl_display_cancel_read(state.display);
    }
    if (result > 0 && (descriptor.revents & POLLOUT) != 0 &&
        wl_display_flush(state.display) < 0 && errno != EAGAIN)
        return -1;
    if ((descriptor.revents & (POLLERR | POLLHUP | POLLNVAL)) != 0)
        return -1;
    if (wl_display_dispatch_pending(state.display) < 0)
        return -1;
    return state.failed ? -1 : 0;
}

int cp0_display_poll_key_event(uint8_t *event_bytes, size_t event_byte_count,
                               int timeout_milliseconds) {
    struct cp0_key_event event;

    if (event_bytes == NULL || event_byte_count != sizeof(event) ||
        timeout_milliseconds < 0 || timeout_milliseconds > 1000)
        return -3;
    if (state.display == NULL || state.failed)
        return -2;
    if (cp0_input_queue_take_overflow(&state.input_queue))
        return -4;
    if (cp0_input_queue_pop(&state.input_queue, &event)) {
        memcpy(event_bytes, &event, sizeof(event));
        return 1;
    }
    if (cp0_display_wait(timeout_milliseconds) != 0)
        return -2;
    if (cp0_input_queue_take_overflow(&state.input_queue))
        return -4;
    if (!cp0_input_queue_pop(&state.input_queue, &event))
        return 0;
    memcpy(event_bytes, &event, sizeof(event));
    return 1;
}
