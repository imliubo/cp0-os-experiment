#include "cp0_ui.h"

#include <stddef.h>
#include <stdio.h>
#include <string.h>

#define COLOR_BG 0x000e1112u
#define COLOR_BAR 0x00171c1eu
#define COLOR_SURFACE 0x001c2224u
#define COLOR_SELECTED 0x00232e29u
#define COLOR_TEXT 0x00f4f6f5u
#define COLOR_MUTED 0x009aa5a1u
#define COLOR_GREEN 0x0035d07fu
#define COLOR_YELLOW 0x00f2c14eu
#define COLOR_RED 0x00ef5b5bu

struct canvas {
    uint32_t *pixels;
    int width;
    int height;
    int stride;
};

static const uint8_t font[][5] = {
    [' ' - ' '] = {0x00, 0x00, 0x00, 0x00, 0x00},
    ['%' - ' '] = {0x23, 0x13, 0x08, 0x64, 0x62},
    ['-' - ' '] = {0x08, 0x08, 0x08, 0x08, 0x08},
    ['.' - ' '] = {0x00, 0x60, 0x60, 0x00, 0x00},
    ['/' - ' '] = {0x20, 0x10, 0x08, 0x04, 0x02},
    ['0' - ' '] = {0x3e, 0x51, 0x49, 0x45, 0x3e},
    ['1' - ' '] = {0x00, 0x42, 0x7f, 0x40, 0x00},
    ['2' - ' '] = {0x42, 0x61, 0x51, 0x49, 0x46},
    ['3' - ' '] = {0x21, 0x41, 0x45, 0x4b, 0x31},
    ['4' - ' '] = {0x18, 0x14, 0x12, 0x7f, 0x10},
    ['5' - ' '] = {0x27, 0x45, 0x45, 0x45, 0x39},
    ['6' - ' '] = {0x3c, 0x4a, 0x49, 0x49, 0x30},
    ['7' - ' '] = {0x01, 0x71, 0x09, 0x05, 0x03},
    ['8' - ' '] = {0x36, 0x49, 0x49, 0x49, 0x36},
    ['9' - ' '] = {0x06, 0x49, 0x49, 0x29, 0x1e},
    [':' - ' '] = {0x00, 0x36, 0x36, 0x00, 0x00},
    ['A' - ' '] = {0x7e, 0x11, 0x11, 0x11, 0x7e},
    ['B' - ' '] = {0x7f, 0x49, 0x49, 0x49, 0x36},
    ['C' - ' '] = {0x3e, 0x41, 0x41, 0x41, 0x22},
    ['D' - ' '] = {0x7f, 0x41, 0x41, 0x22, 0x1c},
    ['E' - ' '] = {0x7f, 0x49, 0x49, 0x49, 0x41},
    ['F' - ' '] = {0x7f, 0x09, 0x09, 0x09, 0x01},
    ['G' - ' '] = {0x3e, 0x41, 0x49, 0x49, 0x7a},
    ['H' - ' '] = {0x7f, 0x08, 0x08, 0x08, 0x7f},
    ['I' - ' '] = {0x00, 0x41, 0x7f, 0x41, 0x00},
    ['J' - ' '] = {0x20, 0x40, 0x41, 0x3f, 0x01},
    ['K' - ' '] = {0x7f, 0x08, 0x14, 0x22, 0x41},
    ['L' - ' '] = {0x7f, 0x40, 0x40, 0x40, 0x40},
    ['M' - ' '] = {0x7f, 0x02, 0x0c, 0x02, 0x7f},
    ['N' - ' '] = {0x7f, 0x04, 0x08, 0x10, 0x7f},
    ['O' - ' '] = {0x3e, 0x41, 0x41, 0x41, 0x3e},
    ['P' - ' '] = {0x7f, 0x09, 0x09, 0x09, 0x06},
    ['Q' - ' '] = {0x3e, 0x41, 0x51, 0x21, 0x5e},
    ['R' - ' '] = {0x7f, 0x09, 0x19, 0x29, 0x46},
    ['S' - ' '] = {0x46, 0x49, 0x49, 0x49, 0x31},
    ['T' - ' '] = {0x01, 0x01, 0x7f, 0x01, 0x01},
    ['U' - ' '] = {0x3f, 0x40, 0x40, 0x40, 0x3f},
    ['V' - ' '] = {0x1f, 0x20, 0x40, 0x20, 0x1f},
    ['W' - ' '] = {0x3f, 0x40, 0x38, 0x40, 0x3f},
    ['X' - ' '] = {0x63, 0x14, 0x08, 0x14, 0x63},
    ['Y' - ' '] = {0x07, 0x08, 0x70, 0x08, 0x07},
    ['Z' - ' '] = {0x61, 0x51, 0x49, 0x45, 0x43},
};

static void fill_rect(struct canvas *canvas, int x, int y, int width,
                      int height, uint32_t color)
{
    int left = x < 0 ? 0 : x;
    int top = y < 0 ? 0 : y;
    int right = x + width > canvas->width ? canvas->width : x + width;
    int bottom = y + height > canvas->height ? canvas->height : y + height;

    for (int py = top; py < bottom; py++) {
        for (int px = left; px < right; px++)
            canvas->pixels[py * canvas->stride + px] = color;
    }
}

static void stroke_rect(struct canvas *canvas, int x, int y, int width,
                        int height, int thickness, uint32_t color)
{
    fill_rect(canvas, x, y, width, thickness, color);
    fill_rect(canvas, x, y + height - thickness, width, thickness, color);
    fill_rect(canvas, x, y, thickness, height, color);
    fill_rect(canvas, x + width - thickness, y, thickness, height, color);
}

static void draw_glyph(struct canvas *canvas, int x, int y, char character,
                       int scale, uint32_t color)
{
    if (character >= 'a' && character <= 'z')
        character = (char)(character - 'a' + 'A');
    if (character < ' ' || character > 'Z')
        character = ' ';

    const uint8_t *columns = font[(unsigned char)character - ' '];
    for (int column = 0; column < 5; column++) {
        for (int row = 0; row < 7; row++) {
            if ((columns[column] & (1u << row)) != 0)
                fill_rect(canvas, x + column * scale, y + row * scale, scale,
                          scale, color);
        }
    }
}

static void draw_text(struct canvas *canvas, int x, int y, const char *text,
                      int scale, uint32_t color)
{
    for (; *text != '\0'; text++) {
        draw_glyph(canvas, x, y, *text, scale, color);
        x += 6 * scale;
    }
}

static void draw_prompt_line(struct canvas *canvas, int y, const char *text,
                             size_t start, size_t maximum);

static const char *screen_title(const struct cp0_ui *ui)
{
    if (ui->permission_prompt)
        return "PERMISSION";
    if (ui->document_prompt)
        return "DOCUMENT";
    switch (ui->screen) {
    case CP0_UI_HOME:
        return "HOME";
    case CP0_UI_APPS:
        return "APPS";
    case CP0_UI_STORE:
        return ui->store_detail ? "STORE APP" : "STORE";
    case CP0_UI_DEVICE:
        return "DEVICE";
    case CP0_UI_NETWORK:
        return "NETWORK";
    case CP0_UI_SETTINGS:
        return "SETTINGS";
    case CP0_UI_TASKS:
        return "TASKS";
    }
    return "SYSTEM";
}

static void draw_network_icon(struct canvas *canvas, bool online)
{
    uint32_t color = online ? COLOR_GREEN : COLOR_MUTED;
    fill_rect(canvas, 267, 13, 3, 3, color);
    fill_rect(canvas, 272, 10, 3, 6, color);
    fill_rect(canvas, 277, 7, 3, 9, color);
}

static void draw_battery(struct canvas *canvas, int percent)
{
    stroke_rect(canvas, 290, 6, 24, 10, 1, COLOR_MUTED);
    fill_rect(canvas, 314, 9, 2, 4, COLOR_MUTED);
    if (percent < 0)
        return;
    if (percent > 100)
        percent = 100;
    uint32_t color = percent <= 15 ? COLOR_RED : COLOR_GREEN;
    fill_rect(canvas, 292, 8, (20 * percent) / 100, 6, color);
}

static void draw_status_bar(struct canvas *canvas, const struct cp0_ui *ui)
{
    fill_rect(canvas, 0, 0, CP0_UI_WIDTH, 21, COLOR_BAR);
    fill_rect(canvas, 0, 20, CP0_UI_WIDTH, 1, COLOR_GREEN);
    draw_text(canvas, 8, 7, screen_title(ui), 1, COLOR_TEXT);
    draw_text(canvas, 145, 7, ui->clock_text, 1, COLOR_MUTED);
    draw_network_icon(canvas, ui->network_online);
    draw_battery(canvas, ui->battery_percent);
}

static void draw_apps_icon(struct canvas *canvas, int x, int y,
                           uint32_t color)
{
    fill_rect(canvas, x, y, 8, 8, color);
    fill_rect(canvas, x + 11, y, 8, 8, color);
    fill_rect(canvas, x, y + 11, 8, 8, color);
    fill_rect(canvas, x + 11, y + 11, 8, 8, color);
}

static void draw_store_icon(struct canvas *canvas, int x, int y,
                            uint32_t color)
{
    stroke_rect(canvas, x + 2, y + 6, 21, 16, 2, color);
    stroke_rect(canvas, x + 7, y, 11, 9, 2, color);
}

static void draw_device_icon(struct canvas *canvas, int x, int y,
                             uint32_t color)
{
    stroke_rect(canvas, x, y + 2, 23, 16, 2, color);
    fill_rect(canvas, x + 8, y + 19, 7, 2, color);
}

static void draw_large_network_icon(struct canvas *canvas, int x, int y,
                                    uint32_t color)
{
    fill_rect(canvas, x, y + 15, 4, 6, color);
    fill_rect(canvas, x + 7, y + 10, 4, 11, color);
    fill_rect(canvas, x + 14, y + 5, 4, 16, color);
    fill_rect(canvas, x + 21, y, 4, 21, color);
}

static void draw_settings_icon(struct canvas *canvas, int x, int y,
                               uint32_t color)
{
    for (int row = 0; row < 3; row++) {
        int y_offset = y + 2 + row * 8;
        int knob = row == 1 ? 15 : 6;
        fill_rect(canvas, x, y_offset + 2, 23, 2, color);
        fill_rect(canvas, x + knob, y_offset, 4, 6, color);
    }
}

static void draw_home(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"APPS", "STORE", "DEVICE", "NETWORK",
                                   "SETTINGS"};
    for (unsigned int index = 0; index < 5; index++) {
        int x = 8 + (int)(index % 3) * 103;
        int y = 28 + (int)(index / 3) * 68;
        bool selected = index == ui->selected;
        fill_rect(canvas, x, y, 98, 61,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, x, y, 98, 61, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        uint32_t icon_color = selected ? COLOR_GREEN : COLOR_MUTED;
        switch (index) {
        case 0:
            draw_apps_icon(canvas, x + 11, y + 12, icon_color);
            break;
        case 1:
            draw_store_icon(canvas, x + 10, y + 11, icon_color);
            break;
        case 2:
            draw_device_icon(canvas, x + 10, y + 12, icon_color);
            break;
        case 3:
            draw_large_network_icon(canvas, x + 10, y + 11, icon_color);
            break;
        default:
            draw_settings_icon(canvas, x + 10, y + 9, icon_color);
            break;
        }
        draw_text(canvas, x + 10, y + 44, labels[index], 1, COLOR_TEXT);
    }
}

static void draw_empty_page(struct canvas *canvas, const char *title,
                            const char *detail, uint32_t accent)
{
    fill_rect(canvas, 8, 31, 304, 126, COLOR_SURFACE);
    fill_rect(canvas, 8, 31, 4, 126, accent);
    draw_text(canvas, 28, 55, title, 2, COLOR_TEXT);
    draw_text(canvas, 28, 91, detail, 1, COLOR_MUTED);
}

static void draw_apps_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *states[] = {"READY", "STARTING", "RUNNING", "FAILED"};
    if (ui->app_count == 0) {
        draw_empty_page(canvas, "APPS", "NO APPS INSTALLED", COLOR_GREEN);
        return;
    }

    unsigned int first = ui->app_selected > 3 ? ui->app_selected - 3 : 0;
    unsigned int visible = ui->app_count - first;
    if (visible > 4)
        visible = 4;
    for (unsigned int row = 0; row < visible; row++) {
        unsigned int index = first + row;
        int y = 28 + (int)row * 32;
        bool selected = index == ui->app_selected;
        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        fill_rect(canvas, 17, y + 9, 10, 10,
                  ui->apps[index].state == CP0_UI_APP_RUNNING
                      ? COLOR_GREEN
                      : (selected ? COLOR_YELLOW : COLOR_MUTED));
        draw_text(canvas, 36, y + 6, ui->apps[index].name, 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text(canvas, 218, y + 17, states[ui->apps[index].state], 1,
                  ui->apps[index].state == CP0_UI_APP_FAILED
                      ? COLOR_RED
                      : COLOR_MUTED);
    }
    if (ui->app_list_truncated)
        draw_text(canvas, 284, 159, "32+", 1, COLOR_YELLOW);
}

static const char *store_state_label(const struct cp0_ui_store_app *app,
                                     char label[16])
{
    static const char *states[] = {"GET",       "UPDATE", "QUEUED", "DOWNLOAD",
                                   "INSTALL",   "INSTALLED", "FAILED"};
    if (app->state == CP0_UI_STORE_DOWNLOADING) {
        snprintf(label, 16, "DOWN %u%%", app->progress_percent);
        return label;
    }
    return states[app->state];
}

static void draw_store_list(struct canvas *canvas, const struct cp0_ui *ui)
{
    if (ui->store_status != CP0_UI_STORE_READY) {
        static const char *messages[] = {"LOADING CATALOG", "", "NOT CONFIGURED",
                                         "STORE UNAVAILABLE"};
        draw_empty_page(canvas, "STORE", messages[ui->store_status],
                        ui->store_status == CP0_UI_STORE_UNAVAILABLE
                            ? COLOR_RED
                            : COLOR_YELLOW);
        return;
    }
    if (ui->store_count == 0) {
        draw_empty_page(canvas, "STORE", "NO APPS AVAILABLE", COLOR_GREEN);
        return;
    }

    unsigned int first =
        ui->store_selected > 3 ? ui->store_selected - 3 : 0;
    unsigned int visible = ui->store_count - first;
    if (visible > 4)
        visible = 4;
    for (unsigned int row = 0; row < visible; row++) {
        unsigned int index = first + row;
        const struct cp0_ui_store_app *app = &ui->store_apps[index];
        int y = 28 + (int)row * 32;
        bool selected = index == ui->store_selected;
        char state[16];

        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        fill_rect(canvas, 17, y + 9, 10, 10,
                  app->state == CP0_UI_STORE_INSTALLED
                      ? COLOR_GREEN
                      : (app->state == CP0_UI_STORE_FAILED ? COLOR_RED
                                                           : COLOR_YELLOW));
        draw_prompt_line(canvas, y + 6, app->name, 0, 27);
        draw_text(canvas, 218, y + 17, store_state_label(app, state), 1,
                  app->state == CP0_UI_STORE_FAILED ? COLOR_RED : COLOR_MUTED);
    }
    if (ui->store_catalog_stale)
        draw_text(canvas, 8, 159, "STALE", 1, COLOR_YELLOW);
    if (ui->store_list_truncated)
        draw_text(canvas, 284, 159, "32+", 1, COLOR_YELLOW);
}

static void draw_store_detail(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *permission_names[] = {
        "AUDIO CAPTURE",  "AUDIO PLAYBACK", "CAMERA CAPTURE", "DOCUMENTS OPEN",
        "HARDWARE GPIO",  "NETWORK CLIENT", "NOTIFICATIONS",  "RADIO LORA",
    };
    const struct cp0_ui_store_app *app = &ui->store_apps[ui->store_selected];
    char version[76];
    char state[16];
    unsigned int visible_permission = 0;

    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    fill_rect(canvas, 8, 27, 4, 136, COLOR_GREEN);
    draw_prompt_line(canvas, 34, app->name, 0, 46);
    snprintf(version, sizeof(version), "VERSION %s", app->version);
    draw_prompt_line(canvas, 48, version, 0, 46);
    draw_prompt_line(canvas, 64, app->summary, 0, 46);
    draw_prompt_line(canvas, 77, app->summary, 46, 46);
    draw_text(canvas, 20, 93, store_state_label(app, state), 1,
              app->state == CP0_UI_STORE_FAILED ? COLOR_RED : COLOR_GREEN);
    for (unsigned int bit = 0; bit < 8; bit++) {
        if ((app->permissions & (1U << bit)) == 0)
            continue;
        int x = 20 + (int)(visible_permission % 2) * 146;
        int y = 108 + (int)(visible_permission / 2) * 12;
        draw_text(canvas, x, y, permission_names[bit], 1, COLOR_TEXT);
        visible_permission++;
    }
    if (visible_permission == 0)
        draw_text(canvas, 20, 108, "NO PERMISSIONS", 1, COLOR_MUTED);
}

static void draw_store_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    if (ui->store_detail && ui->store_selected < ui->store_count)
        draw_store_detail(canvas, ui);
    else
        draw_store_list(canvas, ui);
}

static int running_app_index(const struct cp0_ui *ui)
{
    for (unsigned int index = 0; index < ui->app_count; index++) {
        if (ui->apps[index].state == CP0_UI_APP_RUNNING ||
            ui->apps[index].state == CP0_UI_APP_STARTING)
            return (int)index;
    }
    return -1;
}

static void draw_tasks_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"RESUME", "STOP"};
    int index = running_app_index(ui);
    if (index < 0) {
        draw_empty_page(canvas, "TASKS", "NO RUNNING APPS", COLOR_YELLOW);
        return;
    }

    fill_rect(canvas, 8, 31, 304, 126, COLOR_SURFACE);
    fill_rect(canvas, 8, 31, 4, 126, COLOR_YELLOW);
    draw_text(canvas, 28, 49, "TASKS", 2, COLOR_TEXT);
    draw_text(canvas, 28, 78, ui->apps[index].name, 1, COLOR_TEXT);
    draw_text(canvas, 28, 94,
              ui->apps[index].state == CP0_UI_APP_STARTING ? "STARTING"
                                                           : "RUNNING",
              1, COLOR_GREEN);
    for (unsigned int action = 0; action < 2; action++) {
        int x = 28 + (int)action * 116;
        bool selected = action == ui->task_action_selected;
        fill_rect(canvas, x, 121, 104, 25,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 121, 104, 25, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text(canvas, x + 23, 130, labels[action], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static const char *settings_state(bool enabled, bool allowed)
{
    if (!allowed)
        return "LOCKED";
    return enabled ? "ON" : "OFF";
}

static void draw_settings_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *titles[] = {"DEVELOPER MODE", "RECOVERY BOOT"};
    static const char *details[] = {"INSTALL TRUSTED DEV PACKAGES",
                                    "NEXT BOOT USES CONSOLE"};
    static const char *authorities[] = {"PERSONAL POLICY", "PARENT POLICY",
                                        "ORGANIZATION POLICY"};
    if (!ui->settings_available) {
        draw_empty_page(canvas, "SETTINGS", "SETTINGS UNAVAILABLE", COLOR_RED);
        return;
    }
    for (unsigned int row = 0; row < 2; row++) {
        int y = 30 + (int)row * 54;
        bool selected = ui->settings_selected == row;
        bool enabled = row == 0 ? ui->developer_mode : ui->recovery_mode;
        bool allowed =
            row == 0 ? ui->developer_mode_allowed : ui->recovery_mode_allowed;
        fill_rect(canvas, 8, y, 304, 48,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 48, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        draw_text(canvas, 20, y + 10, titles[row], 1, COLOR_TEXT);
        draw_text(canvas, 230, y + 10, settings_state(enabled, allowed), 1,
                  !allowed ? COLOR_MUTED : (enabled ? COLOR_YELLOW : COLOR_GREEN));
        draw_text(canvas, 20, y + 29, details[row], 1, COLOR_MUTED);
    }
    draw_text(canvas, 8, 145, authorities[ui->settings_authority], 1,
              ui->settings_authority == CP0_UI_AUTHORITY_PERSONAL ? COLOR_MUTED
                                                                  : COLOR_YELLOW);
    if (!ui->store_install_allowed || ui->app_launch_restricted ||
        ui->denied_permission_count > 0)
        draw_text(canvas, 200, 145, "RESTRICTED", 1, COLOR_YELLOW);
}

static void draw_settings_confirm(struct canvas *canvas,
                                  const struct cp0_ui *ui)
{
    static const char *labels[] = {"ENABLE", "CANCEL"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 24, 35, 272, 108, COLOR_SURFACE);
    stroke_rect(canvas, 24, 35, 272, 108, 2, COLOR_YELLOW);
    draw_text(canvas, 42, 50,
              ui->settings_confirm_recovery ? "ENABLE RECOVERY BOOT"
                                            : "ENABLE DEVELOPER MODE",
              1, COLOR_TEXT);
    draw_text(canvas, 42, 72,
              ui->settings_confirm_recovery ? "NEXT BOOT OPENS CONSOLE"
                                            : "DEV PACKAGES MAY INSTALL",
              1, COLOR_MUTED);
    for (unsigned int index = 0; index < 2; index++) {
        int x = 48 + (int)index * 116;
        bool selected = ui->dialog_selected == index;
        fill_rect(canvas, x, 105, 104, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 105, 104, 24, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text(canvas, x + 28, 114, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    switch (ui->screen) {
    case CP0_UI_APPS:
        draw_apps_page(canvas, ui);
        break;
    case CP0_UI_STORE:
        draw_store_page(canvas, ui);
        break;
    case CP0_UI_DEVICE:
        draw_empty_page(canvas, "CARDPUTER ZERO", "V0.6  320 X 170  512 MB",
                        COLOR_YELLOW);
        break;
    case CP0_UI_NETWORK:
        draw_empty_page(canvas, "NETWORK",
                        ui->network_online ? "CONNECTED" : "OFFLINE",
                        ui->network_online ? COLOR_GREEN : COLOR_RED);
        break;
    case CP0_UI_SETTINGS:
        draw_settings_page(canvas, ui);
        break;
    case CP0_UI_TASKS:
        draw_tasks_page(canvas, ui);
        break;
    case CP0_UI_HOME:
        draw_home(canvas, ui);
        break;
    }
}

static void draw_power_dialog(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"SLEEP", "RESTART", "CANCEL"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 36, 35, 248, 105, COLOR_SURFACE);
    stroke_rect(canvas, 36, 35, 248, 105, 2, COLOR_GREEN);
    draw_text(canvas, 54, 52, "POWER", 2, COLOR_TEXT);

    for (unsigned int index = 0; index < 3; index++) {
        int x = 50 + (int)index * 76;
        bool selected = index == ui->dialog_selected;
        fill_rect(canvas, x, 101, 68, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 101, 68, 24, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text(canvas, x + 7, 110, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_prompt_line(struct canvas *canvas, int y, const char *text,
                             size_t start, size_t maximum)
{
    char line[47];
    size_t length = strlen(text);
    size_t output = 0;

    while (start < length && output < maximum && output + 1 < sizeof(line)) {
        unsigned char byte = (unsigned char)text[start++];
        line[output++] = byte >= 0x20U && byte < 0x7fU ? (char)byte : ' ';
    }
    line[output] = '\0';
    draw_text(canvas, 20, y, line, 1, COLOR_TEXT);
}

static void draw_notification_banner(struct canvas *canvas,
                                     const struct cp0_ui *ui)
{
    fill_rect(canvas, 5, 24, 310, 64, COLOR_SURFACE);
    fill_rect(canvas, 5, 24, 4, 64, COLOR_GREEN);
    stroke_rect(canvas, 5, 24, 310, 64, 1, COLOR_GREEN);
    draw_prompt_line(canvas, 32, ui->notification_app_name, 0, 46);
    draw_prompt_line(canvas, 46, ui->notification_title, 0, 46);
    draw_prompt_line(canvas, 62, ui->notification_body, 0, 46);
    draw_prompt_line(canvas, 75, ui->notification_body, 46, 46);
}

static void draw_permission_dialog(struct canvas *canvas,
                                   const struct cp0_ui *ui)
{
    static const char *labels[] = {"ONCE", "ALWAYS", "DENY"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    stroke_rect(canvas, 8, 27, 304, 136, 2, COLOR_GREEN);
    draw_prompt_line(canvas, 38, ui->prompt_app_name, 0, 46);
    draw_prompt_line(canvas, 53, ui->prompt_permission, 0, 46);
    draw_prompt_line(canvas, 74, ui->prompt_reason, 0, 46);
    draw_prompt_line(canvas, 87, ui->prompt_reason, 46, 46);
    draw_prompt_line(canvas, 100, ui->prompt_reason, 92, 46);

    for (unsigned int index = 0; index < 3; index++) {
        int x = 20 + (int)index * 96;
        bool selected = index == ui->prompt_selected;
        fill_rect(canvas, x, 132, 88, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 132, 88, 24, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text(canvas, x + 16, 141, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_document_dialog(struct canvas *canvas,
                                 const struct cp0_ui *ui)
{
    unsigned int first = ui->document_selected > 3
                             ? ui->document_selected - 3
                             : 0;
    unsigned int visible = ui->document_count - first;

    if (visible > 4)
        visible = 4;
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    stroke_rect(canvas, 8, 27, 304, 136, 2, COLOR_GREEN);
    draw_prompt_line(canvas, 35, ui->document_app_name, 0, 46);
    for (unsigned int row = 0; row < visible; row++) {
        unsigned int index = first + row;
        int y = 49 + (int)row * 23;
        bool selected = index == ui->document_selected;
        char size[16];
        uint64_t bounded_size = ui->documents[index].size_bytes;
        if (bounded_size > 16U * 1024U * 1024U)
            bounded_size = 16U * 1024U * 1024U;
        unsigned int kib = (unsigned int)((bounded_size + 1023U) / 1024U);

        fill_rect(canvas, 16, y, 288, 20,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, 16, y, 288, 20, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_prompt_line(canvas, y + 7, ui->documents[index].name, 0, 31);
        snprintf(size, sizeof(size), "%uK", kib);
        draw_text(canvas, 259, y + 7, size, 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
    draw_text(canvas, 20, 149, "ENTER OPEN", 1, COLOR_GREEN);
    draw_text(canvas, 210, 149, "ESC CANCEL", 1, COLOR_MUTED);
}

void cp0_ui_init(struct cp0_ui *ui)
{
    memset(ui, 0, sizeof(*ui));
    ui->screen = CP0_UI_HOME;
    ui->store_status = CP0_UI_STORE_LOADING;
    ui->battery_percent = -1;
    memcpy(ui->clock_text, "--:--", sizeof(ui->clock_text));
}

void cp0_ui_set_status(struct cp0_ui *ui, const char *clock_text,
                       bool network_online, int battery_percent)
{
    if (clock_text != NULL && strlen(clock_text) == 5) {
        memcpy(ui->clock_text, clock_text, 5);
        ui->clock_text[5] = '\0';
    }
    ui->network_online = network_online;
    ui->battery_percent = battery_percent;
}

void cp0_ui_set_device_settings(
    struct cp0_ui *ui, enum cp0_ui_authority authority,
    bool developer_mode, bool developer_mode_allowed, bool recovery_mode,
    bool recovery_mode_allowed, bool store_install_allowed,
    bool app_launch_restricted, uint8_t denied_permission_count)
{
    if (ui == NULL || authority > CP0_UI_AUTHORITY_ORGANIZATION ||
        denied_permission_count > 8)
        return;
    ui->settings_authority = authority;
    ui->developer_mode = developer_mode && developer_mode_allowed;
    ui->developer_mode_allowed = developer_mode_allowed;
    ui->recovery_mode = recovery_mode && recovery_mode_allowed;
    ui->recovery_mode_allowed = recovery_mode_allowed;
    ui->store_install_allowed = store_install_allowed;
    ui->app_launch_restricted = app_launch_restricted;
    ui->denied_permission_count = denied_permission_count;
    ui->settings_available = true;
}

static bool copy_text(char *output, size_t capacity, const char *input)
{
    size_t length;

    if (output == NULL || capacity == 0 || input == NULL || input[0] == '\0')
        return false;
    length = strlen(input);
    if (length >= capacity)
        length = capacity - 1;
    memcpy(output, input, length);
    output[length] = '\0';
    return true;
}

static bool copy_optional_text(char *output, size_t capacity,
                               const char *input)
{
    size_t length;

    if (output == NULL || capacity == 0 || input == NULL)
        return false;
    length = strlen(input);
    if (length >= capacity)
        length = capacity - 1;
    memcpy(output, input, length);
    output[length] = '\0';
    return true;
}

static int app_index_by_id(const struct cp0_ui *ui, const char *app_id)
{
    if (ui == NULL || app_id == NULL)
        return -1;
    for (unsigned int index = 0; index < ui->app_count; index++) {
        if (strcmp(ui->apps[index].app_id, app_id) == 0)
            return (int)index;
    }
    return -1;
}

void cp0_ui_add_app(struct cp0_ui *ui, uint32_t token, const char *app_id)
{
    int found;
    unsigned int index;

    if (ui == NULL || token == 0 || app_id == NULL || app_id[0] == '\0')
        return;
    found = app_index_by_id(ui, app_id);
    if (found >= 0) {
        index = (unsigned int)found;
    } else {
        for (index = 0; index < ui->app_count; index++) {
            if (ui->apps[index].token == token)
                break;
        }
    }
    if (index == ui->app_count) {
        if (ui->app_count == CP0_UI_MAX_APPS)
            return;
        memset(&ui->apps[index], 0, sizeof(ui->apps[index]));
        if (!copy_text(ui->apps[index].app_id,
                       sizeof(ui->apps[index].app_id), app_id) ||
            !copy_text(ui->apps[index].name, sizeof(ui->apps[index].name),
                       app_id))
            return;
        ui->app_count++;
    } else if (ui->apps[index].app_id[0] == '\0') {
        if (!copy_text(ui->apps[index].app_id,
                       sizeof(ui->apps[index].app_id), app_id) ||
            !copy_text(ui->apps[index].name, sizeof(ui->apps[index].name),
                       app_id))
            return;
    }
    ui->apps[index].token = token;
    ui->apps[index].state = CP0_UI_APP_RUNNING;
}

void cp0_ui_sync_app_catalog(struct cp0_ui *ui,
                             const struct cp0_ui_catalog_app *apps,
                             size_t app_count, bool truncated)
{
    struct cp0_ui_app previous[CP0_UI_MAX_APPS];
    char selected_id[CP0_UI_APP_ID_MAX + 1] = {0};
    unsigned int previous_count;

    if (ui == NULL || (apps == NULL && app_count != 0))
        return;
    if (app_count > CP0_UI_MAX_APPS)
        app_count = CP0_UI_MAX_APPS;
    previous_count = ui->app_count;
    memcpy(previous, ui->apps, sizeof(previous));
    if (ui->app_selected < ui->app_count)
        copy_text(selected_id, sizeof(selected_id),
                  ui->apps[ui->app_selected].app_id);
    memset(ui->apps, 0, sizeof(ui->apps));
    ui->app_count = 0;
    ui->app_list_truncated = truncated;

    for (size_t source = 0; source < app_count; source++) {
        struct cp0_ui_app app = {
            .installed = true,
            .immersive = apps[source].immersive,
            .state = apps[source].running ? CP0_UI_APP_RUNNING
                                          : CP0_UI_APP_STOPPED,
        };
        if (!copy_text(app.app_id, sizeof(app.app_id), apps[source].app_id) ||
            !copy_text(app.name, sizeof(app.name), apps[source].name))
            continue;
        for (unsigned int old = 0; old < previous_count; old++) {
            if (strcmp(previous[old].app_id, app.app_id) != 0)
                continue;
            app.token = previous[old].token;
            if (app.token != 0)
                app.state = CP0_UI_APP_RUNNING;
            else if (previous[old].state == CP0_UI_APP_STARTING &&
                     apps[source].running)
                app.state = CP0_UI_APP_STARTING;
            else if (previous[old].state == CP0_UI_APP_FAILED &&
                     !apps[source].running)
                app.state = CP0_UI_APP_FAILED;
            break;
        }
        ui->apps[ui->app_count++] = app;
    }

    ui->app_selected = 0;
    for (unsigned int index = 0; index < ui->app_count; index++) {
        if (strcmp(ui->apps[index].app_id, selected_id) == 0) {
            ui->app_selected = index;
            break;
        }
    }
}

void cp0_ui_set_app_display_mode(struct cp0_ui *ui, uint32_t token,
                                 bool immersive)
{
    if (ui == NULL || token == 0)
        return;
    for (unsigned int index = 0; index < ui->app_count; index++) {
        if (ui->apps[index].token == token) {
            ui->apps[index].immersive = immersive;
            return;
        }
    }
}

void cp0_ui_remove_app(struct cp0_ui *ui, uint32_t token)
{
    unsigned int index;

    if (ui == NULL)
        return;
    for (index = 0; index < ui->app_count; index++) {
        if (ui->apps[index].token == token)
            break;
    }
    if (index == ui->app_count)
        return;

    if (ui->apps[index].installed) {
        ui->apps[index].token = 0;
        ui->apps[index].state = CP0_UI_APP_STOPPED;
        return;
    }

    if (index + 1 < ui->app_count) {
        memmove(&ui->apps[index], &ui->apps[index + 1],
                (ui->app_count - index - 1) * sizeof(ui->apps[0]));
    }
    ui->app_count--;
    if (ui->app_count == 0 || ui->app_selected >= ui->app_count)
        ui->app_selected = ui->app_count == 0 ? 0 : ui->app_count - 1;
}

void cp0_ui_set_app_state(struct cp0_ui *ui, const char *app_id,
                          enum cp0_ui_app_state state)
{
    int index = app_index_by_id(ui, app_id);
    if (index < 0 || state > CP0_UI_APP_FAILED)
        return;
    ui->apps[index].state = state;
    if (state == CP0_UI_APP_STOPPED || state == CP0_UI_APP_FAILED)
        ui->apps[index].token = 0;
}

static int store_app_index_by_id(const struct cp0_ui *ui, const char *app_id)
{
    if (ui == NULL || app_id == NULL)
        return -1;
    for (unsigned int index = 0; index < ui->store_count; index++) {
        if (strcmp(ui->store_apps[index].app_id, app_id) == 0)
            return (int)index;
    }
    return -1;
}

void cp0_ui_set_store_status(struct cp0_ui *ui,
                             enum cp0_ui_store_status status)
{
    if (ui == NULL || status > CP0_UI_STORE_UNAVAILABLE)
        return;
    ui->store_status = status;
    if (status != CP0_UI_STORE_READY)
        ui->store_detail = false;
}

void cp0_ui_sync_store_catalog(
    struct cp0_ui *ui, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count, bool truncated, bool stale)
{
    struct cp0_ui_store_app previous[CP0_UI_MAX_APPS];
    char selected_id[CP0_UI_APP_ID_MAX + 1] = {0};
    size_t previous_count;

    if (ui == NULL || (apps == NULL && app_count > 0))
        return;
    if (ui->store_selected < ui->store_count)
        copy_optional_text(selected_id, sizeof(selected_id),
                           ui->store_apps[ui->store_selected].app_id);
    previous_count = ui->store_count;
    memcpy(previous, ui->store_apps, sizeof(previous));
    if (app_count > CP0_UI_MAX_APPS)
        app_count = CP0_UI_MAX_APPS;
    memset(ui->store_apps, 0, sizeof(ui->store_apps));
    ui->store_count = 0;
    for (size_t index = 0; index < app_count; index++) {
        if (apps[index].state > CP0_UI_STORE_FAILED ||
            apps[index].progress_percent > 100)
            continue;
        struct cp0_ui_store_app app = {
            .package_bytes = apps[index].package_bytes,
            .permissions = apps[index].permissions,
            .progress_percent = apps[index].progress_percent,
            .state = apps[index].state,
        };
        if (!copy_text(app.app_id, sizeof(app.app_id), apps[index].app_id) ||
            !copy_text(app.name, sizeof(app.name), apps[index].name) ||
            !copy_text(app.version, sizeof(app.version), apps[index].version) ||
            !copy_text(app.summary, sizeof(app.summary), apps[index].summary))
            continue;
        if (app.state == CP0_UI_STORE_AVAILABLE ||
            app.state == CP0_UI_STORE_INSTALLED) {
            if (apps[index].installed_version == NULL) {
                app.state = CP0_UI_STORE_AVAILABLE;
            } else if (strcmp(apps[index].installed_version, app.version) == 0) {
                app.state = CP0_UI_STORE_INSTALLED;
            } else {
                app.state = CP0_UI_STORE_UPDATE;
            }
        }
        for (size_t old = 0; old < previous_count; old++) {
            bool old_active = previous[old].state == CP0_UI_STORE_QUEUED ||
                              previous[old].state == CP0_UI_STORE_DOWNLOADING ||
                              previous[old].state == CP0_UI_STORE_INSTALLING;
            bool new_idle = app.state == CP0_UI_STORE_AVAILABLE ||
                            app.state == CP0_UI_STORE_UPDATE;
            if (strcmp(previous[old].app_id, app.app_id) == 0 && old_active &&
                new_idle) {
                app.state = previous[old].state;
                app.progress_percent = previous[old].progress_percent;
                break;
            }
        }
        ui->store_apps[ui->store_count++] = app;
    }
    ui->store_status = CP0_UI_STORE_READY;
    ui->store_list_truncated = truncated;
    ui->store_catalog_stale = stale;
    ui->store_selected = 0;
    for (unsigned int index = 0; index < ui->store_count; index++) {
        if (strcmp(ui->store_apps[index].app_id, selected_id) == 0) {
            ui->store_selected = index;
            break;
        }
    }
    if (ui->store_count == 0)
        ui->store_detail = false;
}

void cp0_ui_set_store_app_state(struct cp0_ui *ui, const char *app_id,
                                enum cp0_ui_store_state state,
                                uint8_t progress_percent)
{
    int index = store_app_index_by_id(ui, app_id);
    if (index < 0 || state > CP0_UI_STORE_FAILED || progress_percent > 100)
        return;
    ui->store_apps[index].state = state;
    ui->store_apps[index].progress_percent = progress_percent;
}

const char *cp0_ui_selected_store_app_id(const struct cp0_ui *ui)
{
    if (ui == NULL || ui->screen != CP0_UI_STORE ||
        ui->store_selected >= ui->store_count)
        return NULL;
    return ui->store_apps[ui->store_selected].app_id;
}

enum cp0_ui_store_state cp0_ui_selected_store_app_state(
    const struct cp0_ui *ui)
{
    return ui != NULL && ui->store_selected < ui->store_count
               ? ui->store_apps[ui->store_selected].state
               : CP0_UI_STORE_AVAILABLE;
}

static int selected_app_index(const struct cp0_ui *ui)
{
    if (ui == NULL)
        return -1;
    if (ui->screen == CP0_UI_TASKS)
        return running_app_index(ui);
    return ui->app_selected < ui->app_count ? (int)ui->app_selected : -1;
}

const char *cp0_ui_selected_app_id(const struct cp0_ui *ui)
{
    int index = selected_app_index(ui);
    return index < 0 ? NULL : ui->apps[index].app_id;
}

enum cp0_ui_app_state cp0_ui_selected_app_state(const struct cp0_ui *ui)
{
    int index = selected_app_index(ui);
    return index < 0 ? CP0_UI_APP_STOPPED : ui->apps[index].state;
}

uint32_t cp0_ui_selected_app_token(const struct cp0_ui *ui)
{
    int index = selected_app_index(ui);
    return index < 0 ? 0 : ui->apps[index].token;
}

bool cp0_ui_selected_app_is_immersive(const struct cp0_ui *ui)
{
    int index = selected_app_index(ui);
    return index >= 0 && ui->apps[index].immersive;
}

bool cp0_ui_app_is_immersive(const struct cp0_ui *ui, uint32_t token)
{
    if (ui == NULL || token == 0)
        return false;
    for (unsigned int index = 0; index < ui->app_count; index++) {
        if (ui->apps[index].token == token)
            return ui->apps[index].immersive;
    }
    return false;
}

bool cp0_ui_show_permission(struct cp0_ui *ui, uint64_t prompt_id,
                            const char *app_name, const char *permission,
                            const char *reason)
{
    if (ui == NULL || prompt_id == 0 ||
        !copy_text(ui->prompt_app_name, sizeof(ui->prompt_app_name), app_name) ||
        !copy_text(ui->prompt_permission, sizeof(ui->prompt_permission),
                   permission) ||
        !copy_text(ui->prompt_reason, sizeof(ui->prompt_reason), reason))
        return false;
    ui->prompt_id = prompt_id;
    ui->prompt_selected = 0;
    ui->permission_prompt = true;
    ui->power_dialog = false;
    ui->settings_confirm = false;
    return true;
}

void cp0_ui_clear_permission(struct cp0_ui *ui)
{
    if (ui == NULL)
        return;
    ui->permission_prompt = false;
    ui->prompt_id = 0;
    ui->prompt_selected = 0;
    memset(ui->prompt_app_name, 0, sizeof(ui->prompt_app_name));
    memset(ui->prompt_permission, 0, sizeof(ui->prompt_permission));
    memset(ui->prompt_reason, 0, sizeof(ui->prompt_reason));
}

bool cp0_ui_show_documents(struct cp0_ui *ui, uint64_t prompt_id,
                           const char *app_name,
                           const struct cp0_ui_document_option *documents,
                           size_t document_count)
{
    if (ui == NULL || prompt_id == 0 || documents == NULL ||
        document_count == 0 || document_count > CP0_UI_MAX_DOCUMENTS ||
        !copy_text(ui->document_app_name, sizeof(ui->document_app_name),
                   app_name))
        return false;
    memset(ui->documents, 0, sizeof(ui->documents));
    for (size_t index = 0; index < document_count; index++) {
        if (!copy_text(ui->documents[index].document_id,
                       sizeof(ui->documents[index].document_id),
                       documents[index].document_id) ||
            strlen(ui->documents[index].document_id) !=
                CP0_UI_DOCUMENT_ID_MAX ||
            !copy_text(ui->documents[index].name,
                       sizeof(ui->documents[index].name),
                       documents[index].name)) {
            cp0_ui_clear_documents(ui);
            return false;
        }
        for (size_t byte = 0; byte < CP0_UI_DOCUMENT_ID_MAX; byte++) {
            unsigned char value =
                (unsigned char)ui->documents[index].document_id[byte];
            if (!((value >= '0' && value <= '9') ||
                  (value >= 'a' && value <= 'f'))) {
                cp0_ui_clear_documents(ui);
                return false;
            }
        }
        if (documents[index].size_bytes > 16U * 1024U * 1024U) {
            cp0_ui_clear_documents(ui);
            return false;
        }
        ui->documents[index].size_bytes = documents[index].size_bytes;
    }
    ui->document_prompt_id = prompt_id;
    ui->document_selected = 0;
    ui->document_count = (unsigned int)document_count;
    ui->document_prompt = true;
    ui->power_dialog = false;
    ui->settings_confirm = false;
    return true;
}

void cp0_ui_clear_documents(struct cp0_ui *ui)
{
    if (ui == NULL)
        return;
    ui->document_prompt = false;
    ui->document_prompt_id = 0;
    ui->document_selected = 0;
    ui->document_count = 0;
    memset(ui->document_app_name, 0, sizeof(ui->document_app_name));
    memset(ui->documents, 0, sizeof(ui->documents));
}

const char *cp0_ui_selected_document_id(const struct cp0_ui *ui)
{
    if (ui == NULL || !ui->document_prompt ||
        ui->document_selected >= ui->document_count)
        return NULL;
    return ui->documents[ui->document_selected].document_id;
}

bool cp0_ui_show_notification(struct cp0_ui *ui, uint64_t notification_id,
                              const char *app_name, const char *title,
                              const char *body)
{
    if (ui == NULL || notification_id == 0 ||
        !copy_text(ui->notification_app_name,
                   sizeof(ui->notification_app_name), app_name) ||
        !copy_text(ui->notification_title, sizeof(ui->notification_title),
                   title) ||
        !copy_optional_text(ui->notification_body,
                            sizeof(ui->notification_body), body))
        return false;
    ui->notification_id = notification_id;
    ui->notification_banner = true;
    return true;
}

void cp0_ui_clear_notification(struct cp0_ui *ui)
{
    if (ui == NULL)
        return;
    ui->notification_banner = false;
    ui->notification_id = 0;
    memset(ui->notification_app_name, 0,
           sizeof(ui->notification_app_name));
    memset(ui->notification_title, 0, sizeof(ui->notification_title));
    memset(ui->notification_body, 0, sizeof(ui->notification_body));
}

enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action)
{
    if (ui->permission_prompt) {
        if (action == CP0_UI_LEFT && ui->prompt_selected > 0)
            ui->prompt_selected--;
        else if (action == CP0_UI_RIGHT && ui->prompt_selected < 2)
            ui->prompt_selected++;
        else if (action == CP0_UI_BACK)
            return CP0_UI_EVENT_PERMISSION_DENY;
        else if (action == CP0_UI_ACCEPT) {
            static const enum cp0_ui_event choices[] = {
                CP0_UI_EVENT_PERMISSION_ONCE,
                CP0_UI_EVENT_PERMISSION_ALWAYS,
                CP0_UI_EVENT_PERMISSION_DENY,
            };
            return choices[ui->prompt_selected];
        }
        return CP0_UI_EVENT_NONE;
    }
    if (ui->document_prompt) {
        if (action == CP0_UI_UP && ui->document_selected > 0)
            ui->document_selected--;
        else if (action == CP0_UI_DOWN &&
                 ui->document_selected + 1 < ui->document_count)
            ui->document_selected++;
        else if (action == CP0_UI_BACK)
            return CP0_UI_EVENT_DOCUMENT_CANCEL;
        else if (action == CP0_UI_ACCEPT && ui->document_count > 0)
            return CP0_UI_EVENT_DOCUMENT_SELECT;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_GO_HOME) {
        ui->power_dialog = false;
        ui->settings_confirm = false;
        ui->store_detail = false;
        ui->screen = CP0_UI_HOME;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_TASKS) {
        ui->power_dialog = false;
        ui->settings_confirm = false;
        ui->screen = CP0_UI_TASKS;
        ui->task_action_selected = 0;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_POWER) {
        ui->settings_confirm = false;
        ui->power_dialog = true;
        ui->dialog_selected = 0;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->power_dialog) {
        if (action == CP0_UI_LEFT && ui->dialog_selected > 0)
            ui->dialog_selected--;
        else if (action == CP0_UI_RIGHT && ui->dialog_selected < 2)
            ui->dialog_selected++;
        else if (action == CP0_UI_BACK) {
            ui->power_dialog = false;
        } else if (action == CP0_UI_ACCEPT) {
            unsigned int selected = ui->dialog_selected;
            ui->power_dialog = false;
            if (selected == 0)
                return CP0_UI_EVENT_SLEEP;
            if (selected == 1)
                return CP0_UI_EVENT_RESTART;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->settings_confirm) {
        if (action == CP0_UI_LEFT)
            ui->dialog_selected = 0;
        else if (action == CP0_UI_RIGHT)
            ui->dialog_selected = 1;
        else if (action == CP0_UI_BACK) {
            ui->settings_confirm = false;
        } else if (action == CP0_UI_ACCEPT) {
            bool recovery = ui->settings_confirm_recovery;
            unsigned int selected = ui->dialog_selected;
            ui->settings_confirm = false;
            if (selected == 0)
                return recovery ? CP0_UI_EVENT_RECOVERY_ENABLE
                                : CP0_UI_EVENT_DEVELOPER_ENABLE;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (action == CP0_UI_BACK) {
        if (ui->screen == CP0_UI_STORE && ui->store_detail) {
            ui->store_detail = false;
            return CP0_UI_EVENT_NONE;
        }
        ui->screen = CP0_UI_HOME;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_TASKS) {
        if (action == CP0_UI_LEFT)
            ui->task_action_selected = 0;
        else if (action == CP0_UI_RIGHT)
            ui->task_action_selected = 1;
        else if (action == CP0_UI_ACCEPT && running_app_index(ui) >= 0)
            return ui->task_action_selected == 0 ? CP0_UI_EVENT_OPEN_APP
                                                 : CP0_UI_EVENT_STOP_APP;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_APPS) {
        if (action == CP0_UI_UP && ui->app_selected > 0)
            ui->app_selected--;
        else if (action == CP0_UI_DOWN &&
                 ui->app_selected + 1 < ui->app_count)
            ui->app_selected++;
        else if (action == CP0_UI_ACCEPT && ui->app_count > 0)
            return CP0_UI_EVENT_OPEN_APP;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_STORE) {
        if (ui->store_detail) {
            enum cp0_ui_store_state state =
                cp0_ui_selected_store_app_state(ui);
            if (action == CP0_UI_ACCEPT &&
                (state == CP0_UI_STORE_AVAILABLE ||
                 state == CP0_UI_STORE_UPDATE || state == CP0_UI_STORE_FAILED))
                return CP0_UI_EVENT_STORE_INSTALL;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->store_status != CP0_UI_STORE_READY)
            return action == CP0_UI_RIGHT ? CP0_UI_EVENT_STORE_REFRESH
                                          : CP0_UI_EVENT_NONE;
        if (action == CP0_UI_UP && ui->store_selected > 0)
            ui->store_selected--;
        else if (action == CP0_UI_DOWN &&
                 ui->store_selected + 1 < ui->store_count)
            ui->store_selected++;
        else if (action == CP0_UI_RIGHT)
            return CP0_UI_EVENT_STORE_REFRESH;
        else if (action == CP0_UI_ACCEPT && ui->store_count > 0)
            ui->store_detail = true;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_SETTINGS) {
        if (!ui->settings_available)
            return CP0_UI_EVENT_NONE;
        if (action == CP0_UI_UP)
            ui->settings_selected = 0;
        else if (action == CP0_UI_DOWN)
            ui->settings_selected = 1;
        else if (action == CP0_UI_ACCEPT) {
            bool recovery = ui->settings_selected == 1;
            bool allowed = recovery ? ui->recovery_mode_allowed
                                    : ui->developer_mode_allowed;
            bool enabled = recovery ? ui->recovery_mode : ui->developer_mode;
            if (!allowed)
                return CP0_UI_EVENT_NONE;
            if (enabled)
                return recovery ? CP0_UI_EVENT_RECOVERY_DISABLE
                                : CP0_UI_EVENT_DEVELOPER_DISABLE;
            ui->settings_confirm = true;
            ui->settings_confirm_recovery = recovery;
            ui->dialog_selected = 1;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen != CP0_UI_HOME)
        return CP0_UI_EVENT_NONE;

    if (action == CP0_UI_LEFT && (ui->selected % 3) != 0)
        ui->selected--;
    else if (action == CP0_UI_RIGHT && (ui->selected % 3) != 2 &&
             ui->selected + 1 < 5)
        ui->selected++;
    else if (action == CP0_UI_UP && ui->selected >= 3)
        ui->selected -= 3;
    else if (action == CP0_UI_DOWN && ui->selected + 3 < 5)
        ui->selected += 3;
    else if (action == CP0_UI_ACCEPT) {
        switch (ui->selected) {
        case 0:
            ui->screen = CP0_UI_APPS;
            break;
        case 1:
            ui->screen = CP0_UI_STORE;
            break;
        case 2:
            ui->screen = CP0_UI_DEVICE;
            break;
        case 3:
            ui->screen = CP0_UI_NETWORK;
            break;
        default:
            ui->screen = CP0_UI_SETTINGS;
            ui->settings_selected = 0;
            break;
        }
    }

    return CP0_UI_EVENT_NONE;
}

void cp0_ui_render(const struct cp0_ui *ui, uint32_t *pixels, int width,
                   int height, int stride_pixels)
{
    if (ui == NULL || pixels == NULL || width <= 0 || height <= 0 ||
        stride_pixels < width)
        return;

    struct canvas canvas = {
        .pixels = pixels,
        .width = width,
        .height = height,
        .stride = stride_pixels,
    };
    fill_rect(&canvas, 0, 0, width, height, COLOR_BG);
    draw_status_bar(&canvas, ui);
    draw_page(&canvas, ui);
    if (ui->notification_banner && !ui->power_dialog &&
        !ui->settings_confirm && !ui->permission_prompt &&
        !ui->document_prompt)
        draw_notification_banner(&canvas, ui);
    if (ui->power_dialog)
        draw_power_dialog(&canvas, ui);
    else if (ui->settings_confirm)
        draw_settings_confirm(&canvas, ui);
    if (ui->permission_prompt)
        draw_permission_dialog(&canvas, ui);
    else if (ui->document_prompt)
        draw_document_dialog(&canvas, ui);
}
