#define _POSIX_C_SOURCE 200809L

#include "cardputerzero-system-shell-client-protocol.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <wayland-client.h>

struct test_state {
    struct cp0_system_shell_v1 *system_shell;
    uint32_t app_token;
    uint32_t activation_failed_token;
};

static void
handle_action(void *data, struct cp0_system_shell_v1 *system_shell,
              uint32_t action)
{
    (void)data;
    (void)system_shell;
    (void)action;
}

static void
handle_app_added(void *data, struct cp0_system_shell_v1 *system_shell,
                 uint32_t token, const char *app_id)
{
    struct test_state *state = data;
    (void)system_shell;
    (void)app_id;
    if (state->app_token == 0)
        state->app_token = token;
}

static void
handle_app_removed(void *data, struct cp0_system_shell_v1 *system_shell,
                   uint32_t token)
{
    struct test_state *state = data;
    (void)system_shell;
    if (state->app_token == token)
        state->app_token = 0;
}

static void
handle_activation_failed(void *data,
                         struct cp0_system_shell_v1 *system_shell,
                         uint32_t token)
{
    struct test_state *state = data;
    (void)system_shell;
    state->activation_failed_token = token;
}

static const struct cp0_system_shell_v1_listener system_shell_listener = {
    .action = handle_action,
    .app_added = handle_app_added,
    .app_removed = handle_app_removed,
    .activation_failed = handle_activation_failed,
};

static void
registry_global(void *data, struct wl_registry *registry, uint32_t name,
                const char *interface, uint32_t version)
{
    struct test_state *state = data;

    if (strcmp(interface, cp0_system_shell_v1_interface.name) != 0 ||
        version < 2)
        return;
    state->system_shell = wl_registry_bind(
        registry, name, &cp0_system_shell_v1_interface, 2);
    cp0_system_shell_v1_add_listener(state->system_shell,
                                     &system_shell_listener, state);
}

static void
registry_global_remove(void *data, struct wl_registry *registry,
                       uint32_t name)
{
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {
    .global = registry_global,
    .global_remove = registry_global_remove,
};

int
main(int argc, char **argv)
{
    static const struct timespec hold_time = {.tv_sec = 30};
    struct test_state state = {0};
    struct wl_display *display = wl_display_connect(NULL);
    struct wl_registry *registry;

    if (display == NULL) {
        fputs("cannot connect to compositor\n", stderr);
        return 2;
    }
    registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, &state);
    if (wl_display_roundtrip(display) < 0 ||
        wl_display_roundtrip(display) < 0 || state.system_shell == NULL) {
        fputs("trusted protocol version 2 is unavailable\n", stderr);
        return 3;
    }
    if (state.app_token == 0) {
        fputs("no application surface was announced\n", stderr);
        return 4;
    }

    if (argc == 2 && strcmp(argv[1], "--stale") == 0) {
        cp0_system_shell_v1_activate_app(state.system_shell, UINT32_MAX);
        if (wl_display_roundtrip(display) < 0 ||
            state.activation_failed_token != UINT32_MAX) {
            fputs("stale token did not return activation_failed\n", stderr);
            return 5;
        }
        puts("stale app token rejected without disconnecting Shell");
        return 0;
    }

    if (argc == 2 && strcmp(argv[1], "--stress") == 0) {
        for (unsigned int iteration = 0; iteration < 200; iteration++) {
            cp0_system_shell_v1_activate_app(state.system_shell,
                                             state.app_token);
            cp0_system_shell_v1_set_visible(state.system_shell, 1);
            if (wl_display_roundtrip(display) < 0) {
                fputs("switching failed during stress test\n", stderr);
                return 5;
            }
        }
        puts("completed 200 app/Shell layer switches");
        return 0;
    }

    cp0_system_shell_v1_activate_app(state.system_shell, state.app_token);
    if (wl_display_flush(display) < 0) {
        fputs("cannot send activate_app request\n", stderr);
        return 5;
    }
    printf("activated app token=%u\n", state.app_token);
    nanosleep(&hold_time, NULL);
    return 0;
}
