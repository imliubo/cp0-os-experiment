#include "cp0_ui.h"

#include <stddef.h>
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

static const char *screen_title(const struct cp0_ui *ui)
{
    if (ui->permission_prompt)
        return "PERMISSION";
    switch (ui->screen) {
    case CP0_UI_HOME:
        return "HOME";
    case CP0_UI_APPS:
        return "APPS";
    case CP0_UI_DEVICE:
        return "DEVICE";
    case CP0_UI_NETWORK:
        return "NETWORK";
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

static void draw_power_icon(struct canvas *canvas, int x, int y,
                            uint32_t color)
{
    stroke_rect(canvas, x + 3, y + 4, 18, 18, 2, color);
    fill_rect(canvas, x + 10, y, 4, 13, COLOR_SURFACE);
    fill_rect(canvas, x + 11, y, 2, 12, color);
}

static void draw_home(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"APPS", "DEVICE", "NETWORK", "POWER"};
    for (unsigned int index = 0; index < 4; index++) {
        int x = 8 + (int)(index % 2) * 156;
        int y = 28 + (int)(index / 2) * 68;
        bool selected = index == ui->selected;
        fill_rect(canvas, x, y, 148, 61,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, x, y, 148, 61, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        uint32_t icon_color = selected ? COLOR_GREEN : COLOR_MUTED;
        switch (index) {
        case 0:
            draw_apps_icon(canvas, x + 17, y + 20, icon_color);
            break;
        case 1:
            draw_device_icon(canvas, x + 15, y + 19, icon_color);
            break;
        case 2:
            draw_large_network_icon(canvas, x + 15, y + 18, icon_color);
            break;
        default:
            draw_power_icon(canvas, x + 15, y + 18, icon_color);
            break;
        }
        draw_text(canvas, x + 52, y + 27, labels[index], 1, COLOR_TEXT);
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
    if (ui->app_count == 0) {
        draw_empty_page(canvas, "APPS", "NO APPS INSTALLED", COLOR_GREEN);
        return;
    }

    for (unsigned int index = 0; index < ui->app_count; index++) {
        int y = 28 + (int)index * 32;
        bool selected = index == ui->app_selected;
        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        fill_rect(canvas, 17, y + 9, 10, 10,
                  selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text(canvas, 36, y + 10, ui->apps[index].app_id, 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    switch (ui->screen) {
    case CP0_UI_APPS:
        draw_apps_page(canvas, ui);
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
    case CP0_UI_TASKS:
        draw_empty_page(canvas, "TASKS", "NO RUNNING APPS", COLOR_YELLOW);
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

void cp0_ui_init(struct cp0_ui *ui)
{
    memset(ui, 0, sizeof(*ui));
    ui->screen = CP0_UI_HOME;
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

void cp0_ui_add_app(struct cp0_ui *ui, uint32_t token, const char *app_id)
{
    unsigned int index;
    size_t length;

    if (ui == NULL || token == 0 || app_id == NULL || app_id[0] == '\0')
        return;
    for (index = 0; index < ui->app_count; index++) {
        if (ui->apps[index].token == token)
            break;
    }
    if (index == ui->app_count) {
        if (ui->app_count == CP0_UI_MAX_APPS)
            return;
        ui->app_count++;
    }

    length = strlen(app_id);
    if (length > CP0_UI_APP_ID_MAX)
        length = CP0_UI_APP_ID_MAX;
    ui->apps[index].token = token;
    memcpy(ui->apps[index].app_id, app_id, length);
    ui->apps[index].app_id[length] = '\0';
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

    if (index + 1 < ui->app_count) {
        memmove(&ui->apps[index], &ui->apps[index + 1],
                (ui->app_count - index - 1) * sizeof(ui->apps[0]));
    }
    ui->app_count--;
    if (ui->app_count == 0 || ui->app_selected >= ui->app_count)
        ui->app_selected = ui->app_count == 0 ? 0 : ui->app_count - 1;
}

uint32_t cp0_ui_selected_app_token(const struct cp0_ui *ui)
{
    if (ui == NULL || ui->app_selected >= ui->app_count)
        return 0;
    return ui->apps[ui->app_selected].token;
}

bool cp0_ui_selected_app_is_immersive(const struct cp0_ui *ui)
{
    if (ui == NULL || ui->app_selected >= ui->app_count)
        return false;
    return ui->apps[ui->app_selected].immersive;
}

static bool copy_prompt_text(char *output, size_t capacity, const char *input)
{
    if (input == NULL || input[0] == '\0')
        return false;
    size_t length = strlen(input);
    if (length >= capacity)
        length = capacity - 1;
    memcpy(output, input, length);
    output[length] = '\0';
    return true;
}

bool cp0_ui_show_permission(struct cp0_ui *ui, uint64_t prompt_id,
                            const char *app_name, const char *permission,
                            const char *reason)
{
    if (ui == NULL || prompt_id == 0 ||
        !copy_prompt_text(ui->prompt_app_name, sizeof(ui->prompt_app_name),
                          app_name) ||
        !copy_prompt_text(ui->prompt_permission, sizeof(ui->prompt_permission),
                          permission) ||
        !copy_prompt_text(ui->prompt_reason, sizeof(ui->prompt_reason), reason))
        return false;
    ui->prompt_id = prompt_id;
    ui->prompt_selected = 0;
    ui->permission_prompt = true;
    ui->power_dialog = false;
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
    if (action == CP0_UI_GO_HOME) {
        ui->power_dialog = false;
        ui->screen = CP0_UI_HOME;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_TASKS) {
        ui->power_dialog = false;
        ui->screen = CP0_UI_TASKS;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_POWER) {
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

    if (action == CP0_UI_BACK) {
        ui->screen = CP0_UI_HOME;
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

    if (ui->screen != CP0_UI_HOME)
        return CP0_UI_EVENT_NONE;

    if (action == CP0_UI_LEFT && (ui->selected % 2) != 0)
        ui->selected--;
    else if (action == CP0_UI_RIGHT && (ui->selected % 2) == 0)
        ui->selected++;
    else if (action == CP0_UI_UP && ui->selected >= 2)
        ui->selected -= 2;
    else if (action == CP0_UI_DOWN && ui->selected < 2)
        ui->selected += 2;
    else if (action == CP0_UI_ACCEPT) {
        switch (ui->selected) {
        case 0:
            ui->screen = CP0_UI_APPS;
            break;
        case 1:
            ui->screen = CP0_UI_DEVICE;
            break;
        case 2:
            ui->screen = CP0_UI_NETWORK;
            break;
        default:
            ui->power_dialog = true;
            ui->dialog_selected = 0;
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
    if (ui->power_dialog)
        draw_power_dialog(&canvas, ui);
    if (ui->permission_prompt)
        draw_permission_dialog(&canvas, ui);
}
