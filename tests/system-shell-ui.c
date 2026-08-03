#include "cp0_ui.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define GUARD 0x5aa55aa5u
#define GREEN 0x0035d07fu

struct guarded_frame {
    uint32_t before;
    uint32_t pixels[CP0_UI_WIDTH * CP0_UI_HEIGHT];
    uint32_t after;
};

static uint32_t pixel(const struct guarded_frame *frame, int x, int y)
{
    return frame->pixels[y * CP0_UI_WIDTH + x];
}

static void render(const struct cp0_ui *ui, struct guarded_frame *frame)
{
    frame->before = GUARD;
    frame->after = GUARD;
    cp0_ui_render(ui, frame->pixels, CP0_UI_WIDTH, CP0_UI_HEIGHT,
                  CP0_UI_WIDTH);
    assert(frame->before == GUARD);
    assert(frame->after == GUARD);
}

static void write_ppm(const char *path, const struct guarded_frame *frame)
{
    FILE *output = fopen(path, "wb");
    assert(output != NULL);
    fprintf(output, "P6\n%d %d\n255\n", CP0_UI_WIDTH, CP0_UI_HEIGHT);
    for (size_t index = 0; index < CP0_UI_WIDTH * CP0_UI_HEIGHT; index++) {
        uint32_t value = frame->pixels[index];
        fputc((int)((value >> 16) & 0xff), output);
        fputc((int)((value >> 8) & 0xff), output);
        fputc((int)(value & 0xff), output);
    }
    assert(fclose(output) == 0);
}

static void write_snapshot(const char *directory, const char *name,
                           struct cp0_ui *ui, struct guarded_frame *frame)
{
    char path[1024];
    int length = snprintf(path, sizeof(path), "%s/%s.ppm", directory, name);
    assert(length > 0 && (size_t)length < sizeof(path));
    render(ui, frame);
    write_ppm(path, frame);
}

static void write_snapshots(const char *directory, struct cp0_ui *ui,
                            struct guarded_frame *frame)
{
    static const struct cp0_ui_catalog_app apps[] = {
        {.running = false,
         .immersive = false,
         .app_id = "dev.cardputerzero.first",
         .name = "First Card",
         .version = "1.2.3",
         .installed_at_unix_seconds = 1722470400,
         .package_bytes = 3U * 1024U * 1024U,
         .data_bytes = 512U * 1024U,
         .permissions = (1U << 2) | (1U << 5)},
        {.running = true,
         .immersive = true,
         .app_id = "dev.cardputerzero.second",
         .name = "Second Card",
         .version = "2.0.0",
         .installed_at_unix_seconds = 1722556800,
         .package_bytes = 8U * 1024U * 1024U,
         .data_bytes = 2U * 1024U * 1024U,
         .permissions = (1U << 0) | (1U << 6)},
    };
    cp0_ui_init(ui);
    cp0_ui_set_local_simulation(ui, true);
    cp0_ui_set_status(ui, "12:34", true, 73);
    write_snapshot(directory, "home", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "apps-empty", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);

    static const struct cp0_ui_store_catalog_app store_apps[] = {
        {.package_bytes = 4096,
         .permissions = (1U << 2) | (1U << 5),
         .progress_percent = 0,
         .state = CP0_UI_STORE_AVAILABLE,
         .app_id = "dev.cardputerzero.camera",
         .name = "Camera Notes",
         .version = "1.2.0",
         .summary = "Capture a photo and attach it to a field note"},
        {.package_bytes = 8192,
         .permissions = 1U << 6,
         .progress_percent = 38,
         .state = CP0_UI_STORE_DOWNLOADING,
         .app_id = "dev.cardputerzero.notify",
         .name = "Notify",
         .version = "2.0.1",
         .summary = "Deliver reviewed status notifications"},
    };
    cp0_ui_sync_store_catalog(ui, store_apps, 2, false, false);
    static const struct cp0_ui_store_editorial_collection store_collections[] = {
        {.title = "Small-screen essentials",
         .apps = &store_apps[1],
         .app_count = 1},
    };
    static const struct cp0_ui_store_editorial store_editorial = {
        .headline = "Made for your CardputerZero",
        .featured = &store_apps[0],
        .collections = store_collections,
        .collection_count = 1,
    };
    cp0_ui_sync_store_today(ui, &store_editorial);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "store-today", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "store-today-collection", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_sync_store_browse(ui, 0, 2, false, 0, store_apps, 2, false);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "store", ui, frame);
    assert(cp0_ui_handle_action(ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_DETAILS);
    static uint32_t icon_pixels[48U * 48U];
    static uint32_t screenshot_pixels[320U * 170U];
    for (size_t index = 0; index < 48U * 48U; index++)
        icon_pixels[index] = 0xff35d07fU;
    for (unsigned int y = 0; y < 170U; y++) {
        for (unsigned int x = 0; x < 320U; x++)
            screenshot_pixels[y * 320U + x] =
                0xff000000U | ((x * 255U / 319U) << 16U) |
                (y * 255U / 169U);
    }
    cp0_ui_set_store_details(
        ui, "dev.cardputerzero.notify", "2.0.1", "CardputerZero Labs",
        "UTILITIES", "4+",
        "A detailed reviewed description for the notification application. "
        "It remains readable on the compact display and can be scrolled.",
        "Adds immutable screenshots and clearer permission review.", 2);
    cp0_ui_set_store_icon(ui, "dev.cardputerzero.notify", "2.0.1",
                          icon_pixels, 48, 48);
    write_snapshot(directory, "store-detail", ui, frame);
    struct cp0_ui_store_catalog_app failed_store_app = store_apps[1];
    failed_store_app.state = CP0_UI_STORE_FAILED;
    failed_store_app.failure_reason = CP0_UI_STORE_FAILURE_NETWORK;
    cp0_ui_sync_store_catalog(ui, &failed_store_app, 1, false, false);
    cp0_ui_sync_store_browse(ui, 0, 1, false, 0, &failed_store_app, 1, false);
    write_snapshot(directory, "store-failed", ui, frame);
    cp0_ui_sync_store_catalog(ui, store_apps, 2, false, false);
    cp0_ui_sync_store_browse(ui, 0, 2, false, 0, store_apps, 2, false);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "store-description", ui, frame);
    assert(cp0_ui_handle_action(ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_SCREENSHOT);
    cp0_ui_set_store_screenshot(ui, "dev.cardputerzero.notify", "2.0.1", 0,
                                screenshot_pixels, 320, 170);
    write_snapshot(directory, "store-screenshot", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "store-permissions", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "store-release-notes", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "store-search-empty", ui, frame);
    static const char search_query[] = "camera";
    for (size_t index = 0; index < strlen(search_query); index++)
        assert(cp0_ui_store_input_ascii(ui, search_query[index]) ==
               CP0_UI_EVENT_STORE_SEARCH);
    cp0_ui_sync_store_search(ui, search_query, 0, 2, false, 0, store_apps, 2,
                             true);
    write_snapshot(directory, "store-search", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    struct cp0_ui_store_catalog_app update_snapshot = store_apps[1];
    update_snapshot.state = CP0_UI_STORE_AVAILABLE;
    update_snapshot.progress_percent = 0;
    update_snapshot.installed_version = "1.0.0";
    cp0_ui_sync_store_catalog(ui, &update_snapshot, 1, false, false);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "store-updates", ui, frame);
    cp0_ui_show_store_install_prompt(ui, 2, 3, 1, 24U * 1024U * 1024U,
                                     180U * 1024U * 1024U);
    write_snapshot(directory, "store-install-confirm", ui, frame);
    assert(cp0_ui_handle_action(ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    cp0_ui_show_store_preflight_error(ui, CP0_UI_STORE_PREFLIGHT_STORAGE);
    write_snapshot(directory, "store-install-storage", ui, frame);
    assert(cp0_ui_handle_action(ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);

    cp0_ui_init(ui);
    cp0_ui_set_local_simulation(ui, true);
    cp0_ui_set_status(ui, "12:34", true, 73);
    cp0_ui_sync_store_catalog(ui, store_apps, 2, false, false);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    static const char missing_query[] = "missing";
    for (size_t index = 0; index < strlen(missing_query); index++)
        cp0_ui_store_input_ascii(ui, missing_query[index]);
    cp0_ui_sync_store_search(ui, missing_query, 0, 0, false, 0, NULL, 0,
                             false);
    write_snapshot(directory, "store-search-none", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_BACK);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "store-search-recent", ui, frame);

    cp0_ui_init(ui);
    cp0_ui_set_local_simulation(ui, true);
    cp0_ui_set_status(ui, "12:34", true, 73);
    cp0_ui_sync_store_catalog(ui, store_apps, 2, false, false);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    for (unsigned int index = 0; index < 32; index++)
        cp0_ui_store_input_ascii(ui, (char)('a' + index % 26));
    cp0_ui_sync_store_search(ui, ui->store_search_query, 0, 0, false, 0,
                             NULL, 0, false);
    write_snapshot(directory, "store-search-max", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_GO_HOME);
    write_snapshot(directory, "store-background-progress", ui, frame);
    cp0_ui_set_store_app_state(ui, "dev.cardputerzero.notify",
                               CP0_UI_STORE_PAUSED, 38);
    cp0_ui_handle_action(ui, CP0_UI_LEFT);

    cp0_ui_sync_app_catalog(ui, apps, 2, false);
    cp0_ui_add_app(ui, 42, "dev.cardputerzero.second");
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "apps", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "app-overview", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "app-storage", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "app-permissions", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "app-actions", ui, frame);
    cp0_ui_set_app_state(ui, "dev.cardputerzero.second", CP0_UI_APP_STOPPED);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "app-uninstall", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);
    cp0_ui_handle_action(ui, CP0_UI_BACK);

    cp0_ui_handle_action(ui, CP0_UI_SHOW_TASKS);
    write_snapshot(directory, "tasks", ui, frame);

    cp0_ui_show_notification(ui, 7, "Hello Card", "Operation complete",
                             "The requested work finished successfully");
    write_snapshot(directory, "notification", ui, frame);
    cp0_ui_clear_notification(ui);

    cp0_ui_handle_action(ui, CP0_UI_SHOW_POWER);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "power", ui, frame);

    cp0_ui_show_permission(ui, 9, "Hello Card", "notifications.post",
                           "Notify when background work is complete and ready");
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "permission", ui, frame);
    cp0_ui_clear_permission(ui);

    static const struct cp0_ui_document_option documents[] = {
        {.size_bytes = 1200,
         .document_id = "00000000000000010000000000000002",
         .name = "notes.txt"},
        {.size_bytes = 8192,
         .document_id = "00000000000000030000000000000004",
         .name = "field-report.md"},
    };
    assert(cp0_ui_show_documents(ui, 10, "Hello Card", documents, 2));
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "document", ui, frame);

    cp0_ui_clear_documents(ui);
    cp0_ui_handle_action(ui, CP0_UI_GO_HOME);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    const struct cp0_ui_device_info device = {
        .available = true,
        .battery_percent = 73,
        .temperature_millicelsius = 48750,
        .battery_present = true,
        .battery_voltage_available = true,
        .battery_current_available = true,
        .battery_voltage_microvolts = 3875000,
        .battery_current_microamps = -125000,
        .battery_status = 2,
        .i2c_bus_state = 3,
        .display_state = 2,
        .keyboard_state = 2,
        .audio_state = 2,
        .camera_state = 2,
        .uptime_seconds = 93784,
        .memory_total_bytes = 512U * 1024U * 1024U,
        .memory_available_bytes = 318U * 1024U * 1024U,
        .storage_total_bytes = 16ULL * 1024U * 1024U * 1024U,
        .storage_available_bytes = 11ULL * 1024U * 1024U * 1024U,
        .model = "CARDPUTER ZERO",
        .os_version = "CARDPUTERZERO OS 0.1",
    };
    cp0_ui_set_device_info(ui, &device);
    write_snapshot(directory, "device", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "device-resources", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "device-power", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "device-diagnostics", ui, frame);
    const struct cp0_ui_device_info unavailable_device = {
        .battery_percent = -1,
        .temperature_millicelsius = -1,
        .model = "",
        .os_version = "",
    };
    cp0_ui_set_device_info(ui, &unavailable_device);
    write_snapshot(directory, "device-unavailable", ui, frame);
    cp0_ui_set_device_info(ui, &device);

    cp0_ui_handle_action(ui, CP0_UI_GO_HOME);
    cp0_ui_handle_action(ui, CP0_UI_LEFT);
    cp0_ui_handle_action(ui, CP0_UI_LEFT);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    const struct cp0_ui_network_info network = {
        .available = true,
        .online = true,
        .link_up = true,
        .interface_name = "eth0",
        .ipv4_address = "192.168.20.146",
    };
    cp0_ui_set_network_info(ui, &network);
    write_snapshot(directory, "network", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "network-detail", ui, frame);
    const struct cp0_ui_network_info offline_network = {
        .available = true,
        .interface_name = "eth0",
        .ipv4_address = "",
    };
    cp0_ui_set_network_info(ui, &offline_network);
    cp0_ui_handle_action(ui, CP0_UI_LEFT);
    write_snapshot(directory, "network-offline", ui, frame);
    cp0_ui_set_network_info(ui, &network);

    cp0_ui_handle_action(ui, CP0_UI_GO_HOME);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_set_device_settings(
        ui, CP0_UI_AUTHORITY_ORGANIZATION, false, true, false, true,
        false, true, 3);
    write_snapshot(directory, "settings", ui, frame);
    static const char *setting_names[] = {
        "settings-connectivity", "settings-display", "settings-sound",
        "settings-camera", "settings-power", "settings-apps-privacy",
        "settings-system", "settings-security",
    };
    for (unsigned int category = 0; category < 8; category++) {
        ui->settings_selected = category;
        ui->settings_item_selected = 0;
        ui->settings_detail = true;
        write_snapshot(directory, setting_names[category], ui, frame);
    }
    ui->settings_selected = 5;
    ui->settings_item_selected = 4;
    ui->settings_detail = true;
    cp0_ui_set_auto_update(ui, true, true, true, false, true, true, false);
    write_snapshot(directory, "settings-auto-update", ui, frame);
    ui->settings_item_selected = 5;
    cp0_ui_set_metrics(ui, true, true, true, true, true);
    write_snapshot(directory, "settings-metrics", ui, frame);
    cp0_ui_set_metrics(ui, true, false, true, true, false);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "settings-metrics-confirm", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);
    ui->settings_selected = 7;
    ui->settings_item_selected = 1;
    ui->settings_detail = true;
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "settings-confirm", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);

    cp0_ui_handle_action(ui, CP0_UI_BRIGHTNESS_UP);
    write_snapshot(directory, "system-brightness", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_MEDIA_PLAY_PAUSE);
    cp0_ui_set_media_status(ui, CP0_UI_MEDIA_SENT);
    write_snapshot(directory, "system-media-sent", ui, frame);
    cp0_ui_set_media_status(ui, CP0_UI_MEDIA_UNAVAILABLE);
    write_snapshot(directory, "system-media-unavailable", ui, frame);
    cp0_ui_set_media_status(ui, CP0_UI_MEDIA_BUSY);
    write_snapshot(directory, "system-media-busy", ui, frame);
    cp0_ui_set_media_status(ui, CP0_UI_MEDIA_FAILED);
    write_snapshot(directory, "system-media-failed", ui, frame);
    cp0_ui_set_screenshot_status(ui, CP0_UI_SCREENSHOT_SAVED);
    write_snapshot(directory, "system-screenshot-saved", ui, frame);
    cp0_ui_set_screenshot_status(ui, CP0_UI_SCREENSHOT_UNAVAILABLE);
    write_snapshot(directory, "system-screenshot-unavailable", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_HELP);
    write_snapshot(directory, "system-help", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_HELP);
    ui->system_action_overlay = false;
    ui->theme = 1;
    write_snapshot(directory, "theme-light", ui, frame);
    ui->theme = 2;
    write_snapshot(directory, "theme-high-contrast", ui, frame);

    cp0_ui_init(ui);
    cp0_ui_setup_begin(ui, CP0_UI_SETUP_WELCOME);
    write_snapshot(directory, "setup-welcome-waiting", ui, frame);
    const struct cp0_ui_network_info setup_network_info = {
        .available = true,
        .online = true,
        .link_up = true,
        .interface_name = "eth0",
        .ipv4_address = "192.168.51.121",
    };
    cp0_ui_set_network_info(ui, &setup_network_info);
    write_snapshot(directory, "setup-welcome", ui, frame);
    ui->setup_page = CP0_UI_SETUP_HOSTNAME;
    snprintf(ui->setup_hostname, sizeof(ui->setup_hostname), "cp0-bedroom");
    write_snapshot(directory, "setup-hostname", ui, frame);
    ui->setup_page = CP0_UI_SETUP_PASSWORD;
    snprintf(ui->setup_password, sizeof(ui->setup_password), "secret-pass-1");
    write_snapshot(directory, "setup-password", ui, frame);
    memset(ui->setup_password, 0, sizeof(ui->setup_password));
    ui->setup_page = CP0_UI_SETUP_NETWORK;
    ui->setup_network = 1;
    cp0_ui_setup_set_network_status(ui, true, true, "192.168.31.121", true,
                                    false, NULL);
    write_snapshot(directory, "setup-network", ui, frame);
    cp0_ui_setup_set_busy(ui, "SCANNING WI-FI",
                          "SEARCHING FOR NEARBY NETWORKS");
    write_snapshot(directory, "setup-busy", ui, frame);
    cp0_ui_setup_set_busy(ui, NULL, NULL);
    static const struct cp0_ui_setup_wifi setup_wifi[] = {
        {.security = 2, .signal_percent = 91, .ssid = "Home WiFi"},
        {.security = 1, .signal_percent = 67, .ssid = "Studio"},
        {.security = 0, .signal_percent = 35, .ssid = "Guest"},
    };
    cp0_ui_setup_set_wifi(ui, setup_wifi, 3);
    write_snapshot(directory, "setup-wifi", ui, frame);
    snprintf(ui->setup_hostname, sizeof(ui->setup_hostname), "cp0-bedroom");
    snprintf(ui->setup_username, sizeof(ui->setup_username), "owner");
    ui->setup_network = 1;
    ui->setup_ssh_enabled = false;
    ui->setup_page = CP0_UI_SETUP_REVIEW;
    write_snapshot(directory, "setup-review", ui, frame);
    cp0_ui_setup_set_network_status(ui, true, false, NULL, true, true,
                                    "192.168.31.122");
    ui->setup_page = CP0_UI_SETUP_COMPLETE;
    write_snapshot(directory, "setup-complete", ui, frame);
    cp0_ui_setup_begin(ui, CP0_UI_SETUP_REPAIR);
    write_snapshot(directory, "setup-repair", ui, frame);
}

int main(int argc, char **argv)
{
    struct cp0_ui ui;
    struct guarded_frame *frame = calloc(1, sizeof(*frame));
    assert(frame != NULL);
    assert(sizeof(struct cp0_ui) <= 64U * 1024U);

    static const struct {
        uint32_t key;
        char plain;
        char shifted;
    } symbol_keys[] = {
        {2, '1', '!'},  {3, '2', '@'},  {4, '3', '#'},
        {5, '4', '$'},  {6, '5', '%'},  {7, '6', '^'},
        {8, '7', '&'},  {9, '8', '*'},  {10, '9', '('},
        {11, '0', ')'}, {12, '-', '_'}, {13, '=', '+'},
        {26, '[', '{'}, {27, ']', '}'}, {39, ';', ':'},
        {40, '\'', '"'}, {41, '`', '~'}, {43, '\\', '|'},
        {51, ',', '<'}, {52, '.', '>'}, {53, '/', '?'},
    };
    for (size_t index = 0;
         index < sizeof(symbol_keys) / sizeof(symbol_keys[0]); index++) {
        assert(cp0_ui_key_character(symbol_keys[index].key, false) ==
               symbol_keys[index].plain);
        assert(cp0_ui_key_character(symbol_keys[index].key, true) ==
               symbol_keys[index].shifted);
    }
    assert(cp0_ui_key_character(30, false) == 'a');
    assert(cp0_ui_key_character(30, true) == 'A');
    static const uint32_t letter_keys[] = {
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50,
        49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
    };
    for (size_t index = 0;
         index < sizeof(letter_keys) / sizeof(letter_keys[0]); index++) {
        char lowercase = cp0_ui_key_character(letter_keys[index], false);
        assert(lowercase >= 'a' && lowercase <= 'z');
        assert(cp0_ui_key_character(letter_keys[index], true) ==
               (char)(lowercase - 'a' + 'A'));
    }
    assert(cp0_ui_key_character(2, false) == '1');
    assert(cp0_ui_key_character(2, true) == '!');
    assert(cp0_ui_key_character(11, false) == '0');
    assert(cp0_ui_key_character(11, true) == ')');
    assert(cp0_ui_key_character(12, false) == '-');
    assert(cp0_ui_key_character(12, true) == '_');
    assert(cp0_ui_key_character(57, false) == ' ');
    assert(cp0_ui_key_character(0, false) == '\0');

    cp0_ui_init(&ui);
    cp0_ui_set_local_simulation(&ui, true);
    cp0_ui_set_status(&ui, "12:34", true, 73);
    render(&ui, frame);
    assert(ui.screen == CP0_UI_HOME);
    assert(ui.selected == 0);
    assert(pixel(frame, 8, 28) == GREEN);
    assert(pixel(frame, 164, 28) != GREEN);

    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    render(&ui, frame);
    assert(ui.selected == 1);
    assert(pixel(frame, 164, 28) == GREEN);

    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    render(&ui, frame);
    assert(ui.screen == CP0_UI_SETTINGS && !ui.settings_available);

    cp0_ui_set_device_settings(
        &ui, CP0_UI_AUTHORITY_ORGANIZATION, false, true, false, true,
        false, true, 3);
    render(&ui, frame);
    assert(ui.settings_available && ui.settings_selected == 0);
    assert(!ui.developer_mode && !ui.store_install_allowed);
    assert(ui.app_launch_restricted && ui.denied_permission_count == 3);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    assert(ui.settings_detail && ui.settings_item_selected == 0);
    assert(ui.wifi_enabled);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    assert(!ui.wifi_enabled);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(!ui.settings_detail);
    for (unsigned int index = 0; index < 7; index++)
        cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.settings_selected == 7);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.settings_item_selected == 1);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.settings_confirm && ui.dialog_selected == 1);
    cp0_ui_handle_action(&ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_DEVELOPER_ENABLE);
    cp0_ui_set_device_settings(
        &ui, CP0_UI_AUTHORITY_PERSONAL, true, true, false, true, true,
        false, 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_DEVELOPER_DISABLE);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(&ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_RECOVERY_ENABLE);
    assert(ui.settings_item_selected == 2);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(!ui.settings_detail && ui.screen == CP0_UI_SETTINGS);

    ui.settings_selected = 5;
    ui.settings_item_selected = 4;
    ui.settings_detail = true;
    cp0_ui_set_auto_update(&ui, true, false, false, true, true, false,
                           false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    cp0_ui_set_auto_update(&ui, true, false, true, true, true, true, false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_AUTO_UPDATE_ENABLE);
    cp0_ui_set_auto_update(&ui, true, true, true, true, false, true, false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_AUTO_UPDATE_DISABLE);

    ui.settings_item_selected = 5;
    cp0_ui_set_metrics(&ui, true, false, true, true, false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    assert(ui.settings_confirm && ui.settings_confirm_metrics &&
           ui.dialog_selected == 1);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    assert(!ui.settings_confirm);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(&ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_METRICS_ENABLE);
    cp0_ui_set_metrics(&ui, true, true, true, true, false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_METRICS_DISABLE);

    cp0_ui_handle_action(&ui, CP0_UI_SHOW_POWER);
    render(&ui, frame);
    assert(ui.power_dialog);
    assert(pixel(frame, 36, 35) == GREEN);

    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_RESTART);
    assert(!ui.power_dialog);
    cp0_ui_handle_action(&ui, CP0_UI_SHOW_POWER);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_POWER_OFF);

    cp0_ui_init(&ui);
    cp0_ui_set_local_simulation(&ui, true);
    static const struct cp0_ui_store_catalog_app store_catalog[] = {
        {.package_bytes = 1024,
         .permissions = (1U << 2) | (1U << 5),
         .progress_percent = 0,
         .state = CP0_UI_STORE_AVAILABLE,
         .app_id = "dev.cardputerzero.alpha",
         .name = "Alpha",
         .version = "1.0.0",
         .summary = "First reviewed store application",
         .installed_version = "1.0.0"},
        {.package_bytes = 2048,
         .permissions = (1U << 2) | (1U << 6),
         .progress_percent = 0,
         .state = CP0_UI_STORE_AVAILABLE,
         .app_id = "dev.cardputerzero.beta",
         .name = "Beta",
         .version = "2.0.0",
         .summary = "An update ready for installation",
         .installed_version = "1.0.0",
         .installed_permissions = 1U << 6},
    };
    struct cp0_ui today_ui;
    cp0_ui_init(&today_ui);
    cp0_ui_set_local_simulation(&today_ui, true);
    cp0_ui_sync_store_catalog(&today_ui, store_catalog, 2, false, false);
    const struct cp0_ui_store_editorial_collection today_collections[] = {
        {.title = "Utilities",
         .apps = &store_catalog[1],
         .app_count = 1},
    };
    const struct cp0_ui_store_editorial today_editorial = {
        .headline = "Reviewed for the small screen",
        .featured = &store_catalog[0],
        .collections = today_collections,
        .collection_count = 1,
    };
    cp0_ui_sync_store_today(&today_ui, &today_editorial);
    cp0_ui_handle_action(&today_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&today_ui, CP0_UI_ACCEPT);
    assert(today_ui.screen == CP0_UI_STORE &&
           today_ui.store_section == CP0_UI_STORE_TODAY &&
           strcmp(cp0_ui_selected_store_app_id(&today_ui),
                  "dev.cardputerzero.alpha") == 0);
    cp0_ui_handle_action(&today_ui, CP0_UI_DOWN);
    assert(today_ui.store_today_selected == 1 &&
           cp0_ui_selected_store_app_id(&today_ui) == NULL);
    assert(cp0_ui_handle_action(&today_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_NONE);
    assert(today_ui.store_today_collection_open &&
           strcmp(cp0_ui_selected_store_app_id(&today_ui),
                  "dev.cardputerzero.beta") == 0);
    cp0_ui_handle_action(&today_ui, CP0_UI_RIGHT);
    assert(today_ui.store_section == CP0_UI_STORE_TODAY);
    cp0_ui_sync_store_today(&today_ui, &today_editorial);
    assert(today_ui.store_today_collection_open &&
           strcmp(cp0_ui_selected_store_app_id(&today_ui),
                  "dev.cardputerzero.beta") == 0);
    cp0_ui_handle_action(&today_ui, CP0_UI_LEFT);
    assert(today_ui.store_section == CP0_UI_STORE_TODAY &&
           today_ui.store_today_collection_open);
    assert(cp0_ui_handle_action(&today_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_DETAILS);
    assert(today_ui.store_detail);
    cp0_ui_handle_action(&today_ui, CP0_UI_BACK);
    assert(!today_ui.store_detail && today_ui.store_today_collection_open);
    cp0_ui_handle_action(&today_ui, CP0_UI_BACK);
    assert(!today_ui.store_today_collection_open &&
           today_ui.store_today_selected == 1);
    cp0_ui_sync_store_today(&today_ui, NULL);
    assert(!today_ui.store_today_available &&
           today_ui.store_today_collection_count == 0);

    cp0_ui_sync_store_catalog(&ui, store_catalog, 2, false, false);
    assert(ui.store_apps[0].state == CP0_UI_STORE_INSTALLED);
    assert(ui.store_apps[1].state == CP0_UI_STORE_UPDATE);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_STORE && !ui.store_detail);
    assert(ui.store_section == CP0_UI_STORE_TODAY);
    assert(cp0_ui_handle_action(&ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_BROWSE);
    assert(ui.store_section == CP0_UI_STORE_APPS);
    cp0_ui_sync_store_browse(&ui, 0, 2, false, 0, store_catalog, 2, false);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(strcmp(cp0_ui_selected_store_app_id(&ui),
                  "dev.cardputerzero.beta") == 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_DETAILS);
    assert(ui.store_detail);
    assert(ui.store_detail_status == CP0_UI_STORE_DETAIL_LOADING);
    assert(cp0_ui_selected_store_app_state(&ui) == CP0_UI_STORE_UPDATE);
    assert(strcmp(cp0_ui_selected_store_app_version(&ui), "2.0.0") == 0);
    cp0_ui_set_store_details(&ui, "dev.cardputerzero.beta", "1.9.0",
                             "Wrong", "UTILITIES", "4+", "Wrong", "Wrong",
                             1);
    assert(ui.store_detail_status == CP0_UI_STORE_DETAIL_LOADING);
    static const char long_description[] =
        "This is a complete reviewed application description that is long "
        "enough to exercise scrolling on the compact screen. It explains the "
        "application behavior, local data handling, network behavior, camera "
        "usage, recovery behavior, and expected interaction model without "
        "requiring a larger desktop layout. Additional bounded text keeps the "
        "scroll position deterministic for regression coverage.";
    cp0_ui_set_store_details(
        &ui, "dev.cardputerzero.beta", "2.0.0", "CardputerZero Labs",
        "UTILITIES", "9+", long_description, "Adds camera attachments.", 2);
    assert(ui.store_detail_status == CP0_UI_STORE_DETAIL_READY &&
           ui.store_screenshot_count == 2);
    assert(cp0_ui_handle_action(&ui, CP0_UI_RIGHT) == CP0_UI_EVENT_NONE);
    assert(ui.store_detail_page == 1);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.store_detail_text_offset == 1);
    assert(cp0_ui_handle_action(&ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_SCREENSHOT);
    assert(ui.store_screenshot_loading && ui.store_screenshot_index == 0);
    static uint32_t detail_screenshot[320U * 170U];
    cp0_ui_set_store_screenshot(&ui, "dev.cardputerzero.beta", "2.0.0", 0,
                                detail_screenshot, 320, 170);
    assert(ui.store_screenshot_available && !ui.store_screenshot_loading);
    assert(cp0_ui_handle_action(&ui, CP0_UI_DOWN) ==
           CP0_UI_EVENT_STORE_SCREENSHOT);
    assert(ui.store_screenshot_index == 1 && ui.store_screenshot_loading);
    cp0_ui_set_store_screenshot_unavailable(
        &ui, "dev.cardputerzero.beta", "2.0.0", 1);
    assert(!ui.store_screenshot_available && !ui.store_screenshot_loading);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(ui.store_detail_page == 3);
    cp0_ui_handle_action(&ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_LEFT) == CP0_UI_EVENT_NONE);
    assert(ui.store_detail_page == 1);
    cp0_ui_handle_action(&ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_INSTALL);
    cp0_ui_set_store_app_state(&ui, "dev.cardputerzero.beta",
                               CP0_UI_STORE_QUEUED, 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_PAUSE);
    struct cp0_ui_store_catalog_app authoritative_update = store_catalog[1];
    authoritative_update.state = CP0_UI_STORE_UPDATE;
    cp0_ui_sync_store_catalog(&ui, &authoritative_update, 1, false, false);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &authoritative_update, 1,
                             false);
    assert(cp0_ui_selected_store_app_state(&ui) == CP0_UI_STORE_UPDATE);
    cp0_ui_set_store_app_state(&ui, "dev.cardputerzero.beta",
                               CP0_UI_STORE_DOWNLOADING, 43);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_PAUSE);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_CANCEL);
    cp0_ui_handle_action(&ui, CP0_UI_UP);
    cp0_ui_set_store_app_state(&ui, "dev.cardputerzero.beta",
                               CP0_UI_STORE_PAUSED, 43);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_RESUME);

    struct cp0_ui_store_catalog_app stale_update = store_catalog[1];
    stale_update.state = CP0_UI_STORE_PAUSED;
    stale_update.progress_percent = 43;
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, true);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &stale_update, 1, true);
    assert(cp0_ui_selected_store_app_state(&ui) == CP0_UI_STORE_PAUSED);
    assert(ui.store_apps[0].update_available);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_CANCEL);
    cp0_ui_handle_action(&ui, CP0_UI_UP);
    stale_update.state = CP0_UI_STORE_FAILED;
    stale_update.progress_percent = 0;
    stale_update.failure_reason = CP0_UI_STORE_FAILURE_NETWORK;
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, true);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &stale_update, 1, true);
    assert(ui.store_apps[0].state == CP0_UI_STORE_FAILED &&
           ui.store_apps[0].failure_reason == CP0_UI_STORE_FAILURE_NETWORK &&
           ui.store_apps[0].update_available);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_CANCEL);
    cp0_ui_handle_action(&ui, CP0_UI_UP);
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, false);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &stale_update, 1, false);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_INSTALL);
    ui.store_operation_action_selected = 1;
    stale_update.state = CP0_UI_STORE_CANCELED;
    stale_update.failure_reason = CP0_UI_STORE_FAILURE_NONE;
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, false);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &stale_update, 1, false);
    assert(ui.store_apps[0].state == CP0_UI_STORE_CANCELED &&
           ui.store_apps[0].update_available &&
           ui.store_operation_action_selected == 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_INSTALL);
    stale_update.version = "2.1.0";
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, false);
    cp0_ui_sync_store_browse(&ui, 0, 1, false, 0, &stale_update, 1, false);
    assert(!ui.store_detail && ui.screen == CP0_UI_STORE);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(ui.screen == CP0_UI_HOME);
    cp0_ui_set_store_status(&ui, CP0_UI_STORE_UNCONFIGURED);
    assert(ui.store_status == CP0_UI_STORE_UNCONFIGURED);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_STORE && !ui.store_detail);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_REFRESH);

    struct cp0_ui search_ui;
    cp0_ui_init(&search_ui);
    cp0_ui_sync_store_catalog(&search_ui, store_catalog, 2, false, false);
    cp0_ui_handle_action(&search_ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_NONE);
    cp0_ui_handle_action(&search_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&search_ui, CP0_UI_RIGHT);
    assert(search_ui.store_section == CP0_UI_STORE_SEARCH);
    assert(cp0_ui_store_accepts_text(&search_ui));
    assert(cp0_ui_store_input_ascii(&search_ui, 'a') ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(cp0_ui_store_input_ascii(&search_ui, 'p') ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(cp0_ui_store_input_ascii(&search_ui, 'p') ==
           CP0_UI_EVENT_STORE_SEARCH);

    struct cp0_ui_store_catalog_app search_page[8];
    char search_ids[8][48];
    char search_names[8][24];
    for (unsigned int index = 0; index < 8; index++) {
        snprintf(search_ids[index], sizeof(search_ids[index]),
                 "dev.cardputerzero.search%u", index);
        snprintf(search_names[index], sizeof(search_names[index]),
                 "Search Result %u", index);
        search_page[index] = (struct cp0_ui_store_catalog_app){
            .package_bytes = 4096,
            .state = CP0_UI_STORE_AVAILABLE,
            .app_id = search_ids[index],
            .name = search_names[index],
            .version = "1.0.0",
            .summary = "Local ranked search result",
        };
    }
    struct cp0_ui browse_ui;
    cp0_ui_init(&browse_ui);
    cp0_ui_sync_store_catalog(&browse_ui, store_catalog, 2, false, false);
    cp0_ui_handle_action(&browse_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&browse_ui, CP0_UI_ACCEPT);
    assert(cp0_ui_handle_action(&browse_ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_BROWSE);
    cp0_ui_sync_store_browse(&browse_ui, 0, 9, true, 8, search_page, 8,
                             true);
    assert(browse_ui.store_browse_count == 8 &&
           browse_ui.store_browse_has_next &&
           browse_ui.store_browse_stale &&
           cp0_ui_store_browse_offset(&browse_ui) == 0);
    for (unsigned int index = 0; index < 7; index++)
        cp0_ui_handle_action(&browse_ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&browse_ui, CP0_UI_DOWN) ==
           CP0_UI_EVENT_STORE_BROWSE);
    assert(cp0_ui_store_browse_offset(&browse_ui) == 8);
    struct cp0_ui_store_catalog_app final_browse = search_page[0];
    final_browse.app_id = "dev.cardputerzero.search8";
    final_browse.name = "Search Result 8";
    cp0_ui_sync_store_browse(&browse_ui, 8, 9, false, 0, &final_browse, 1,
                             true);
    assert(strcmp(cp0_ui_selected_store_app_id(&browse_ui),
                  "dev.cardputerzero.search8") == 0);
    assert(cp0_ui_handle_action(&browse_ui, CP0_UI_UP) ==
           CP0_UI_EVENT_STORE_BROWSE);
    assert(cp0_ui_store_browse_offset(&browse_ui) == 0);

    cp0_ui_sync_store_search(&search_ui, "app", 0, 9, true, 8,
                             search_page, 8, true);
    assert(search_ui.store_search_count == 8 &&
           search_ui.store_search_has_next &&
           search_ui.store_search_stale && !search_ui.store_catalog_stale);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_NONE);
    assert(!search_ui.store_search_input && search_ui.store_recent_count == 1);
    for (unsigned int index = 0; index < 7; index++)
        cp0_ui_handle_action(&search_ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_DOWN) ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(cp0_ui_store_search_offset(&search_ui) == 8);
    struct cp0_ui_store_catalog_app final_result = search_page[0];
    final_result.app_id = "dev.cardputerzero.search8";
    final_result.name = "Search Result 8";
    cp0_ui_sync_store_search(&search_ui, "app", 8, 9, false, 0,
                             &final_result, 1, true);
    assert(strcmp(cp0_ui_selected_store_app_id(&search_ui),
                  "dev.cardputerzero.search8") == 0);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_DETAILS);
    assert(search_ui.store_detail);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_NONE);
    final_result.version = "1.1.0";
    cp0_ui_sync_store_search(&search_ui, "app", 8, 9, false, 0,
                             &final_result, 1, true);
    assert(!search_ui.store_detail);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_UP) ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(cp0_ui_store_search_offset(&search_ui) == 0);
    cp0_ui_handle_action(&search_ui, CP0_UI_BACK);
    assert(search_ui.store_search_query[0] == '\0' &&
           search_ui.store_search_input);
    cp0_ui_handle_action(&search_ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&search_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(strcmp(search_ui.store_search_query, "app") == 0);

    struct cp0_ui max_query_ui;
    cp0_ui_init(&max_query_ui);
    cp0_ui_sync_store_catalog(&max_query_ui, store_catalog, 2, false, false);
    cp0_ui_handle_action(&max_query_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&max_query_ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(&max_query_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&max_query_ui, CP0_UI_RIGHT);
    for (unsigned int index = 0; index < 32; index++)
        assert(cp0_ui_store_input_ascii(&max_query_ui, 'a') ==
               CP0_UI_EVENT_STORE_SEARCH);
    assert(strlen(max_query_ui.store_search_query) == 32);
    assert(cp0_ui_store_input_ascii(&max_query_ui, 'b') ==
           CP0_UI_EVENT_NONE);
    assert(cp0_ui_store_backspace(&max_query_ui) ==
           CP0_UI_EVENT_STORE_SEARCH);
    assert(strlen(max_query_ui.store_search_query) == 31);

    struct cp0_ui maximum_offset_ui;
    cp0_ui_init(&maximum_offset_ui);
    strcpy(maximum_offset_ui.store_search_query, "app");
    maximum_offset_ui.store_search_offset = CP0_UI_STORE_CATALOG_MAX;
    cp0_ui_sync_store_search(&maximum_offset_ui, "app",
                             CP0_UI_STORE_CATALOG_MAX,
                             CP0_UI_STORE_CATALOG_MAX, false, 0, NULL, 0,
                             false);
    assert(maximum_offset_ui.store_search_status == CP0_UI_STORE_READY &&
           maximum_offset_ui.store_search_total ==
               CP0_UI_STORE_CATALOG_MAX &&
           maximum_offset_ui.store_search_count == 0);

    struct cp0_ui_store_catalog_app older_catalog = store_catalog[0];
    older_catalog.version = "1.0.0-beta.1";
    older_catalog.installed_version = "1.0.0";
    cp0_ui_sync_store_catalog(&max_query_ui, &older_catalog, 1, false, false);
    assert(max_query_ui.store_apps[0].state == CP0_UI_STORE_INSTALLED);
    older_catalog.version = "10.0.0";
    older_catalog.installed_version = "2.0.0";
    cp0_ui_sync_store_catalog(&max_query_ui, &older_catalog, 1, false, false);
    assert(max_query_ui.store_apps[0].state == CP0_UI_STORE_UPDATE);
    older_catalog.version = "2.0.0";
    older_catalog.installed_version = "10.0.0";
    cp0_ui_sync_store_catalog(&max_query_ui, &older_catalog, 1, false, false);
    assert(max_query_ui.store_apps[0].state == CP0_UI_STORE_INSTALLED);
    older_catalog.version = "1.0.0-beta.2";
    older_catalog.installed_version = "1.0.0-beta.1";
    cp0_ui_sync_store_catalog(&max_query_ui, &older_catalog, 1, false, false);
    assert(max_query_ui.store_apps[0].state == CP0_UI_STORE_UPDATE);
    older_catalog.version = "1.0.0+new-build";
    older_catalog.installed_version = "1.0.0+old-build";
    cp0_ui_sync_store_catalog(&max_query_ui, &older_catalog, 1, false, false);
    assert(max_query_ui.store_apps[0].state == CP0_UI_STORE_INSTALLED);

    struct cp0_ui update_queue_ui;
    struct cp0_ui_store_catalog_app update_queue[13];
    char update_ids[13][48];
    char update_names[13][24];
    cp0_ui_init(&update_queue_ui);
    update_queue_ui.screen = CP0_UI_STORE;
    update_queue_ui.store_section = CP0_UI_STORE_SEARCH;
    update_queue_ui.store_search_input = false;
    for (unsigned int index = 0; index < 13; index++) {
        snprintf(update_ids[index], sizeof(update_ids[index]),
                 "dev.cardputerzero.batch%02u", index);
        snprintf(update_names[index], sizeof(update_names[index]),
                 "Batch Update %02u", index);
        update_queue[index] = (struct cp0_ui_store_catalog_app){
            .package_bytes = 4096,
            .state = CP0_UI_STORE_UPDATE,
            .app_id = update_ids[index],
            .name = update_names[index],
            .version = "2.0.0",
            .summary = "Bounded update queue candidate",
            .installed_version = "1.0.0",
        };
    }
    update_queue[1].state = CP0_UI_STORE_DOWNLOADING;
    update_queue[1].progress_percent = 42;
    update_queue[2].state = CP0_UI_STORE_PAUSED;
    update_queue[2].progress_percent = 42;
    update_queue[3].state = CP0_UI_STORE_INSTALLING;
    update_queue[4].state = CP0_UI_STORE_FAILED;
    update_queue[4].failure_reason = CP0_UI_STORE_FAILURE_NETWORK;
    update_queue[5].state = CP0_UI_STORE_CANCELED;
    update_queue[6].state = CP0_UI_STORE_QUEUED;
    cp0_ui_sync_store_catalog(&update_queue_ui, update_queue, 13, false,
                              false);
    assert(cp0_ui_handle_action(&update_queue_ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_NONE);
    assert(update_queue_ui.store_section == CP0_UI_STORE_UPDATES &&
           update_queue_ui.store_update_all_selected);
    assert(cp0_ui_handle_action(&update_queue_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_UPDATE_ALL);
    const char *update_batch[CP0_UI_STORE_UPDATE_BATCH_MAX];
    size_t update_count = cp0_ui_collect_store_update_batch(
        &update_queue_ui, update_batch, CP0_UI_STORE_UPDATE_BATCH_MAX);
    assert(update_count == CP0_UI_STORE_UPDATE_BATCH_MAX);
    static const unsigned int expected_updates[] = {0, 4, 5, 7, 8, 9, 10, 11};
    for (size_t index = 0; index < update_count; index++)
        assert(strcmp(update_batch[index],
                      update_ids[expected_updates[index]]) == 0);

    cp0_ui_handle_action(&update_queue_ui, CP0_UI_DOWN);
    assert(!update_queue_ui.store_update_all_selected &&
           strcmp(cp0_ui_selected_store_app_id(&update_queue_ui),
                  update_ids[0]) == 0);
    assert(cp0_ui_handle_action(&update_queue_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_DETAILS);
    assert(update_queue_ui.store_detail);
    cp0_ui_handle_action(&update_queue_ui, CP0_UI_BACK);
    assert(!update_queue_ui.store_detail);
    cp0_ui_handle_action(&update_queue_ui, CP0_UI_UP);
    assert(update_queue_ui.store_update_all_selected &&
           cp0_ui_selected_store_app_id(&update_queue_ui) == NULL);

    cp0_ui_sync_store_catalog(&update_queue_ui, update_queue, 13, false, true);
    assert(update_queue_ui.store_update_all_selected);
    assert(cp0_ui_handle_action(&update_queue_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_NONE);
    assert(cp0_ui_collect_store_update_batch(
               &update_queue_ui, update_batch,
               CP0_UI_STORE_UPDATE_BATCH_MAX) == 0);
    cp0_ui_sync_store_catalog(&update_queue_ui, update_queue, 13, false,
                              false);
    for (size_t index = 0; index < update_count; index++)
        cp0_ui_set_store_app_state(&update_queue_ui, update_batch[index],
                                   CP0_UI_STORE_QUEUED, 0);
    assert(update_queue_ui.store_update_all_selected);
    cp0_ui_set_store_app_state(&update_queue_ui, update_ids[12],
                               CP0_UI_STORE_QUEUED, 0);
    assert(!update_queue_ui.store_update_all_selected &&
           cp0_ui_collect_store_update_batch(
               &update_queue_ui, update_batch,
               CP0_UI_STORE_UPDATE_BATCH_MAX) == 0);

    struct cp0_ui activity_ui;
    cp0_ui_init(&activity_ui);
    struct cp0_ui_store_catalog_app activity_apps[] = {
        {.package_bytes = 4096,
         .state = CP0_UI_STORE_QUEUED,
         .app_id = "dev.cardputerzero.activity-alpha",
         .name = "Activity Alpha",
         .version = "2.0.0",
         .summary = "Queued background update",
         .installed_version = "1.0.0"},
        {.package_bytes = 8192,
         .progress_percent = 42,
         .state = CP0_UI_STORE_DOWNLOADING,
         .app_id = "dev.cardputerzero.activity-beta",
         .name = "Activity Beta",
         .version = "3.0.0",
         .summary = "Downloading background update",
         .installed_version = "2.0.0"},
    };
    cp0_ui_sync_store_catalog(&activity_ui, activity_apps, 2, false, false);
    assert(activity_ui.store_activity &&
           activity_ui.store_activity_count == 2 &&
           activity_ui.store_activity_state == CP0_UI_STORE_DOWNLOADING &&
           activity_ui.store_activity_progress_percent == 42);
    struct cp0_ui_store_completion completion;
    assert(!cp0_ui_take_store_completion(&activity_ui, &completion));

    struct cp0_ui install_prompt_ui;
    cp0_ui_init(&install_prompt_ui);
    cp0_ui_show_store_install_prompt(&install_prompt_ui, 1, 2, 1, 4096,
                                     8192);
    assert(install_prompt_ui.store_install_prompt &&
           install_prompt_ui.dialog_selected == 1 &&
           install_prompt_ui.store_preflight_new_permissions == 2);
    cp0_ui_handle_action(&install_prompt_ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&install_prompt_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_INSTALL_CONFIRM);
    assert(!install_prompt_ui.store_install_prompt);
    cp0_ui_show_store_preflight_error(&install_prompt_ui,
                                      CP0_UI_STORE_PREFLIGHT_POLICY);
    assert(install_prompt_ui.store_install_prompt &&
           cp0_ui_handle_action(&install_prompt_ui, CP0_UI_BACK) ==
               CP0_UI_EVENT_NONE &&
           !install_prompt_ui.store_install_prompt);
    cp0_ui_set_store_app_state(&activity_ui, activity_apps[1].app_id,
                               CP0_UI_STORE_PAUSED, 42);
    assert(activity_ui.store_activity &&
           activity_ui.store_activity_count == 1 &&
           activity_ui.store_activity_state == CP0_UI_STORE_QUEUED);
    cp0_ui_set_store_app_state(&activity_ui, activity_apps[0].app_id,
                               CP0_UI_STORE_INSTALLING, 100);
    assert(activity_ui.store_activity &&
           activity_ui.store_activity_state == CP0_UI_STORE_INSTALLING);
    cp0_ui_set_store_app_state(&activity_ui, activity_apps[0].app_id,
                               CP0_UI_STORE_PAUSED, 100);
    assert(!activity_ui.store_activity &&
           activity_ui.store_activity_count == 0);
    activity_apps[0].state = CP0_UI_STORE_INSTALLED;
    activity_apps[0].progress_percent = 100;
    activity_apps[1].state = CP0_UI_STORE_INSTALLED;
    activity_apps[1].progress_percent = 100;
    cp0_ui_sync_store_catalog(&activity_ui, activity_apps, 2, false, false);
    assert(cp0_ui_take_store_completion(&activity_ui, &completion) &&
           completion.count == 2);
    assert(!cp0_ui_take_store_completion(&activity_ui, &completion));
    cp0_ui_sync_store_catalog(&activity_ui, activity_apps, 2, false, false);
    assert(!cp0_ui_take_store_completion(&activity_ui, &completion));

    struct cp0_ui initial_installed_ui;
    cp0_ui_init(&initial_installed_ui);
    cp0_ui_sync_store_catalog(&initial_installed_ui, &activity_apps[0], 1,
                              false, false);
    assert(!cp0_ui_take_store_completion(&initial_installed_ui, &completion));
    activity_apps[0].state = CP0_UI_STORE_QUEUED;
    activity_apps[0].progress_percent = 0;
    cp0_ui_sync_store_catalog(&initial_installed_ui, &activity_apps[0], 1,
                              false, false);
    activity_apps[0].state = CP0_UI_STORE_INSTALLED;
    activity_apps[0].progress_percent = 100;
    cp0_ui_sync_store_catalog(&initial_installed_ui, &activity_apps[0], 1,
                              false, false);
    assert(cp0_ui_take_store_completion(&initial_installed_ui, &completion) &&
           completion.count == 1 &&
           strcmp(completion.app_name, "Activity Alpha") == 0 &&
           strcmp(completion.version, "2.0.0") == 0);

    cp0_ui_handle_action(&ui, CP0_UI_SHOW_TASKS);
    assert(ui.screen == CP0_UI_TASKS);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(ui.screen == CP0_UI_HOME);

    cp0_ui_init(&ui);
    cp0_ui_set_local_simulation(&ui, true);
    static const struct cp0_ui_catalog_app catalog[] = {
        {.running = false,
         .immersive = false,
         .app_id = "dev.cardputerzero.first",
         .name = "First Card",
         .permissions = UINT16_MAX},
        {.running = true,
         .immersive = true,
         .app_id = "dev.cardputerzero.second",
         .name = "Second Card",
         .permissions = UINT16_MAX},
    };
    cp0_ui_sync_app_catalog(&ui, catalog, 2, false);
    assert(ui.app_count == 2);
    assert(strcmp(cp0_ui_selected_app_id(&ui),
                  "dev.cardputerzero.first") == 0);
    assert(cp0_ui_selected_app_token(&ui) == 0);
    assert(cp0_ui_selected_app_state(&ui) == CP0_UI_APP_STOPPED);
    assert(!cp0_ui_selected_app_is_immersive(&ui));
    cp0_ui_add_app(&ui, 41, "dev.cardputerzero.first");
    assert(ui.app_count == 2);
    assert(cp0_ui_selected_app_token(&ui) == 41);
    cp0_ui_set_app_display_mode(&ui, 41, true);
    assert(cp0_ui_selected_app_is_immersive(&ui));
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_APPS);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    cp0_ui_add_app(&ui, 42, "dev.cardputerzero.second");
    assert(cp0_ui_selected_app_token(&ui) == 42);
    cp0_ui_set_app_display_mode(&ui, 42, false);
    assert(!cp0_ui_selected_app_is_immersive(&ui));
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_OPEN_APP);
    render(&ui, frame);
    assert(pixel(frame, 8, 60) == GREEN);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(ui.app_detail);
    assert(ui.app_detail_page == 0);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(ui.app_detail_page == 2);
    assert(ui.app_permission_offset == 0);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.app_permission_offset == 1);
    for (unsigned int index = 0; index < 8; index++)
        cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.app_permission_offset == 4);
    cp0_ui_handle_action(&ui, CP0_UI_UP);
    assert(ui.app_permission_offset == 3);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(ui.app_detail_page == 3);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STOP_APP);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(!ui.app_detail && ui.screen == CP0_UI_APPS);
    cp0_ui_remove_app(&ui, 42);
    assert(ui.app_count == 2);
    assert(cp0_ui_selected_app_token(&ui) == 0);
    assert(cp0_ui_selected_app_state(&ui) == CP0_UI_APP_STOPPED);
    cp0_ui_remove_app(&ui, 99);
    assert(ui.app_count == 2);

    cp0_ui_set_app_state(&ui, "dev.cardputerzero.second",
                         CP0_UI_APP_STARTING);
    assert(cp0_ui_selected_app_state(&ui) == CP0_UI_APP_STARTING);
    cp0_ui_handle_action(&ui, CP0_UI_SHOW_TASKS);
    assert(strcmp(cp0_ui_selected_app_id(&ui),
                  "dev.cardputerzero.first") == 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_OPEN_APP);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STOP_APP);
    cp0_ui_set_app_state(&ui, "dev.cardputerzero.first",
                         CP0_UI_APP_STOPPED);
    cp0_ui_set_app_state(&ui, "dev.cardputerzero.second", CP0_UI_APP_FAILED);
    assert(cp0_ui_selected_app_id(&ui) == NULL);

    static const struct cp0_ui_catalog_app many[] = {
        {.app_id = "dev.cardputerzero.a", .name = "A"},
        {.app_id = "dev.cardputerzero.b", .name = "B"},
        {.app_id = "dev.cardputerzero.c", .name = "C"},
        {.app_id = "dev.cardputerzero.d", .name = "D"},
        {.app_id = "dev.cardputerzero.e", .name = "E"},
        {.app_id = "dev.cardputerzero.f", .name = "F"},
    };
    cp0_ui_sync_app_catalog(&ui, many, 6, true);
    cp0_ui_handle_action(&ui, CP0_UI_GO_HOME);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    for (unsigned int index = 0; index < 5; index++)
        cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(ui.app_selected == 5);
    assert(ui.app_list_truncated);
    render(&ui, frame);

    unsigned int original_brightness = ui.brightness_percent;
    cp0_ui_handle_action(&ui, CP0_UI_BRIGHTNESS_UP);
    assert(ui.brightness_percent >= original_brightness);
    assert(ui.system_action_overlay && ui.system_action_ticks == 2);
    assert(!cp0_ui_tick(&ui));
    assert(cp0_ui_tick(&ui) && !ui.system_action_overlay);
    assert(cp0_ui_handle_action(&ui, CP0_UI_MEDIA_NEXT) ==
           CP0_UI_EVENT_MEDIA_NEXT);
    assert(ui.media_status == CP0_UI_MEDIA_REQUESTED);
    cp0_ui_set_media_status(&ui, CP0_UI_MEDIA_SENT);
    assert(ui.system_action_overlay && ui.system_action_kind == 5 &&
           ui.media_status == CP0_UI_MEDIA_SENT);
    cp0_ui_set_media_status(&ui, CP0_UI_MEDIA_UNAVAILABLE);
    assert(ui.media_status == CP0_UI_MEDIA_UNAVAILABLE);
    cp0_ui_set_media_status(&ui, CP0_UI_MEDIA_BUSY);
    assert(ui.media_status == CP0_UI_MEDIA_BUSY);
    cp0_ui_set_media_status(&ui, CP0_UI_MEDIA_FAILED);
    assert(ui.media_status == CP0_UI_MEDIA_FAILED);
    assert(cp0_ui_handle_action(&ui, CP0_UI_SCREENSHOT) ==
           CP0_UI_EVENT_SCREENSHOT);
    assert(ui.screenshot_status == CP0_UI_SCREENSHOT_REQUESTED);
    cp0_ui_set_screenshot_status(&ui, CP0_UI_SCREENSHOT_SAVED);
    assert(ui.system_action_overlay && ui.system_action_kind == 6 &&
           ui.screenshot_status == CP0_UI_SCREENSHOT_SAVED);
    cp0_ui_set_screenshot_status(&ui, CP0_UI_SCREENSHOT_FAILED);
    assert(ui.screenshot_status == CP0_UI_SCREENSHOT_FAILED);
    cp0_ui_set_screenshot_status(&ui, CP0_UI_SCREENSHOT_UNAVAILABLE);
    assert(ui.screenshot_status == CP0_UI_SCREENSHOT_UNAVAILABLE);
    cp0_ui_set_screenshot_status(&ui, CP0_UI_SCREENSHOT_BUSY);
    assert(ui.screenshot_status == CP0_UI_SCREENSHOT_BUSY);

    struct cp0_ui production;
    cp0_ui_init(&production);
    unsigned int production_brightness = production.brightness_percent;
    assert(cp0_ui_handle_action(&production, CP0_UI_BRIGHTNESS_UP) ==
           CP0_UI_EVENT_BRIGHTNESS_UP);
    assert(production.brightness_percent == production_brightness);
    assert(production.system_action_overlay);
    cp0_ui_set_display_state(&production, true, 75);
    assert(production.brightness_available &&
           production.brightness_percent == 75);
    assert(cp0_ui_handle_action(&production, CP0_UI_BRIGHTNESS_DOWN) ==
           CP0_UI_EVENT_BRIGHTNESS_DOWN);
    cp0_ui_set_display_state(&production, false, 0);
    assert(!production.brightness_available &&
           production.brightness_percent == 75);
    unsigned int production_volume = production.volume_percent;
    assert(cp0_ui_handle_action(&production, CP0_UI_VOLUME_UP) ==
           CP0_UI_EVENT_VOLUME_UP);
    assert(production.volume_percent == production_volume);
    cp0_ui_set_audio_output_state(&production, true, 75, false);
    assert(production.volume_available && production.volume_percent == 75 &&
           !production.muted);
    assert(cp0_ui_handle_action(&production, CP0_UI_VOLUME_DOWN) ==
           CP0_UI_EVENT_VOLUME_DOWN);
    assert(cp0_ui_handle_action(&production, CP0_UI_MUTE) ==
           CP0_UI_EVENT_MUTE);
    cp0_ui_set_audio_output_state(&production, true, 65, true);
    assert(production.volume_percent == 65 && production.muted);
    cp0_ui_set_connectivity_state(&production, true, true, true, false);
    assert(production.connectivity_available && production.wifi_available &&
           production.wifi_enabled && !production.airplane_mode);
    production.screen = CP0_UI_SETTINGS;
    production.settings_available = true;
    production.settings_detail = true;
    production.settings_selected = 0;
    production.settings_item_selected = 0;
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_WIFI_DISABLE);
    assert(production.wifi_enabled);
    cp0_ui_set_connectivity_state(&production, true, true, false, false);
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_WIFI_ENABLE);
    production.settings_item_selected = 1;
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_AIRPLANE_ENABLE);
    cp0_ui_set_connectivity_state(&production, true, true, false, true);
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_AIRPLANE_DISABLE);
    cp0_ui_set_connectivity_state(&production, false, false, false, false);
    assert(!production.connectivity_available && !production.wifi_available);
    cp0_ui_set_audio_output_state(&production, false, 0, false);
    assert(!production.volume_available && production.volume_percent == 65 &&
           production.muted);
    cp0_ui_set_audio_output_state(&production, true, 65, false);
    production.screen = CP0_UI_SETTINGS;
    production.settings_detail = true;
    production.settings_selected = 2;
    production.settings_item_selected = 0;
    assert(cp0_ui_handle_action(&production, CP0_UI_LEFT) ==
           CP0_UI_EVENT_VOLUME_DOWN);
    production.settings_item_selected = 1;
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_MUTE);
    production.settings_selected = 1;
    production.settings_item_selected = 1;
    assert(cp0_ui_handle_action(&production, CP0_UI_LEFT) ==
           CP0_UI_EVENT_THEME_PREVIOUS);
    assert(cp0_ui_handle_action(&production, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_THEME_NEXT);
    production.settings_item_selected = 2;
    assert(cp0_ui_handle_action(&production, CP0_UI_LEFT) ==
           CP0_UI_EVENT_TIMEOUT_PREVIOUS);
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_TIMEOUT_NEXT);
    production.settings_selected = 2;
    production.settings_item_selected = 2;
    assert(cp0_ui_handle_action(&production, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_KEY_SOUNDS_TOGGLE);
    cp0_ui_set_preferences(&production, 2, 3, false);
    assert(production.theme == 2 && production.screen_timeout == 3 &&
           !production.key_sounds);

    struct cp0_ui uninstall_ui;
    cp0_ui_init(&uninstall_ui);
    cp0_ui_set_local_simulation(&uninstall_ui, true);
    cp0_ui_sync_app_catalog(&uninstall_ui, catalog, 1, false);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_DOWN);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_ACCEPT);
    assert(uninstall_ui.app_uninstall_confirm &&
           uninstall_ui.dialog_selected == 1);
    cp0_ui_handle_action(&uninstall_ui, CP0_UI_LEFT);
    assert(cp0_ui_handle_action(&uninstall_ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_UNINSTALL_APP);

    assert(cp0_ui_show_permission(
        &ui, 77, "Hello Card", "camera.capture",
        "Capture a photograph selected by the user"));
    assert(ui.permission_prompt);
    assert(ui.prompt_id == 77);
    cp0_ui_handle_action(&ui, CP0_UI_GO_HOME);
    assert(ui.permission_prompt);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_PERMISSION_ALWAYS);
    cp0_ui_clear_permission(&ui);
    assert(!ui.permission_prompt);
    assert(ui.prompt_id == 0);
    assert(cp0_ui_show_permission(&ui, 78, "Hello Card", "camera.capture",
                                  "Capture a photograph"));
    assert(cp0_ui_handle_action(&ui, CP0_UI_BACK) ==
           CP0_UI_EVENT_PERMISSION_DENY);
    cp0_ui_clear_permission(&ui);

    assert(cp0_ui_show_notification(
        &ui, 91, "Hello Card", "Operation complete",
        "The requested work finished successfully"));
    assert(ui.notification_banner && ui.notification_id == 91);
    render(&ui, frame);
    assert(pixel(frame, 5, 24) == GREEN);
    assert(cp0_ui_show_permission(&ui, 92, "Hello Card", "camera.capture",
                                  "Capture a photograph"));
    render(&ui, frame);
    assert(pixel(frame, 5, 24) != GREEN);
    cp0_ui_clear_permission(&ui);
    cp0_ui_clear_notification(&ui);
    assert(!ui.notification_banner && ui.notification_id == 0);
    assert(ui.notification_app_name[0] == '\0');
    assert(ui.notification_title[0] == '\0');
    assert(ui.notification_body[0] == '\0');
    assert(cp0_ui_show_notification(&ui, 93, "Hello", "Title", ""));
    assert(ui.notification_body[0] == '\0');
    cp0_ui_clear_notification(&ui);
    assert(!cp0_ui_show_notification(&ui, 0, "Hello", "Title", "Body"));

    static const struct cp0_ui_document_option documents[] = {
        {.size_bytes = 5,
         .document_id = "00000000000000010000000000000002",
         .name = "one.txt"},
        {.size_bytes = 4097,
         .document_id = "00000000000000030000000000000004",
         .name = "two.md"},
    };
    assert(cp0_ui_show_documents(&ui, 101, "Hello Card", documents, 2));
    assert(ui.document_prompt && ui.document_prompt_id == 101);
    assert(strcmp(cp0_ui_selected_document_id(&ui),
                  documents[0].document_id) == 0);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_DOCUMENT_SELECT);
    assert(strcmp(cp0_ui_selected_document_id(&ui),
                  documents[1].document_id) == 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_BACK) ==
           CP0_UI_EVENT_DOCUMENT_CANCEL);
    cp0_ui_clear_documents(&ui);
    assert(!ui.document_prompt && ui.document_prompt_id == 0);
    assert(cp0_ui_selected_document_id(&ui) == NULL);

    cp0_ui_init(&ui);
    cp0_ui_setup_begin(&ui, CP0_UI_SETUP_WELCOME);
    assert(ui.setup_active);
    assert(cp0_ui_handle_action(&ui, CP0_UI_GO_HOME) == CP0_UI_EVENT_NONE);
    assert(ui.setup_page == CP0_UI_SETUP_WELCOME);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.setup_page == CP0_UI_SETUP_LANGUAGE);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(strcmp(cp0_ui_setup_locale(&ui), "zh_CN.UTF-8") == 0);
    ui.setup_page = CP0_UI_SETUP_HOSTNAME;
    assert(cp0_ui_setup_accepts_text(&ui));
    assert(!cp0_ui_setup_input_ascii(&ui, ' '));
    static const char hostname[] = "cp0-test";
    for (size_t index = 0; index < strlen(hostname); index++)
        assert(cp0_ui_setup_input_ascii(&ui, hostname[index]));
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_SETUP_SET_REGION);
    cp0_ui_setup_result(&ui, CP0_UI_EVENT_SETUP_SET_REGION, true, NULL);
    assert(ui.setup_page == CP0_UI_SETUP_DISPLAY_NAME);
    assert(cp0_ui_setup_input_ascii(&ui, 'O'));
    assert(cp0_ui_setup_input_ascii(&ui, 'w'));
    assert(cp0_ui_setup_backspace(&ui));
    snprintf(ui.setup_display_name, sizeof(ui.setup_display_name), "Owner");
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    snprintf(ui.setup_username, sizeof(ui.setup_username), "owner");
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_SETUP_SET_OWNER);
    cp0_ui_setup_result(&ui, CP0_UI_EVENT_SETUP_SET_OWNER, true, NULL);
    static const char printable_password[] =
        "Aa1!@#$%^&*()_+-=[]{};:'\"~`\\|,.<>/?";
    for (size_t index = 0; index < strlen(printable_password); index++)
        assert(cp0_ui_setup_input_ascii(&ui, printable_password[index]));
    assert(strcmp(ui.setup_password, printable_password) == 0);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    for (size_t index = 0; index < strlen(printable_password); index++)
        assert(cp0_ui_setup_input_ascii(&ui, printable_password[index]));
    assert(strcmp(ui.setup_password_confirm, printable_password) == 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_SETUP_SET_PASSWORD);
    cp0_ui_setup_result(&ui, CP0_UI_EVENT_SETUP_SET_PASSWORD, true, NULL);
    assert(ui.setup_password[0] == '\0' &&
           ui.setup_password_confirm[0] == '\0');
    assert(cp0_ui_handle_action(&ui, CP0_UI_GO_HOME) == CP0_UI_EVENT_NONE);
    assert(ui.setup_page == CP0_UI_SETUP_NETWORK);
    snprintf(ui.setup_password, sizeof(ui.setup_password), "left-in-memory");
    snprintf(ui.setup_password_confirm, sizeof(ui.setup_password_confirm),
             "left-in-memory");
    cp0_ui_setup_resume(&ui, 4, "cp0-test", "Owner", "owner", false);
    assert(ui.setup_page == CP0_UI_SETUP_NETWORK);
    assert(ui.setup_password[0] == '\0' &&
           ui.setup_password_confirm[0] == '\0');
    snprintf(ui.setup_wifi_password, sizeof(ui.setup_wifi_password),
             "wifi-secret");
    cp0_ui_setup_resume(&ui, 5, "cp0-test", "Owner", "owner", false);
    assert(ui.setup_page == CP0_UI_SETUP_SSH);
    assert(ui.setup_wifi_password[0] == '\0');
    cp0_ui_setup_resume(&ui, 2, "cp0-test", "Owner", "owner", false);
    assert(ui.setup_page == CP0_UI_SETUP_DISPLAY_NAME);
    cp0_ui_setup_resume(&ui, 3, "cp0-test", "Owner", "owner", false);
    assert(ui.setup_page == CP0_UI_SETUP_PASSWORD);
    cp0_ui_setup_resume(&ui, 4, "cp0-test", "Owner", "owner", false);
    cp0_ui_setup_set_network_status(&ui, true, true, "192.168.20.146", true,
                                    false, NULL);
    assert(ui.setup_network_manager_available && ui.setup_ethernet_connected);
    assert(strcmp(ui.setup_ethernet_ipv4, "192.168.20.146") == 0);
    cp0_ui_setup_set_busy(&ui, "SCANNING WI-FI", "SEARCHING");
    assert(ui.setup_busy);
    cp0_ui_setup_result(&ui, CP0_UI_EVENT_SETUP_RETRY, true, NULL);
    assert(!ui.setup_busy);
    ui.setup_page = CP0_UI_SETUP_WIFI_PASSWORD;
    memset(ui.setup_wifi_password, 'x', 63);
    ui.setup_wifi_password[63] = '\0';
    assert(!cp0_ui_setup_input_ascii(&ui, 'x'));
    static const struct cp0_ui_setup_wifi wifi[] = {
        {.security = 1, .signal_percent = 80, .ssid = "Lab"},
    };
    cp0_ui_setup_set_wifi(&ui, wifi, 1);
    assert(ui.setup_page == CP0_UI_SETUP_WIFI_LIST && ui.setup_wifi_count == 1);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.setup_page == CP0_UI_SETUP_WIFI_PASSWORD);
    cp0_ui_setup_result(&ui, CP0_UI_EVENT_SETUP_CONNECT_WIFI, false,
                        "Connection failed");
    assert(ui.setup_page == CP0_UI_SETUP_ERROR);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_SETUP_RETRY);
    static const struct cp0_ui_setup_wifi unsupported_wifi[] = {
        {.security = 3, .signal_percent = 70, .ssid = "Enterprise"},
    };
    cp0_ui_setup_set_wifi(&ui, unsupported_wifi, 1);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);
    assert(ui.setup_page == CP0_UI_SETUP_WIFI_LIST);
    assert(strstr(ui.setup_error, "not supported") != NULL);

    render(&ui, frame);
    if (argc == 2)
        write_snapshots(argv[1], &ui, frame);
    free(frame);
    return 0;
}
