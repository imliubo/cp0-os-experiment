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
#define CP0_APP_ID_MAX 47

struct cp0_policy;

struct cp0_surface_watch {
    struct cp0_policy *policy;
    struct weston_surface *surface;
    uint32_t app_token;
    bool announced;
    bool immersive;
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
    struct weston_surface *active_surface;
    struct weston_layer trusted_layer;
    struct weston_layer app_layer;
    struct weston_layer hidden_layer;
    struct wl_listener compositor_destroy_listener;
    struct wl_listener compositor_wake_listener;
    struct wl_listener create_surface_listener;
    struct wl_list surface_watches;
    struct wl_event_source *reassert_idle;
    uint32_t next_app_token;
    uint32_t overlay_mode;
};

static void announce_apps(struct cp0_policy *policy);

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

static struct cp0_surface_watch *
watch_for_surface(struct cp0_policy *policy, struct weston_surface *surface)
{
    struct cp0_surface_watch *watch;

    wl_list_for_each(watch, &policy->surface_watches, link) {
        if (watch->surface == surface)
            return watch;
    }
    return NULL;
}

static struct weston_surface *
desktop_root_surface(struct weston_surface *surface)
{
    struct weston_desktop_surface *desktop_surface;
    struct weston_desktop_surface *parent;

    if (surface == NULL || !weston_surface_is_desktop_surface(surface))
        return NULL;
    desktop_surface = weston_surface_get_desktop_surface(surface);
    while ((parent = weston_desktop_surface_get_parent(desktop_surface)) != NULL)
        desktop_surface = parent;
    return weston_desktop_surface_get_surface(desktop_surface);
}

static void
reassert_layers(struct cp0_policy *policy)
{
    struct cp0_surface_watch *watch;
    struct weston_surface *root;
    struct weston_view *view;

    wl_list_for_each(watch, &policy->surface_watches, link) {
        root = desktop_root_surface(watch->surface);
        if (root == NULL)
            continue;
        view = first_mapped_view(watch->surface);
        if (view == NULL)
            continue;
        if (root == policy->trusted_surface) {
            weston_view_move_to_layer(
                view, policy->overlay_mode !=
                              CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN
                          ? &policy->trusted_layer.view_list
                          : &policy->hidden_layer.view_list);
        } else {
            weston_view_move_to_layer(
                view, policy->overlay_mode !=
                                  CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL &&
                              root == policy->active_surface
                          ? &policy->app_layer.view_list
                          : &policy->hidden_layer.view_list);
        }
    }
}

static void
reassert_idle(void *data)
{
    struct cp0_policy *policy = data;

    policy->reassert_idle = NULL;
    announce_apps(policy);
    reassert_layers(policy);
}

static void
schedule_reassert(struct cp0_policy *policy)
{
    struct wl_event_loop *loop;

    if (policy->reassert_idle != NULL)
        return;
    loop = wl_display_get_event_loop(policy->compositor->wl_display);
    policy->reassert_idle = wl_event_loop_add_idle(loop, reassert_idle, policy);
}

static void
set_overlay_mode(struct cp0_policy *policy, uint32_t mode,
                 struct weston_keyboard *keyboard)
{
    struct weston_view *view;

    policy->overlay_mode = mode;
    reassert_layers(policy);

    if (mode == CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL) {
        view = first_mapped_view(policy->trusted_surface);
        if (view != NULL && keyboard != NULL)
            weston_view_activate_input(view, keyboard->seat,
                                       WESTON_ACTIVATE_FLAG_NONE);
        return;
    }

    view = first_mapped_view(policy->active_surface);
    if (view != NULL && keyboard != NULL)
        weston_view_activate_input(view, keyboard->seat,
                                   WESTON_ACTIVATE_FLAG_NONE);
}

static void
set_visible(struct cp0_policy *policy, bool visible,
            struct weston_keyboard *keyboard)
{
    set_overlay_mode(policy,
                     visible ? CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL
                             : CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN,
                     keyboard);
}

static uint32_t
allocate_app_token(struct cp0_policy *policy)
{
    struct cp0_surface_watch *watch;
    bool collision;

    do {
        policy->next_app_token++;
        if (policy->next_app_token == 0)
            policy->next_app_token++;
        collision = false;
        wl_list_for_each(watch, &policy->surface_watches, link) {
            if (watch->app_token == policy->next_app_token) {
                collision = true;
                break;
            }
        }
    } while (collision);
    return policy->next_app_token;
}

static void
sanitize_app_id(const char *source, char output[CP0_APP_ID_MAX + 1])
{
    static const char fallback[] = "unknown.application";
    size_t index;

    if (source == NULL || source[0] == '\0')
        source = fallback;
    for (index = 0; index < CP0_APP_ID_MAX && source[index] != '\0'; index++) {
        unsigned char byte = (unsigned char)source[index];
        output[index] = (byte >= 'a' && byte <= 'z') ||
                                (byte >= 'A' && byte <= 'Z') ||
                                (byte >= '0' && byte <= '9') || byte == '.' ||
                                byte == '-' || byte == '_'
                            ? (char)byte
                            : '?';
    }
    output[index] = '\0';
}

static void
announce_app(struct cp0_surface_watch *watch)
{
    struct cp0_policy *policy = watch->policy;
    struct weston_desktop_surface *desktop_surface;
    char safe_app_id[CP0_APP_ID_MAX + 1];
    const char *app_id;
    const char *title;

    if (watch->announced || policy->shell_resource == NULL ||
        wl_resource_get_version(policy->shell_resource) < 2 ||
        desktop_root_surface(watch->surface) != watch->surface ||
        watch->surface == policy->trusted_surface ||
        first_mapped_view(watch->surface) == NULL)
        return;

    desktop_surface = weston_surface_get_desktop_surface(watch->surface);
    app_id = weston_desktop_surface_get_app_id(desktop_surface);
    title = weston_desktop_surface_get_title(desktop_surface);
    watch->immersive = title == NULL ||
                       strcmp(title, "cardputerzero:standard") != 0;
    sanitize_app_id(app_id, safe_app_id);
    if (watch->app_token == 0)
        watch->app_token = allocate_app_token(policy);
    cp0_system_shell_v1_send_app_added(policy->shell_resource,
                                       watch->app_token, safe_app_id);
    if (wl_resource_get_version(policy->shell_resource) >= 3) {
        cp0_system_shell_v1_send_app_display_mode(
            policy->shell_resource, watch->app_token,
            watch->immersive ? CP0_SYSTEM_SHELL_V1_DISPLAY_MODE_IMMERSIVE
                             : CP0_SYSTEM_SHELL_V1_DISPLAY_MODE_STANDARD);
    }
    watch->announced = true;
    weston_log("cardputerzero-policy: app token=%u available\n",
               watch->app_token);
}

static void
announce_apps(struct cp0_policy *policy)
{
    struct cp0_surface_watch *watch;

    wl_list_for_each(watch, &policy->surface_watches, link)
        announce_app(watch);
}

static void
withdraw_app(struct cp0_surface_watch *watch)
{
    struct cp0_policy *policy = watch->policy;

    if (!watch->announced)
        return;
    if (policy->shell_resource != NULL &&
        wl_resource_get_version(policy->shell_resource) >= 2) {
        cp0_system_shell_v1_send_app_removed(policy->shell_resource,
                                             watch->app_token);
    }
    watch->announced = false;
    weston_log("cardputerzero-policy: app token=%u removed\n",
               watch->app_token);
    watch->app_token = 0;
}

static void
surface_committed(struct wl_listener *listener, void *data)
{
    struct cp0_surface_watch *watch =
        wl_container_of(listener, watch, commit_listener);
    (void)data;

    if (watch->surface != watch->policy->trusted_surface &&
        desktop_root_surface(watch->surface) == watch->surface) {
        if (first_mapped_view(watch->surface) != NULL) {
            announce_app(watch);
        } else if (watch->announced) {
            if (watch->policy->active_surface == watch->surface) {
                watch->policy->active_surface = NULL;
                set_overlay_mode(
                    watch->policy, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL,
                    first_keyboard(watch->policy->compositor));
                if (watch->policy->shell_resource != NULL) {
                    cp0_system_shell_v1_send_action(
                        watch->policy->shell_resource,
                        CP0_SYSTEM_SHELL_V1_ACTION_HOME);
                }
            }
            withdraw_app(watch);
        }
    }
    schedule_reassert(watch->policy);
}

static void
surface_destroyed(struct wl_listener *listener, void *data)
{
    struct cp0_surface_watch *watch =
        wl_container_of(listener, watch, destroy_listener);
    struct cp0_policy *policy = watch->policy;
    bool active_lost = policy->active_surface == watch->surface;
    (void)data;

    withdraw_app(watch);
    if (policy->trusted_surface == watch->surface) {
        policy->trusted_surface = NULL;
        policy->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    }
    if (active_lost)
        policy->active_surface = NULL;

    wl_list_remove(&watch->commit_listener.link);
    wl_list_remove(&watch->destroy_listener.link);
    wl_list_remove(&watch->link);
    free(watch);

    if (active_lost) {
        set_overlay_mode(policy, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL,
                         first_keyboard(policy->compositor));
        if (policy->shell_resource != NULL) {
            cp0_system_shell_v1_send_action(
                policy->shell_resource, CP0_SYSTEM_SHELL_V1_ACTION_HOME);
        }
    } else {
        schedule_reassert(policy);
    }
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
    struct cp0_surface_watch *watch;

    if (policy == NULL || policy->shell_resource != resource)
        return;
    policy->shell_resource = NULL;
    policy->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    wl_list_for_each(watch, &policy->surface_watches, link)
        watch->announced = false;
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
    struct cp0_surface_watch *watch;
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
    policy->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    watch = watch_for_surface(policy, surface);
    if (watch != NULL)
        withdraw_app(watch);
    schedule_reassert(policy);
    announce_apps(policy);
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

static void
shell_set_overlay_mode(struct wl_client *client, struct wl_resource *resource,
                       uint32_t mode)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);
    (void)client;

    if (mode > CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN ||
        (mode != CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL &&
         policy->active_surface == NULL)) {
        wl_resource_post_error(resource,
                               CP0_SYSTEM_SHELL_V1_ERROR_INVALID_VISIBILITY,
                               "overlay mode is invalid for current state");
        return;
    }
    set_overlay_mode(policy, mode, first_keyboard(policy->compositor));
}

static void
shell_sleep_display(struct wl_client *client, struct wl_resource *resource)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);
    (void)client;

    set_overlay_mode(policy, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL,
                     first_keyboard(policy->compositor));
    weston_compositor_sleep(policy->compositor);
    weston_log("cardputerzero-policy: display sleeping\n");
}

static void
shell_activate_app(struct wl_client *client, struct wl_resource *resource,
                   uint32_t token)
{
    struct cp0_policy *policy = wl_resource_get_user_data(resource);
    struct cp0_surface_watch *watch;
    (void)client;

    wl_list_for_each(watch, &policy->surface_watches, link) {
        if (watch->announced && watch->app_token == token &&
            first_mapped_view(watch->surface) != NULL) {
            policy->active_surface = watch->surface;
            set_overlay_mode(
                policy,
                watch->immersive ? CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_HIDDEN
                                 : CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS,
                first_keyboard(policy->compositor));
            weston_log("cardputerzero-policy: app token=%u activated mode=%s\n",
                       token, watch->immersive ? "immersive" : "standard");
            return;
        }
    }

    cp0_system_shell_v1_send_activation_failed(resource, token);
    weston_log("cardputerzero-policy: app token=%u activation failed\n",
               token);
}

static const struct cp0_system_shell_v1_interface shell_implementation = {
    .destroy = shell_destroy_request,
    .register_surface = shell_register_surface,
    .set_visible = shell_set_visible,
    .activate_app = shell_activate_app,
    .set_overlay_mode = shell_set_overlay_mode,
    .sleep_display = shell_sleep_display,
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
                                  version < 3 ? version : 3, id);
    if (resource == NULL) {
        wl_client_post_no_memory(client);
        return;
    }
    policy->shell_resource = resource;
    wl_resource_set_implementation(resource, &shell_implementation, policy,
                                   shell_resource_destroyed);
    announce_apps(policy);
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
    set_overlay_mode(policy, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL, keyboard);
    cp0_system_shell_v1_send_action(policy->shell_resource, action);
}

static void
add_system_binding(struct cp0_policy *policy, uint32_t key)
{
    weston_compositor_add_key_binding(policy->compositor, key, 0,
                                      system_key_binding, policy);
}

static void
compositor_woke(struct wl_listener *listener, void *data)
{
    struct cp0_policy *policy =
        wl_container_of(listener, policy, compositor_wake_listener);
    (void)data;

    set_overlay_mode(policy, CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL,
                     first_keyboard(policy->compositor));
    if (policy->shell_resource != NULL) {
        cp0_system_shell_v1_send_action(policy->shell_resource,
                                        CP0_SYSTEM_SHELL_V1_ACTION_HOME);
    }
    weston_log("cardputerzero-policy: display awake\n");
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
    wl_list_for_each_safe(watch, next, &policy->surface_watches, link) {
        wl_list_remove(&watch->commit_listener.link);
        wl_list_remove(&watch->destroy_listener.link);
        wl_list_remove(&watch->link);
        free(watch);
    }
    wl_list_remove(&policy->create_surface_listener.link);
    wl_list_remove(&policy->compositor_wake_listener.link);
    wl_list_remove(&policy->compositor_destroy_listener.link);
    if (policy->global != NULL)
        wl_global_destroy(policy->global);
    weston_layer_fini(&policy->trusted_layer);
    weston_layer_fini(&policy->app_layer);
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
    policy->overlay_mode = CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_FULL;
    wl_list_init(&policy->surface_watches);

    weston_layer_init(&policy->trusted_layer, compositor);
    weston_layer_init(&policy->app_layer, compositor);
    weston_layer_init(&policy->hidden_layer, compositor);
    weston_layer_set_position(&policy->trusted_layer,
                              WESTON_LAYER_POSITION_TOP_UI);
    weston_layer_set_position(&policy->app_layer,
                              WESTON_LAYER_POSITION_NORMAL);
    weston_layer_set_position(&policy->hidden_layer,
                              WESTON_LAYER_POSITION_HIDDEN);

    policy->create_surface_listener.notify = surface_created;
    wl_signal_add(&compositor->create_surface_signal,
                  &policy->create_surface_listener);
    policy->compositor_destroy_listener.notify = policy_destroyed;
    wl_signal_add(&compositor->destroy_signal,
                  &policy->compositor_destroy_listener);
    policy->compositor_wake_listener.notify = compositor_woke;
    wl_signal_add(&compositor->wake_signal,
                  &policy->compositor_wake_listener);

    policy->global = wl_global_create(
        compositor->wl_display, &cp0_system_shell_v1_interface, 3, policy,
        bind_system_shell);
    if (policy->global == NULL) {
        wl_list_remove(&policy->create_surface_listener.link);
        wl_list_remove(&policy->compositor_wake_listener.link);
        wl_list_remove(&policy->compositor_destroy_listener.link);
        weston_layer_fini(&policy->trusted_layer);
        weston_layer_fini(&policy->app_layer);
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
