#include "display.h"
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
#include <sys/socket.h>
#include <sys/syscall.h>
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
    bool has_xrgb8888;
    bool configured;
    bool failed;
    bool first_frame;
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

bool cp0_display_initialize(int socket_fd, const char *app_id, bool immersive) {
    int socket_type = 0;
    socklen_t socket_type_bytes = sizeof(socket_type);
    unsigned int roundtrip;

    if (state.display != NULL || socket_fd != 3 || !valid_app_id(app_id) ||
        getsockopt(socket_fd, SOL_SOCKET, SO_TYPE, &socket_type,
                   &socket_type_bytes) != 0 ||
        socket_type != SOCK_STREAM)
        return false;

    state.content_height =
        immersive ? CP0_DISPLAY_HEIGHT : CP0_STANDARD_CONTENT_HEIGHT;
    state.content_offset_y = immersive ? 0U : CP0_STANDARD_CONTENT_OFFSET_Y;
    state.first_frame = true;
    state.shadow = calloc(CP0_FRAME_PIXELS, sizeof(uint32_t));
    if (state.shadow == NULL)
        return false;

    state.display = wl_display_connect_to_fd(socket_fd);
    if (state.display == NULL)
        goto failure;
    state.registry = wl_display_get_registry(state.display);
    if (state.registry == NULL)
        goto failure;
    wl_registry_add_listener(state.registry, &registry_listener, &state);
    if (wl_display_roundtrip(state.display) < 0 || state.compositor == NULL ||
        state.shm == NULL || state.wm_base == NULL)
        goto failure;
    wl_shm_add_listener(state.shm, &shm_listener, &state);
    xdg_wm_base_add_listener(state.wm_base, &wm_base_listener, &state);
    if (wl_display_roundtrip(state.display) < 0 || !state.has_xrgb8888 ||
        !create_buffers(&state))
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
    xdg_toplevel_set_title(state.toplevel, app_id);
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
