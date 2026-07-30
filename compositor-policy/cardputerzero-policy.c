#define _POSIX_C_SOURCE 200809L

#include "cardputerzero-system-shell-server-protocol.h"

#include <libweston/desktop.h>
#include <libweston/libweston.h>
#include <linux/input-event-codes.h>
#include <pwd.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <wayland-server-core.h>

#define CP0_SHELL_APP_ID "os.cardputerzero.shell"
#define CP0_SHELL_USER "cp0-shell"

struct cp0_policy;

struct cp0_surface_watch {
    struct cp0_policy *policy;
    struct weston_surface *surface;
    struct wl_listener commit_listener;
    struct wl_listener destroy_listener;
    struct wl_list link;
};

struct cp0_policy {
    struct weston_compositor *compositor;
    uid_t trusted_uid;
    struct wl_global *global;
    struct wl_resource *shell_resource;
    struct weston_surface *trusted_surface;
    struct weston_surface *return_focus;
    struct weston_layer trusted_layer;
    struct weston_layer hidden_layer;
    struct wl_listener compositor_destroy_listener;
    struct wl_listener create_surface_listener;
    struct wl_list surface_watches;
    struct wl_event_source *reassert_idle;
    bool visible;
};

static struct weston_view *
first_mapped_view(struct weston_surface *surface)
{
    struct weston_view *view;

    if (surface == NULL)
        return NULL;
    wl_list_for_each(view, &surface->views, surface_link) {
        if (weston_view_is_mapped(view))
            return view;
    }
    return NULL;
}

static struct weston_keyboard *
first_keyboard(struct weston_compositor *compositor)
{
    struct weston_seat *seat;

    if (wl_list_empty(&compositor->seat_list))
        return NULL;
    seat = wl_container_of(compositor->seat_list.next, seat, link);
    return weston_seat_get_keyboard(seat);
}

static void
clear_return_focus(struct cp0_policy *policy)
{
    if (policy->return_focus != NULL)
        weston_surface_unref(policy->return_focus);
    policy->return_focus = NULL;
}

static void
reassert_trusted_layer(struct cp0_policy *policy)
{
    struct weston_view *view = first_mapped_view(policy->trusted_surface);

    if (view == NULL)
        return;
    weston_view_move_to_layer(
        view, policy->visible ? &policy->trusted_layer.view_list
                              : &policy->hidden_layer.view_list);
}

static void
reassert_idle(void *data)
{
    struct cp0_policy *policy = data;

    policy->reassert_idle = NULL;
    reassert_trusted_layer(policy);
}

static void
schedule_reassert(struct cp0_policy *policy)
{
    struct wl_event_loop *loop;

    if (policy->trusted_surface == NULL || policy->reassert_idle != NULL)
        return;
    loop = wl_display_get_event_loop(policy->compositor->wl_display);
    policy->reassert_idle = wl_event_loop_add_idle(loop, reassert_idle, policy);
}

static void
set_visible(struct cp0_policy *policy, bool visible,
            struct weston_keyboard *keyboard)
{
    struct weston_view *view;

    if (visible && keyboard != NULL && keyboard->focus != NULL &&
        keyboard->focus != policy->trusted_surface) {
        clear_return_focus(policy);
        policy->return_focus = weston_surface_ref(keyboard->focus);
    }

    policy->visible = visible;
    reassert_trusted_layer(policy);

    if (visible) {
        view = first_mapped_view(policy->trusted_surface);
        if (view != NULL && keyboard != NULL)
            weston_view_activate_input(view, keyboard->seat,
                                       WESTON_ACTIVATE_FLAG_NONE);
        return;
    }

    view = first_mapped_view(policy->return_focus);
    if (view != NULL && keyboard != NULL)
        weston_view_activate_input(view, keyboard->seat,
                                   WESTON_ACTIVATE_FLAG_NONE);
    clear_return_focus(policy);
}

static void
surface_committed(struct wl_listener *listener, void *data)
{
    struct cp0_surface_watch *watch =
        wl_container_of(listener, watch, commit_listener);
    (void)data;
    schedule_reassert(watch->policy);
}

static void
surface_destroyed(struct wl_listener *listener, void *data)
{
    struct cp0_surface_watch *watch =
        wl_container_of(listener, watch, destroy_listener);
    struct cp0_policy *policy = watch->policy;
    (void)data;

    if (policy->trusted_surface == watch->surface) {
        policy->trusted_surface = NULL;
        policy->visible = false;
    }
    if (policy->return_focus == watch->surface)
        clear_return_focus(policy);

    wl_list_remove(&watch->commit_listener.link);
    wl_list_remove(&watch->destroy_listener.link);
    wl_list_remove(&watch->link);
    free(watch);
}

static void
surface_created(struct wl_listener *listener, void *data)
{
    struct cp0_policy *policy =
        wl_container_of(listener, policy, create_surface_listener);
    struct weston_surface *surface = data;
    struct cp0_surface_watch *watch = calloc(1, sizeof(*watch));

    if (watch == NULL) {
        weston_log("cardputerzero-policy: cannot watch a new surface\n");
        return;
    }
    watch->policy = policy;
    watch->surface = surface;
    watch->commit_listener.notify = surface_committed;
    watch->destroy_listener.notify = surface_destroyed;
    wl_signal_add(&surface->commit_signal, &watch->commit_listener);
    wl_signal_add(&surface->destroy_signal, &watch->destroy_listener);
    wl_list_insert(&policy->surface_watches, &watch->link);
}

static void
shell_resource_destroyed(struct wl_resource *resource)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);

    if (policy == NULL || policy->shell_resource != resource)
        return;
    policy->shell_resource = NULL;
    policy->visible = false;
    schedule_reassert(policy);
}

static void
shell_destroy_request(struct wl_client *client, struct wl_resource *resource)
{
    (void)client;
    wl_resource_destroy(resource);
}

static void
shell_register_surface(struct wl_client *client, struct wl_resource *resource,
                       struct wl_resource *surface_resource)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);
    struct weston_surface *surface;
    struct weston_desktop_surface *desktop_surface;
    const char *app_id;

    if (policy->trusted_surface != NULL) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_ALREADY_REGISTERED,
                               "trusted System Shell surface already registered");
        return;
    }
    if (wl_resource_get_client(surface_resource) != client) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_INVALID_SURFACE,
                               "surface belongs to a different client");
        return;
    }

    surface = wl_resource_get_user_data(surface_resource);
    if (surface == NULL || !weston_surface_is_desktop_surface(surface)) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_INVALID_SURFACE,
                               "surface is not an xdg desktop surface");
        return;
    }
    desktop_surface = weston_surface_get_desktop_surface(surface);
    app_id = weston_desktop_surface_get_app_id(desktop_surface);
    if (app_id == NULL || strcmp(app_id, CP0_SHELL_APP_ID) != 0) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_INVALID_SURFACE,
                               "unexpected System Shell app-id");
        return;
    }

    policy->trusted_surface = surface;
    policy->visible = true;
    schedule_reassert(policy);
    weston_log("cardputerzero-policy: trusted System Shell registered\n");
}

static void
shell_set_visible(struct wl_client *client, struct wl_resource *resource,
                  uint32_t visible)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);
    (void)client;

    if (visible > 1) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_INVALID_VISIBILITY,
                               "visibility must be zero or one");
        return;
    }
    set_visible(policy, visible == 1, first_keyboard(policy->compositor));
}

static const struct cp0_system_shell_v1_interface shell_implementation = {
    .destroy = shell_destroy_request,
    .register_surface = shell_register_surface,
    .set_visible = shell_set_visible,
};

static void
bind_system_shell(struct wl_client *client, void *data, uint32_t version,
                  uint32_t id)
{
    struct cp0_policy *policy = data;
    struct wl_resource *resource;
    uid_t uid;

    wl_client_get_credentials(client, NULL, &uid, NULL);
    if (uid != policy->trusted_uid) {
        wl_client_post_implementation_error(
            client, "cardputerzero system shell protocol is restricted");
        return;
    }
    if (policy->shell_resource != NULL) {
        wl_client_post_implementation_error(
            client, "cardputerzero system shell is already bound");
        return;
    }

    resource = wl_resource_create(client, &cp0_system_shell_v1_interface,
                                  version < 1 ? version : 1, id);
    if (resource == NULL) {
        wl_client_post_no_memory(client);
        return;
    }
    policy->shell_resource = resource;
    wl_resource_set_implementation(resource, &shell_implementation, policy,
                                   shell_resource_destroyed);
}

static uint32_t
action_for_key(uint32_t key)
{
    switch (key) {
    case KEY_HOME:
    case KEY_HOMEPAGE:
    case KEY_F1:
        return CP0_SYSTEM_SHELL_V1_ACTION_HOME;
    case KEY_ESC:
    case KEY_F2:
        return CP0_SYSTEM_SHELL_V1_ACTION_BACK;
    case KEY_F3:
        return CP0_SYSTEM_SHELL_V1_ACTION_TASKS;
    case KEY_POWER:
    case KEY_F4:
        return CP0_SYSTEM_SHELL_V1_ACTION_POWER;
    default:
        return UINT32_MAX;
    }
}

static void
system_key_binding(struct weston_keyboard *keyboard,
                   const struct timespec *time, uint32_t key, void *data)
{
    struct cp0_policy *policy = data;
    uint32_t action = action_for_key(key);
    (void)time;

    if (action == UINT32_MAX || policy->shell_resource == NULL ||
        policy->trusted_surface == NULL)
        return;
    set_visible(policy, true, keyboard);
    cp0_system_shell_v1_send_action(policy->shell_resource, action);
}

static void
add_system_binding(struct cp0_policy *policy, uint32_t key)
{
    weston_compositor_add_key_binding(policy->compositor, key, 0,
                                      system_key_binding, policy);
}

static void
policy_destroyed(struct wl_listener *listener, void *data)
{
    struct cp0_policy *policy =
        wl_container_of(listener, policy, compositor_destroy_listener);
    struct cp0_surface_watch *watch, *next;
    (void)data;

    if (policy->reassert_idle != NULL)
        wl_event_source_remove(policy->reassert_idle);
    clear_return_focus(policy);
    wl_list_for_each_safe(watch, next, &policy->surface_watches, link) {
        wl_list_remove(&watch->commit_listener.link);
        wl_list_remove(&watch->destroy_listener.link);
        wl_list_remove(&watch->link);
        free(watch);
    }
    wl_list_remove(&policy->create_surface_listener.link);
    wl_list_remove(&policy->compositor_destroy_listener.link);
    if (policy->global != NULL)
        wl_global_destroy(policy->global);
    weston_layer_fini(&policy->trusted_layer);
    weston_layer_fini(&policy->hidden_layer);
    free(policy);
}

WL_EXPORT int
wet_module_init(struct weston_compositor *compositor, int *argc, char *argv[])
{
    struct cp0_policy *policy;
    struct passwd *shell_user;
    (void)argc;
    (void)argv;

    shell_user = getpwnam(CP0_SHELL_USER);
    if (shell_user == NULL) {
        weston_log("cardputerzero-policy: user %s does not exist\n",
                   CP0_SHELL_USER);
        return -1;
    }

    policy = calloc(1, sizeof(*policy));
    if (policy == NULL)
        return -1;
    policy->compositor = compositor;
    policy->trusted_uid = shell_user->pw_uid;
    policy->visible = false;
    wl_list_init(&policy->surface_watches);

    weston_layer_init(&policy->trusted_layer, compositor);
    weston_layer_init(&policy->hidden_layer, compositor);
    weston_layer_set_position(&policy->trusted_layer,
                              WESTON_LAYER_POSITION_TOP_UI);
    weston_layer_set_position(&policy->hidden_layer,
                              WESTON_LAYER_POSITION_HIDDEN);

    policy->create_surface_listener.notify = surface_created;
    wl_signal_add(&compositor->create_surface_signal,
                  &policy->create_surface_listener);
    policy->compositor_destroy_listener.notify = policy_destroyed;
    wl_signal_add(&compositor->destroy_signal,
                  &policy->compositor_destroy_listener);

    policy->global = wl_global_create(
        compositor->wl_display, &cp0_system_shell_v1_interface, 1, policy,
        bind_system_shell);
    if (policy->global == NULL) {
        wl_list_remove(&policy->create_surface_listener.link);
        wl_list_remove(&policy->compositor_destroy_listener.link);
        weston_layer_fini(&policy->trusted_layer);
        weston_layer_fini(&policy->hidden_layer);
        free(policy);
        return -1;
    }

    add_system_binding(policy, KEY_HOME);
    add_system_binding(policy, KEY_HOMEPAGE);
    add_system_binding(policy, KEY_F1);
    add_system_binding(policy, KEY_ESC);
    add_system_binding(policy, KEY_F2);
    add_system_binding(policy, KEY_F3);
    add_system_binding(policy, KEY_POWER);
    add_system_binding(policy, KEY_F4);

    weston_log("cardputerzero-policy: trusted uid=%u policy active\n",
               (unsigned int)policy->trusted_uid);
    return 0;
}
