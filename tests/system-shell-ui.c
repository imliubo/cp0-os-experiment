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
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "store", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "store-detail", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_GO_HOME);
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
    ui->settings_selected = 7;
    ui->settings_item_selected = 1;
    ui->settings_detail = true;
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    write_snapshot(directory, "settings-confirm", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_BACK);

    cp0_ui_handle_action(ui, CP0_UI_BRIGHTNESS_UP);
    write_snapshot(directory, "system-brightness", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_HELP);
    write_snapshot(directory, "system-help", ui, frame);
    cp0_ui_handle_action(ui, CP0_UI_HELP);
    ui->system_action_overlay = false;
    ui->theme = 1;
    write_snapshot(directory, "theme-light", ui, frame);
    ui->theme = 2;
    write_snapshot(directory, "theme-high-contrast", ui, frame);
}

int main(int argc, char **argv)
{
    struct cp0_ui ui;
    struct guarded_frame *frame = calloc(1, sizeof(*frame));
    assert(frame != NULL);

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

    cp0_ui_handle_action(&ui, CP0_UI_SHOW_POWER);
    render(&ui, frame);
    assert(ui.power_dialog);
    assert(pixel(frame, 36, 35) == GREEN);

    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_RESTART);
    assert(!ui.power_dialog);

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
         .permissions = 1U << 6,
         .progress_percent = 0,
         .state = CP0_UI_STORE_AVAILABLE,
         .app_id = "dev.cardputerzero.beta",
         .name = "Beta",
         .version = "2.0.0",
         .summary = "An update ready for installation",
         .installed_version = "1.0.0"},
    };
    cp0_ui_sync_store_catalog(&ui, store_catalog, 2, false, true);
    assert(ui.store_apps[0].state == CP0_UI_STORE_INSTALLED);
    assert(ui.store_apps[1].state == CP0_UI_STORE_UPDATE);
    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_STORE && !ui.store_detail);
    assert(cp0_ui_handle_action(&ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_REFRESH);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(strcmp(cp0_ui_selected_store_app_id(&ui),
                  "dev.cardputerzero.beta") == 0);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.store_detail);
    assert(cp0_ui_selected_store_app_state(&ui) == CP0_UI_STORE_UPDATE);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_STORE_INSTALL);
    cp0_ui_set_store_app_state(&ui, "dev.cardputerzero.beta",
                               CP0_UI_STORE_QUEUED, 0);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) == CP0_UI_EVENT_NONE);

    struct cp0_ui_store_catalog_app stale_update = store_catalog[1];
    stale_update.state = CP0_UI_STORE_UPDATE;
    cp0_ui_sync_store_catalog(&ui, &stale_update, 1, false, false);
    assert(cp0_ui_selected_store_app_state(&ui) == CP0_UI_STORE_QUEUED);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(!ui.store_detail && ui.screen == CP0_UI_STORE);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(ui.screen == CP0_UI_HOME);
    cp0_ui_set_store_status(&ui, CP0_UI_STORE_UNCONFIGURED);
    assert(ui.store_status == CP0_UI_STORE_UNCONFIGURED);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_STORE && !ui.store_detail);
    assert(cp0_ui_handle_action(&ui, CP0_UI_RIGHT) ==
           CP0_UI_EVENT_STORE_REFRESH);

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
    assert(cp0_ui_handle_action(&ui, CP0_UI_SCREENSHOT) ==
           CP0_UI_EVENT_SCREENSHOT);

    struct cp0_ui production;
    cp0_ui_init(&production);
    unsigned int production_brightness = production.brightness_percent;
    cp0_ui_handle_action(&production, CP0_UI_BRIGHTNESS_UP);
    assert(production.brightness_percent == production_brightness);
    assert(production.system_action_overlay);

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

    render(&ui, frame);
    if (argc == 2)
        write_snapshots(argv[1], &ui, frame);
    free(frame);
    return 0;
}
