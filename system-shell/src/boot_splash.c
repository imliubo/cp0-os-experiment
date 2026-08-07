#define _POSIX_C_SOURCE 200809L

#include "xdg-shell-client-protocol.h"

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <wayland-client.h>

#define SPLASH_WIDTH 320
#define SPLASH_HEIGHT 170
#define SPLASH_PIXELS ((size_t)SPLASH_WIDTH * (size_t)SPLASH_HEIGHT)
#define SPLASH_RGB565_BYTES (SPLASH_PIXELS * 2U)
#define SPLASH_XRGB_BYTES (SPLASH_PIXELS * 4U)
#define SPLASH_PATH "/usr/share/cardputerzero/boot/splash.rgb565"
#define READY_PATH "/run/cardputerzero/boot-splash-ready"

struct boot_splash {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct xdg_wm_base *wm_base;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *xdg_toplevel;
    struct wl_buffer *buffer;
    struct wl_callback *frame_callback;
    uint32_t *pixels;
    bool configured;
    bool committed;
};

static volatile sig_atomic_t stop_requested;
static unsigned int shm_serial;

static void handle_signal(int signal_number)
{
    (void)signal_number;
    stop_requested = 1;
}

static int create_anonymous_file(size_t size)
{
    char name[64];
    int fd = -1;

    for (int attempt = 0; attempt < 100; attempt++) {
        snprintf(name, sizeof(name), "/cp0-boot-splash-%ld-%u",
                 (long)getpid(), shm_serial++);
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
    if (ftruncate(fd, (off_t)size) != 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static bool load_splash(uint32_t *pixels)
{
    uint8_t source[SPLASH_RGB565_BYTES];
    struct stat metadata;
    size_t offset = 0;
    int fd = open(SPLASH_PATH, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);

    if (fd < 0 || fstat(fd, &metadata) != 0 ||
        !S_ISREG(metadata.st_mode) ||
        metadata.st_size != (off_t)sizeof(source)) {
        if (fd >= 0)
            close(fd);
        return false;
    }
    while (offset < sizeof(source)) {
        ssize_t count = read(fd, source + offset, sizeof(source) - offset);
        if (count <= 0) {
            close(fd);
            return false;
        }
        offset += (size_t)count;
    }
    close(fd);

    for (size_t index = 0; index < SPLASH_PIXELS; index++) {
        uint16_t rgb565 = (uint16_t)source[index * 2U] |
                          (uint16_t)source[index * 2U + 1U] << 8U;
        uint32_t red = (rgb565 >> 11U) & 0x1fU;
        uint32_t green = (rgb565 >> 5U) & 0x3fU;
        uint32_t blue = rgb565 & 0x1fU;

        red = (red << 3U) | (red >> 2U);
        green = (green << 2U) | (green >> 4U);
        blue = (blue << 3U) | (blue >> 2U);
        pixels[index] = 0xff000000U | (red << 16U) | (green << 8U) | blue;
    }
    return true;
}

static bool create_buffer(struct boot_splash *splash)
{
    int stride = SPLASH_WIDTH * 4;
    int fd = create_anonymous_file(SPLASH_XRGB_BYTES);
    struct wl_shm_pool *pool;

    if (fd < 0)
        return false;
    splash->pixels = mmap(NULL, SPLASH_XRGB_BYTES, PROT_READ | PROT_WRITE,
                          MAP_SHARED, fd, 0);
    if (splash->pixels == MAP_FAILED) {
        splash->pixels = NULL;
        close(fd);
        return false;
    }
    if (!load_splash(splash->pixels)) {
        munmap(splash->pixels, SPLASH_XRGB_BYTES);
        splash->pixels = NULL;
        close(fd);
        return false;
    }
    pool = wl_shm_create_pool(splash->shm, fd, (int)SPLASH_XRGB_BYTES);
    splash->buffer = wl_shm_pool_create_buffer(
        pool, 0, SPLASH_WIDTH, SPLASH_HEIGHT, stride,
        WL_SHM_FORMAT_XRGB8888);
    wl_shm_pool_destroy(pool);
    close(fd);
    return splash->buffer != NULL;
}

static void mark_ready(void)
{
    static const char ready[] = "ready\n";
    int fd = open(READY_PATH,
                  O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC | O_NOFOLLOW,
                  0660);

    if (fd < 0)
        return;
    (void)write(fd, ready, sizeof(ready) - 1U);
    close(fd);
}

static void handle_frame_done(void *data, struct wl_callback *callback,
                              uint32_t time)
{
    struct boot_splash *splash = data;
    (void)time;

    wl_callback_destroy(callback);
    splash->frame_callback = NULL;
    mark_ready();
}

static const struct wl_callback_listener frame_listener = {
    .done = handle_frame_done,
};

static void commit_splash(struct boot_splash *splash)
{
    if (!splash->configured || splash->committed)
        return;
    splash->frame_callback = wl_surface_frame(splash->surface);
    wl_callback_add_listener(splash->frame_callback, &frame_listener, splash);
    wl_surface_attach(splash->surface, splash->buffer, 0, 0);
    wl_surface_damage(splash->surface, 0, 0, SPLASH_WIDTH, SPLASH_HEIGHT);
    wl_surface_commit(splash->surface);
    splash->committed = true;
}

static void handle_xdg_surface_configure(void *data,
                                         struct xdg_surface *xdg_surface,
                                         uint32_t serial)
{
    struct boot_splash *splash = data;

    xdg_surface_ack_configure(xdg_surface, serial);
    splash->configured = true;
    commit_splash(splash);
}

static const struct xdg_surface_listener xdg_surface_listener = {
    .configure = handle_xdg_surface_configure,
};

static void handle_toplevel_configure(void *data,
                                      struct xdg_toplevel *xdg_toplevel,
                                      int32_t width, int32_t height,
                                      struct wl_array *states)
{
    (void)data;
    (void)xdg_toplevel;
    (void)width;
    (void)height;
    (void)states;
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
    struct boot_splash *splash = data;
    (void)version;

    if (strcmp(interface, wl_compositor_interface.name) == 0) {
        splash->compositor = wl_registry_bind(
            registry, name, &wl_compositor_interface, 1);
    } else if (strcmp(interface, wl_shm_interface.name) == 0) {
        splash->shm = wl_registry_bind(registry, name, &wl_shm_interface, 1);
    } else if (strcmp(interface, xdg_wm_base_interface.name) == 0) {
        splash->wm_base = wl_registry_bind(
            registry, name, &xdg_wm_base_interface, 1);
        xdg_wm_base_add_listener(splash->wm_base, &wm_base_listener, splash);
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

static bool connect_splash(struct boot_splash *splash)
{
    splash->display = wl_display_connect(NULL);
    if (splash->display == NULL)
        return false;
    splash->registry = wl_display_get_registry(splash->display);
    wl_registry_add_listener(splash->registry, &registry_listener, splash);
    if (wl_display_roundtrip(splash->display) < 0 ||
        splash->compositor == NULL || splash->shm == NULL ||
        splash->wm_base == NULL || !create_buffer(splash))
        return false;

    splash->surface = wl_compositor_create_surface(splash->compositor);
    splash->xdg_surface =
        xdg_wm_base_get_xdg_surface(splash->wm_base, splash->surface);
    xdg_surface_add_listener(splash->xdg_surface, &xdg_surface_listener,
                             splash);
    splash->xdg_toplevel =
        xdg_surface_get_toplevel(splash->xdg_surface);
    xdg_toplevel_add_listener(splash->xdg_toplevel, &toplevel_listener,
                              splash);
    xdg_toplevel_set_title(splash->xdg_toplevel,
                           "CardputerZero Boot Splash");
    xdg_toplevel_set_app_id(splash->xdg_toplevel,
                            "os.cardputerzero.boot-splash");
    xdg_toplevel_set_fullscreen(splash->xdg_toplevel, NULL);
    wl_surface_commit(splash->surface);
    return true;
}

static void destroy_splash(struct boot_splash *splash)
{
    if (splash->frame_callback != NULL)
        wl_callback_destroy(splash->frame_callback);
    if (splash->xdg_toplevel != NULL)
        xdg_toplevel_destroy(splash->xdg_toplevel);
    if (splash->xdg_surface != NULL)
        xdg_surface_destroy(splash->xdg_surface);
    if (splash->surface != NULL)
        wl_surface_destroy(splash->surface);
    if (splash->buffer != NULL)
        wl_buffer_destroy(splash->buffer);
    if (splash->pixels != NULL)
        munmap(splash->pixels, SPLASH_XRGB_BYTES);
    if (splash->wm_base != NULL)
        xdg_wm_base_destroy(splash->wm_base);
    if (splash->shm != NULL)
        wl_shm_destroy(splash->shm);
    if (splash->compositor != NULL)
        wl_compositor_destroy(splash->compositor);
    if (splash->registry != NULL)
        wl_registry_destroy(splash->registry);
    if (splash->display != NULL)
        wl_display_disconnect(splash->display);
}

int main(void)
{
    struct boot_splash splash = {0};
    struct sigaction action = {
        .sa_handler = handle_signal,
    };
    int result = EXIT_SUCCESS;

    sigemptyset(&action.sa_mask);
    sigaction(SIGINT, &action, NULL);
    sigaction(SIGTERM, &action, NULL);
    if (!connect_splash(&splash)) {
        fprintf(stderr, "boot-splash: cannot connect or load splash: %s\n",
                strerror(errno));
        result = EXIT_FAILURE;
    } else {
        while (!stop_requested && wl_display_dispatch(splash.display) >= 0)
            ;
        if (!stop_requested)
            result = EXIT_FAILURE;
    }
    destroy_splash(&splash);
    return result;
}
