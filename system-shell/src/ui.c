#include "cp0_ui.h"

#include <limits.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
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
    ['!' - ' '] = {0x00, 0x00, 0x5f, 0x00, 0x00},
    ['"' - ' '] = {0x00, 0x07, 0x00, 0x07, 0x00},
    ['#' - ' '] = {0x14, 0x7f, 0x14, 0x7f, 0x14},
    ['$' - ' '] = {0x24, 0x2a, 0x7f, 0x2a, 0x12},
    ['%' - ' '] = {0x23, 0x13, 0x08, 0x64, 0x62},
    ['&' - ' '] = {0x36, 0x49, 0x55, 0x22, 0x50},
    ['\'' - ' '] = {0x00, 0x05, 0x03, 0x00, 0x00},
    ['(' - ' '] = {0x00, 0x1c, 0x22, 0x41, 0x00},
    [')' - ' '] = {0x00, 0x41, 0x22, 0x1c, 0x00},
    ['*' - ' '] = {0x14, 0x08, 0x3e, 0x08, 0x14},
    ['+' - ' '] = {0x08, 0x08, 0x3e, 0x08, 0x08},
    [',' - ' '] = {0x00, 0x50, 0x30, 0x00, 0x00},
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
    [';' - ' '] = {0x00, 0x56, 0x36, 0x00, 0x00},
    ['<' - ' '] = {0x08, 0x14, 0x22, 0x41, 0x00},
    ['=' - ' '] = {0x14, 0x14, 0x14, 0x14, 0x14},
    ['>' - ' '] = {0x00, 0x41, 0x22, 0x14, 0x08},
    ['?' - ' '] = {0x02, 0x01, 0x51, 0x09, 0x06},
    ['@' - ' '] = {0x32, 0x49, 0x79, 0x41, 0x3e},
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
    ['[' - ' '] = {0x00, 0x7f, 0x41, 0x41, 0x00},
    ['\\' - ' '] = {0x02, 0x04, 0x08, 0x10, 0x20},
    [']' - ' '] = {0x00, 0x41, 0x41, 0x7f, 0x00},
    ['^' - ' '] = {0x04, 0x02, 0x01, 0x02, 0x04},
    ['_' - ' '] = {0x40, 0x40, 0x40, 0x40, 0x40},
    ['`' - ' '] = {0x00, 0x01, 0x02, 0x04, 0x00},
    ['a' - ' '] = {0x20, 0x54, 0x54, 0x54, 0x78},
    ['b' - ' '] = {0x7f, 0x48, 0x44, 0x44, 0x38},
    ['c' - ' '] = {0x38, 0x44, 0x44, 0x44, 0x20},
    ['d' - ' '] = {0x38, 0x44, 0x44, 0x48, 0x7f},
    ['e' - ' '] = {0x38, 0x54, 0x54, 0x54, 0x18},
    ['f' - ' '] = {0x08, 0x7e, 0x09, 0x01, 0x02},
    ['g' - ' '] = {0x0c, 0x52, 0x52, 0x52, 0x3e},
    ['h' - ' '] = {0x7f, 0x08, 0x04, 0x04, 0x78},
    ['i' - ' '] = {0x00, 0x44, 0x7d, 0x40, 0x00},
    ['j' - ' '] = {0x20, 0x40, 0x44, 0x3d, 0x00},
    ['k' - ' '] = {0x7f, 0x10, 0x28, 0x44, 0x00},
    ['l' - ' '] = {0x00, 0x41, 0x7f, 0x40, 0x00},
    ['m' - ' '] = {0x7c, 0x04, 0x18, 0x04, 0x78},
    ['n' - ' '] = {0x7c, 0x08, 0x04, 0x04, 0x78},
    ['o' - ' '] = {0x38, 0x44, 0x44, 0x44, 0x38},
    ['p' - ' '] = {0x7c, 0x14, 0x14, 0x14, 0x08},
    ['q' - ' '] = {0x08, 0x14, 0x14, 0x18, 0x7c},
    ['r' - ' '] = {0x7c, 0x08, 0x04, 0x04, 0x08},
    ['s' - ' '] = {0x48, 0x54, 0x54, 0x54, 0x20},
    ['t' - ' '] = {0x04, 0x3f, 0x44, 0x40, 0x20},
    ['u' - ' '] = {0x3c, 0x40, 0x40, 0x20, 0x7c},
    ['v' - ' '] = {0x1c, 0x20, 0x40, 0x20, 0x1c},
    ['w' - ' '] = {0x3c, 0x40, 0x30, 0x40, 0x3c},
    ['x' - ' '] = {0x44, 0x28, 0x10, 0x28, 0x44},
    ['y' - ' '] = {0x0c, 0x50, 0x50, 0x50, 0x3c},
    ['z' - ' '] = {0x44, 0x64, 0x54, 0x4c, 0x44},
    ['{' - ' '] = {0x00, 0x08, 0x36, 0x41, 0x00},
    ['|' - ' '] = {0x00, 0x00, 0x7f, 0x00, 0x00},
    ['}' - ' '] = {0x00, 0x41, 0x36, 0x08, 0x00},
    ['~' - ' '] = {0x08, 0x04, 0x08, 0x10, 0x08},
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

static void draw_scaled_image(struct canvas *canvas, int x, int y,
                              int destination_width, int destination_height,
                              const uint32_t *pixels, unsigned int width,
                              unsigned int height)
{
    if (pixels == NULL || width == 0 || height == 0 ||
        destination_width <= 0 || destination_height <= 0)
        return;
    for (int dy = 0; dy < destination_height; dy++) {
        unsigned int source_y =
            (unsigned int)(((uint64_t)dy * height) /
                           (unsigned int)destination_height);
        int py = y + dy;
        if (py < 0 || py >= canvas->height)
            continue;
        for (int dx = 0; dx < destination_width; dx++) {
            int px = x + dx;
            if (px < 0 || px >= canvas->width)
                continue;
            unsigned int source_x =
                (unsigned int)(((uint64_t)dx * width) /
                               (unsigned int)destination_width);
            uint32_t source = pixels[(size_t)source_y * width + source_x];
            unsigned int alpha = source >> 24U;
            if (alpha == 0)
                continue;
            uint32_t *destination =
                &canvas->pixels[py * canvas->stride + px];
            if (alpha == 255U) {
                *destination = source & 0x00ffffffU;
                continue;
            }
            unsigned int inverse = 255U - alpha;
            unsigned int red = (((source >> 16U) & 0xffU) * alpha +
                                ((*destination >> 16U) & 0xffU) * inverse) /
                               255U;
            unsigned int green = (((source >> 8U) & 0xffU) * alpha +
                                  ((*destination >> 8U) & 0xffU) * inverse) /
                                 255U;
            unsigned int blue = ((source & 0xffU) * alpha +
                                 (*destination & 0xffU) * inverse) /
                                255U;
            *destination = (red << 16U) | (green << 8U) | blue;
        }
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

static void draw_glyph_exact(struct canvas *canvas, int x, int y,
                             char character, int scale, uint32_t color)
{
    if (character < ' ' || character > '~')
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

static void draw_glyph(struct canvas *canvas, int x, int y, char character,
                       int scale, uint32_t color)
{
    if (character < ' ' || character > '~')
        character = ' ';
    draw_glyph_exact(canvas, x, y, character, scale, color);
}

static void draw_text(struct canvas *canvas, int x, int y, const char *text,
                      int scale, uint32_t color)
{
    for (; *text != '\0'; text++) {
        draw_glyph(canvas, x, y, *text, scale, color);
        x += 6 * scale;
    }
}

static void draw_text_exact(struct canvas *canvas, int x, int y,
                            const char *text, int scale, uint32_t color)
{
    for (; *text != '\0'; text++) {
        draw_glyph_exact(canvas, x, y, *text, scale, color);
        x += 6 * scale;
    }
}

static void draw_prompt_line(struct canvas *canvas, int y, const char *text,
                             size_t start, size_t maximum);
static void draw_text_slice(struct canvas *canvas, int x, int y,
                            const char *text, size_t start, size_t maximum,
                            uint32_t color);
static size_t wrapped_line_start(const char *text, size_t line,
                                 size_t maximum);
static size_t wrapped_line_count(const char *text, size_t maximum);
static void draw_wrapped_line(struct canvas *canvas, int x, int y,
                              const char *text, size_t line, size_t maximum,
                              uint32_t color);
static bool copy_optional_text(char *output, size_t capacity,
                               const char *input);
static void format_bytes(char output[16], uint64_t bytes);

static void clear_sensitive(void *memory, size_t size)
{
    volatile unsigned char *bytes = memory;

    while (size-- > 0)
        *bytes++ = 0;
}

static void clear_password_change_secrets(struct cp0_ui *ui)
{
    if (ui->password_secrets == NULL)
        return;
    clear_sensitive(ui->password_secrets, sizeof(*ui->password_secrets));
}

static void cancel_password_change(struct cp0_ui *ui)
{
    clear_password_change_secrets(ui);
    ui->password_change_active = false;
    ui->password_change_show = false;
    ui->password_change_page = CP0_UI_PASSWORD_CURRENT;
}

char cp0_ui_key_character(uint32_t key, bool shifted)
{
    char character = '\0';

    /* Standard Linux evdev keycodes using the US printable layout. */
    switch (key) {
    case 30: character = 'a'; break;
    case 48: character = 'b'; break;
    case 46: character = 'c'; break;
    case 32: character = 'd'; break;
    case 18: character = 'e'; break;
    case 33: character = 'f'; break;
    case 34: character = 'g'; break;
    case 35: character = 'h'; break;
    case 23: character = 'i'; break;
    case 36: character = 'j'; break;
    case 37: character = 'k'; break;
    case 38: character = 'l'; break;
    case 50: character = 'm'; break;
    case 49: character = 'n'; break;
    case 24: character = 'o'; break;
    case 25: character = 'p'; break;
    case 16: character = 'q'; break;
    case 19: character = 'r'; break;
    case 31: character = 's'; break;
    case 20: character = 't'; break;
    case 22: character = 'u'; break;
    case 47: character = 'v'; break;
    case 17: character = 'w'; break;
    case 45: character = 'x'; break;
    case 21: character = 'y'; break;
    case 44: character = 'z'; break;
    default: break;
    }
    if (character != '\0')
        return shifted ? (char)(character - 'a' + 'A') : character;
    if (key >= 2 && key <= 10) {
        static const char shifted_digits[] = "!@#$%^&*(";
        return shifted ? shifted_digits[key - 2]
                       : (char)('1' + (key - 2));
    }
    if (key == 11)
        return shifted ? ')' : '0';
    switch (key) {
    case 12: return shifted ? '_' : '-';
    case 13: return shifted ? '+' : '=';
    case 26: return shifted ? '{' : '[';
    case 27: return shifted ? '}' : ']';
    case 39: return shifted ? ':' : ';';
    case 40: return shifted ? '"' : '\'';
    case 41: return shifted ? '~' : '`';
    case 43: return shifted ? '|' : '\\';
    case 51: return shifted ? '<' : ',';
    case 52: return shifted ? '>' : '.';
    case 53: return shifted ? '?' : '/';
    case 57: return ' ';
    default: return '\0';
    }
}

static const char *screen_title(const struct cp0_ui *ui)
{
    if (ui->setup_active)
        return "SETUP";
    if (ui->permission_prompt)
        return "PERMISSION";
    if (ui->document_prompt)
        return "DOCUMENT";
    if (ui->foreground_app_name[0] != '\0')
        return ui->foreground_app_name;
    switch (ui->screen) {
    case CP0_UI_HOME:
        return "HOME";
    case CP0_UI_APPS:
        return ui->app_detail ? "APP DETAIL" : "APPS";
    case CP0_UI_STORE:
        return ui->store_detail ? "STORE APP" : "STORE";
    case CP0_UI_DEVICE:
        return "DEVICE";
    case CP0_UI_NETWORK:
        return "NETWORK";
    case CP0_UI_SETTINGS:
        if (ui->password_change_active)
            return "CHANGE PASSWORD";
        if (ui->developer_hosts_view)
            return "PAIRED COMPUTERS";
        if (ui->settings_detail) {
            static const char *categories[] = {
                "CONNECTIVITY", "DISPLAY", "SOUND",  "CAMERA",
                "POWER",        "APPS & PRIVACY", "SYSTEM", "SECURITY",
            };
            return ui->settings_selected < 8 ? categories[ui->settings_selected]
                                             : "SETTINGS";
        }
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
    char store_status[12];

    fill_rect(canvas, 0, 0, CP0_UI_WIDTH, 21, COLOR_BAR);
    fill_rect(canvas, 0, 20, CP0_UI_WIDTH, 1, COLOR_GREEN);
    draw_text(canvas, 8, 7, screen_title(ui), 1, COLOR_TEXT);
    if (ui->store_activity) {
        if (ui->store_activity_state == CP0_UI_STORE_DOWNLOADING)
            snprintf(store_status, sizeof(store_status), "DL %u%%",
                     ui->store_activity_progress_percent);
        else if (ui->store_activity_state == CP0_UI_STORE_INSTALLING)
            snprintf(store_status, sizeof(store_status), "INSTALL");
        else
            snprintf(store_status, sizeof(store_status), "QUEUE %u",
                     ui->store_activity_count);
        draw_text(canvas, 92, 7, store_status, 1,
                  ui->store_activity_state == CP0_UI_STORE_DOWNLOADING
                      ? COLOR_GREEN
                      : COLOR_YELLOW);
    }
    if (!ui->setup_active) {
        draw_text(canvas, 145, 7, ui->clock_text, 1, COLOR_MUTED);
        draw_network_icon(canvas, ui->network_online);
    }
    draw_battery(canvas, ui->battery_percent);
}

static void draw_setup_footer(struct canvas *canvas, const char *left,
                              const char *right)
{
    fill_rect(canvas, 0, 148, CP0_UI_WIDTH, 22, COLOR_BAR);
    draw_text(canvas, 8, 156, left, 1, COLOR_MUTED);
    if (right != NULL)
        draw_text(canvas, 220, 156, right, 1, COLOR_GREEN);
}

static void draw_setup_choice(struct canvas *canvas, int y, const char *text,
                              bool selected)
{
    fill_rect(canvas, 12, y, 296, 25,
              selected ? COLOR_SELECTED : COLOR_SURFACE);
    stroke_rect(canvas, 12, y, 296, 25, selected ? 2 : 1,
                selected ? COLOR_GREEN : COLOR_BAR);
    draw_text(canvas, 22, y + 9, text, 1,
              selected ? COLOR_TEXT : COLOR_MUTED);
}

static void draw_setup_input(struct canvas *canvas, const char *value,
                             bool masked)
{
    char visible[45];
    size_t length = strlen(value);
    size_t first = length > sizeof(visible) - 2U
                       ? length - (sizeof(visible) - 2U)
                       : 0;
    size_t shown = length - first;
    if (shown >= sizeof(visible))
        shown = sizeof(visible) - 1U;
    for (size_t index = 0; index < shown; index++)
        visible[index] = masked ? '*' : value[first + index];
    visible[shown] = '\0';
    fill_rect(canvas, 12, 82, 296, 34, COLOR_SURFACE);
    stroke_rect(canvas, 12, 82, 296, 34, 2, COLOR_GREEN);
    draw_text_exact(canvas, 22, 95, visible, 1, COLOR_TEXT);
    fill_rect(canvas, 22 + (int)shown * 6, 104, 5, 2, COLOR_GREEN);
}

static void draw_setup(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *country_names[] = {
        "CHINA", "UNITED STATES", "UNITED KINGDOM", "GERMANY", "JAPAN",
    };
    static const char *timezone_names[] = {
        "ASIA/SHANGHAI", "AMERICA/LOS ANGELES", "EUROPE/LONDON",
        "EUROPE/BERLIN", "ASIA/TOKYO",
    };
    char line[64];
    draw_text(canvas, 12, 31, "CARDPUTERZERO", 2, COLOR_TEXT);
    if (ui->setup_busy) {
        draw_text(canvas, 12, 64, ui->setup_busy_title, 2, COLOR_GREEN);
        draw_text(canvas, 12, 99, ui->setup_busy_detail, 1, COLOR_MUTED);
        draw_setup_footer(canvas, "PLEASE WAIT", NULL);
        return;
    }
    switch (ui->setup_page) {
    case CP0_UI_SETUP_WELCOME:
        draw_text(canvas, 12, 63, "WELCOME", 2, COLOR_GREEN);
        draw_text(canvas, 12, 91, "SET UP YOUR DEVICE", 1, COLOR_MUTED);
        draw_text(canvas, 12, 112, "OWNER AND NETWORK ARE NOT PRESET", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "PRIVATE BY DEFAULT", "ENTER START");
        break;
    case CP0_UI_SETUP_LANGUAGE:
        draw_text(canvas, 12, 58, "REGIONAL LOCALE", 1, COLOR_MUTED);
        draw_setup_choice(canvas, 78, "ENGLISH (US)",
                          ui->setup_language == 0);
        draw_setup_choice(canvas, 108, "CHINESE (SIMPLIFIED)",
                          ui->setup_language == 1);
        draw_setup_footer(canvas, "LEFT/RIGHT CHANGE", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_COUNTRY:
        draw_text(canvas, 12, 58, "WI-FI REGULATORY COUNTRY", 1, COLOR_MUTED);
        draw_setup_choice(canvas, 82, country_names[ui->setup_country], true);
        draw_text(canvas, 12, 121, "UP/DOWN CHANGES LEGAL CHANNELS", 1,
                  COLOR_YELLOW);
        draw_setup_footer(canvas, "ESC BACK", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_TIMEZONE:
        draw_text(canvas, 12, 58, "TIME ZONE", 1, COLOR_MUTED);
        draw_setup_choice(canvas, 82, timezone_names[ui->setup_timezone], true);
        draw_setup_footer(canvas, "LEFT/RIGHT CHANGE", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_HOSTNAME:
        draw_text(canvas, 12, 58, "DEVICE NAME", 1, COLOR_MUTED);
        draw_setup_input(canvas, ui->setup_hostname, false);
        draw_text(canvas, 12, 127, "EXAMPLE: CP0-MYNAME", 1, COLOR_MUTED);
        draw_setup_footer(canvas, "BACKSPACE EDIT", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_DISPLAY_NAME:
        draw_text(canvas, 12, 58, "OWNER DISPLAY NAME", 1, COLOR_MUTED);
        draw_setup_input(canvas, ui->setup_display_name, false);
        draw_setup_footer(canvas, "BACKSPACE EDIT", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_USERNAME:
        draw_text(canvas, 12, 58, "OWNER USERNAME", 1, COLOR_MUTED);
        draw_setup_input(canvas, ui->setup_username, false);
        draw_text(canvas, 12, 127, "LOWERCASE LETTERS, NUMBERS, - OR _", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "BACKSPACE EDIT", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_PASSWORD:
    case CP0_UI_SETUP_PASSWORD_CONFIRM:
        draw_text(canvas, 12, 58,
                  ui->setup_page == CP0_UI_SETUP_PASSWORD ? "OWNER PASSWORD"
                                                         : "CONFIRM PASSWORD",
                  1, COLOR_MUTED);
        draw_setup_input(canvas,
                         ui->setup_page == CP0_UI_SETUP_PASSWORD
                             ? ui->setup_password
                             : ui->setup_password_confirm,
                         !ui->setup_show_password);
        snprintf(line, sizeof(line), "%s  %u/10 MIN",
                 ui->setup_show_password ? "VISIBLE" : "HIDDEN",
                 (unsigned int)strlen(
                     ui->setup_page == CP0_UI_SETUP_PASSWORD
                         ? ui->setup_password
                         : ui->setup_password_confirm));
        draw_text(canvas, 12, 127, line, 1, COLOR_MUTED);
        draw_setup_footer(canvas, "RIGHT SHOW/HIDE", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_NETWORK: {
        char choices[3][48] = {{0}};
        const char *wifi_state = ui->setup_wifi_link_connected
                                     ? (ui->setup_wifi_ipv4[0] != '\0'
                                            ? ui->setup_wifi_ipv4
                                            : "CONNECTED")
                                     : (ui->setup_wifi_available ? "READY"
                                                                 : "UNAVAILABLE");
        if (!ui->setup_network_manager_available) {
            snprintf(choices[0], sizeof(choices[0]), "ETHERNET UNAVAILABLE");
            snprintf(choices[1], sizeof(choices[1]), "WI-FI UNAVAILABLE");
        } else if (ui->setup_ethernet_connected) {
            snprintf(choices[0], sizeof(choices[0]), "ETHERNET %s",
                     ui->setup_ethernet_ipv4[0] != '\0'
                         ? ui->setup_ethernet_ipv4
                         : "WAITING FOR IP");
            snprintf(choices[1], sizeof(choices[1]), "WI-FI %s", wifi_state);
        } else {
            snprintf(choices[0], sizeof(choices[0]), "ETHERNET NOT CONNECTED");
            snprintf(choices[1], sizeof(choices[1]), "WI-FI %s", wifi_state);
        }
        snprintf(choices[2], sizeof(choices[2]), "USE OFFLINE");
        draw_text(canvas, 12, 55, "CONNECT THIS DEVICE", 1, COLOR_MUTED);
        for (unsigned int index = 0; index < 3; index++)
            draw_setup_choice(canvas, 70 + (int)index * 27, choices[index],
                              index == ui->setup_network);
        draw_setup_footer(canvas, "LIVE NETWORK STATUS", "ENTER SELECT");
        break;
    }
    case CP0_UI_SETUP_WIFI_LIST: {
        draw_text(canvas, 12, 52, "WI-FI NETWORKS", 1, COLOR_MUTED);
        if (ui->setup_wifi_count == 0) {
            draw_text(canvas, 12, 82, "NO NETWORKS FOUND", 1, COLOR_YELLOW);
            draw_text(canvas, 12, 104, "PRESS RIGHT TO REFRESH", 1,
                      COLOR_MUTED);
        } else {
            unsigned int first = ui->setup_wifi_selected > 2
                                     ? ui->setup_wifi_selected - 2
                                     : 0;
            unsigned int visible = ui->setup_wifi_count - first;
            if (visible > 3)
                visible = 3;
            for (unsigned int row = 0; row < visible; row++) {
                unsigned int index = first + row;
                const char *security =
                    ui->setup_wifi_security[index] == 0
                        ? "OPEN"
                        : (ui->setup_wifi_security[index] == 3 ? "UNSUPPORTED"
                                                              : "LOCK");
                snprintf(line, sizeof(line), "%.28s  %u%% %s",
                         ui->setup_wifi_ssids[index],
                         ui->setup_wifi_signal[index], security);
                draw_setup_choice(canvas, 66 + (int)row * 27, line,
                                  index == ui->setup_wifi_selected);
            }
        }
        draw_setup_footer(canvas, "RIGHT REFRESH", "ENTER CONNECT");
        break;
    }
    case CP0_UI_SETUP_WIFI_PASSWORD:
        draw_text(canvas, 12, 52, "WI-FI PASSWORD", 1, COLOR_MUTED);
        snprintf(line, sizeof(line), "%.44s", ui->setup_wifi_count > 0
                                                   ? ui->setup_wifi_ssids
                                                         [ui->setup_wifi_selected]
                                                   : "NETWORK");
        draw_text(canvas, 12, 66, line, 1, COLOR_TEXT);
        draw_setup_input(canvas, ui->setup_wifi_password,
                         !ui->setup_show_password);
        draw_text(canvas, 12, 127, "8 TO 63 CHARACTERS", 1, COLOR_MUTED);
        draw_setup_footer(canvas, "RIGHT SHOW/HIDE", "ENTER CONNECT");
        break;
    case CP0_UI_SETUP_SSH:
        draw_text(canvas, 12, 58, "OWNER SSH SHELL", 1, COLOR_MUTED);
        draw_setup_choice(canvas, 82,
                          ui->setup_ssh_enabled ? "ON" : "OFF (RECOMMENDED)",
                          true);
        draw_text(canvas, 12, 121, "FULL SHELL; DEVELOPER ACCESS IS SEPARATE", 1,
                  COLOR_YELLOW);
        draw_setup_footer(canvas, "LEFT/RIGHT CHANGE", "ENTER NEXT");
        break;
    case CP0_UI_SETUP_REVIEW:
        draw_text(canvas, 12, 54, "REVIEW", 1, COLOR_GREEN);
        snprintf(line, sizeof(line), "OWNER  %.31s", ui->setup_username);
        draw_text(canvas, 12, 72, line, 1, COLOR_TEXT);
        snprintf(line, sizeof(line), "DEVICE %.38s", ui->setup_hostname);
        draw_text(canvas, 12, 90, line, 1, COLOR_TEXT);
        snprintf(line, sizeof(line), "NETWORK %s",
                 ui->setup_network == 0
                     ? "ETHERNET"
                     : (ui->setup_network == 1 ? "WI-FI" : "OFFLINE"));
        draw_text(canvas, 12, 108, line, 1, COLOR_TEXT);
        snprintf(line, sizeof(line), "SSH     %s",
                 ui->setup_ssh_enabled ? "ON" : "OFF");
        draw_text(canvas, 12, 126, line, 1, COLOR_TEXT);
        draw_setup_footer(canvas, "ESC CHANGE", "ENTER APPLY");
        break;
    case CP0_UI_SETUP_APPLYING:
        draw_text(canvas, 12, 64, "APPLYING", 2, COLOR_GREEN);
        draw_text(canvas, 12, 99, "VERIFYING OWNER AND SYSTEM SETTINGS", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "DO NOT POWER OFF", NULL);
        break;
    case CP0_UI_SETUP_COMPLETE:
        draw_text(canvas, 12, 62, "SETUP COMPLETE", 2, COLOR_GREEN);
        snprintf(line, sizeof(line), "DEVICE %.42s", ui->setup_hostname);
        draw_text(canvas, 12, 91, line, 1, COLOR_TEXT);
        if (ui->setup_network == 0 && ui->setup_ethernet_ipv4[0] != '\0')
            snprintf(line, sizeof(line), "IP     %s", ui->setup_ethernet_ipv4);
        else if (ui->setup_network == 1 && ui->setup_wifi_ipv4[0] != '\0')
            snprintf(line, sizeof(line), "IP     %s", ui->setup_wifi_ipv4);
        else
            snprintf(line, sizeof(line), "NETWORK %s",
                     ui->setup_network == 2 ? "OFFLINE" : "NO IPv4");
        draw_text(canvas, 12, 109, line, 1, COLOR_TEXT);
        draw_text(canvas, 12, 127,
                  ui->setup_ssh_enabled ? "SSH ENABLED" : "SSH DISABLED", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "READY", "ENTER START");
        break;
    case CP0_UI_SETUP_ERROR:
        draw_text(canvas, 12, 55, "SETUP NEEDS ATTENTION", 1, COLOR_RED);
        draw_wrapped_line(canvas, 12, 80, ui->setup_error, 0, 48, COLOR_TEXT);
        draw_wrapped_line(canvas, 12, 96, ui->setup_error, 1, 48, COLOR_TEXT);
        draw_setup_footer(canvas, "CHECK AND RETRY", "ENTER RETRY");
        break;
    case CP0_UI_SETUP_REPAIR:
        draw_text(canvas, 12, 55, "RECOVERY REQUIRED", 1, COLOR_RED);
        draw_text(canvas, 12, 82, "SETUP DATA IS INCONSISTENT", 1, COLOR_TEXT);
        draw_text(canvas, 12, 103, "USE THE RECOVERY IMAGE OR FACTORY RESET", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "OWNER DATA IS LOCKED", NULL);
        break;
    }
    if (ui->setup_error[0] != '\0' &&
        ui->setup_page != CP0_UI_SETUP_ERROR &&
        ui->setup_page != CP0_UI_SETUP_REPAIR) {
        fill_rect(canvas, 8, 133, 304, 14, COLOR_BG);
        draw_text_slice(canvas, 12, 136, ui->setup_error, 0, 48, COLOR_RED);
    }
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
    _Static_assert(sizeof(labels) / sizeof(labels[0]) ==
                       CP0_UI_HOME_ITEM_COUNT,
                   "Home labels must match the navigation item count");
    for (unsigned int index = 0; index < CP0_UI_HOME_ITEM_COUNT; index++) {
        int x = 8 + (int)(index % CP0_UI_HOME_COLUMNS) * 103;
        int y = 28 + (int)(index / CP0_UI_HOME_COLUMNS) * 68;
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

static void draw_apps_view_switch(struct canvas *canvas,
                                  const struct cp0_ui *ui)
{
    draw_text(canvas, 9, 28, "TAB", 1, COLOR_MUTED);
    fill_rect(canvas, 224, 24, 44, 14,
              ui->app_grid_view ? COLOR_BAR : COLOR_SELECTED);
    fill_rect(canvas, 268, 24, 44, 14,
              ui->app_grid_view ? COLOR_SELECTED : COLOR_BAR);
    stroke_rect(canvas, 224, 24, 88, 14, 1, COLOR_MUTED);
    draw_text(canvas, 231, 28, "LIST", 1,
              ui->app_grid_view ? COLOR_MUTED : COLOR_TEXT);
    draw_text(canvas, 277, 28, "GRID", 1,
              ui->app_grid_view ? COLOR_TEXT : COLOR_MUTED);
}

static void draw_app_icon(struct canvas *canvas, const struct cp0_ui_app *app,
                          int x, int y, uint32_t color)
{
    const char *id = app->app_id;

    if (strstr(id, "calculator") != NULL) {
        stroke_rect(canvas, x + 7, y + 4, 26, 32, 2, color);
        fill_rect(canvas, x + 11, y + 8, 18, 6, color);
        for (int row = 0; row < 3; row++)
            for (int column = 0; column < 3; column++)
                fill_rect(canvas, x + 11 + column * 7, y + 18 + row * 5,
                          4, 3, color);
    } else if (strstr(id, "camera") != NULL) {
        stroke_rect(canvas, x + 4, y + 11, 32, 23, 2, color);
        fill_rect(canvas, x + 11, y + 7, 12, 5, color);
        stroke_rect(canvas, x + 15, y + 16, 11, 11, 2, color);
    } else if (strstr(id, "gallery") != NULL) {
        stroke_rect(canvas, x + 5, y + 7, 27, 25, 2, color);
        stroke_rect(canvas, x + 9, y + 11, 27, 25, 1, color);
        fill_rect(canvas, x + 11, y + 13, 5, 5, color);
        fill_rect(canvas, x + 12, y + 27, 18, 2, color);
        fill_rect(canvas, x + 17, y + 22, 13, 2, color);
    } else if (strstr(id, "snake") != NULL) {
        fill_rect(canvas, x + 7, y + 8, 6, 6, color);
        fill_rect(canvas, x + 12, y + 13, 6, 6, color);
        fill_rect(canvas, x + 17, y + 18, 12, 6, color);
        fill_rect(canvas, x + 24, y + 23, 6, 9, color);
        fill_rect(canvas, x + 28, y + 27, 5, 5, color);
    } else if (strstr(id, "media") != NULL) {
        for (int row = 0; row < 19; row++)
            fill_rect(canvas, x + 9 + row / 2, y + 10 + row, 2, 1, color);
        fill_rect(canvas, x + 28, y + 10, 3, 19, color);
    } else if (strstr(id, "notes") != NULL) {
        stroke_rect(canvas, x + 8, y + 5, 24, 31, 2, color);
        for (int row = 0; row < 4; row++)
            fill_rect(canvas, x + 13, y + 12 + row * 6, 14, 2, color);
    } else if (strstr(id, "stopwatch") != NULL) {
        stroke_rect(canvas, x + 8, y + 9, 24, 25, 2, color);
        fill_rect(canvas, x + 16, y + 5, 8, 4, color);
        fill_rect(canvas, x + 19, y + 14, 2, 9, color);
        fill_rect(canvas, x + 19, y + 22, 7, 2, color);
    } else if (strstr(id, "hello") != NULL) {
        stroke_rect(canvas, x + 5, y + 8, 30, 24, 2, color);
        fill_rect(canvas, x + 11, y + 15, 3, 3, color);
        fill_rect(canvas, x + 26, y + 15, 3, 3, color);
        fill_rect(canvas, x + 13, y + 24, 14, 2, color);
    } else {
        char monogram[2] = {'A', '\0'};
        for (size_t index = 0; app->name[index] != '\0'; index++) {
            char character = app->name[index];
            if (character >= 'A' && character <= 'Z') {
                monogram[0] = character;
                break;
            }
            if (character >= 'a' && character <= 'z') {
                monogram[0] = (char)(character - 'a' + 'A');
                break;
            }
        }
        draw_text(canvas, x + 14, y + 12, monogram, 2, color);
    }
}

static void draw_apps_grid(struct canvas *canvas, const struct cp0_ui *ui)
{
    unsigned int first = (ui->app_selected / 8U) * 8U;
    unsigned int visible = ui->app_count - first;
    if (visible > 8U)
        visible = 8U;

    for (unsigned int offset = 0; offset < visible; offset++) {
        unsigned int index = first + offset;
        int x = 8 + (int)(offset % 4U) * 76;
        int y = 41 + (int)(offset / 4U) * 58;
        bool selected = index == ui->app_selected;
        uint32_t state_color =
            ui->apps[index].state == CP0_UI_APP_RUNNING ? COLOR_GREEN
                                                        : COLOR_MUTED;
        fill_rect(canvas, x, y, 72, 55,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        if (selected)
            stroke_rect(canvas, x, y, 72, 55, 2, COLOR_YELLOW);
        stroke_rect(canvas, x + 16, y + 3, 40, 40, 2, state_color);
        draw_app_icon(canvas, &ui->apps[index], x + 16, y + 3,
                      selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text_slice(canvas, x + 6, y + 47, ui->apps[index].name, 0, 10,
                        selected ? COLOR_TEXT : COLOR_MUTED);
    }
    if (ui->app_count > 8U) {
        char page[24];
        unsigned int pages = (ui->app_count + 7U) / 8U;
        snprintf(page, sizeof(page), "%u/%u", first / 8U + 1U, pages);
        draw_text(canvas, 282, 159, page, 1, COLOR_MUTED);
    }
    if (ui->app_list_truncated)
        draw_text(canvas, 8, 159, "32+", 1, COLOR_YELLOW);
}

static void draw_apps_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *states[] = {"READY", "STARTING", "RUNNING", "FAILED"};
    if (ui->app_count == 0) {
        draw_empty_page(canvas, "APPS", "NO APPS INSTALLED", COLOR_GREEN);
        return;
    }

    draw_apps_view_switch(canvas, ui);
    if (ui->app_grid_view) {
        draw_apps_grid(canvas, ui);
        return;
    }

    unsigned int first = ui->app_selected > 3 ? ui->app_selected - 3 : 0;
    unsigned int visible = ui->app_count - first;
    if (visible > 4)
        visible = 4;
    for (unsigned int row = 0; row < visible; row++) {
        unsigned int index = first + row;
        int y = 42 + (int)row * 29;
        bool selected = index == ui->app_selected;
        fill_rect(canvas, 8, y, 304, 25,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 25, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        fill_rect(canvas, 17, y + 8, 10, 10,
                  ui->apps[index].state == CP0_UI_APP_RUNNING
                      ? COLOR_GREEN
                      : (selected ? COLOR_YELLOW : COLOR_MUTED));
        draw_text_slice(canvas, 36, y + 6, ui->apps[index].name, 0, 28,
                        selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text(canvas, 218, y + 15, states[ui->apps[index].state], 1,
                  ui->apps[index].state == CP0_UI_APP_FAILED
                      ? COLOR_RED
                      : COLOR_MUTED);
    }
    if (ui->app_list_truncated)
        draw_text(canvas, 284, 159, "32+", 1, COLOR_YELLOW);
}

static void draw_labeled_value(struct canvas *canvas, int y, const char *label,
                               const char *value, uint32_t color)
{
    draw_text(canvas, 20, y, label, 1, COLOR_MUTED);
    draw_text_slice(canvas, 142, y, value, 0, 27, color);
}

static void format_install_date(char output[20], uint64_t seconds)
{
    if (seconds == 0 || seconds > 253402300799ULL) {
        snprintf(output, 20, "UNKNOWN");
        return;
    }
    int64_t days = (int64_t)(seconds / 86400U) + 719468;
    int64_t era = days / 146097;
    unsigned int day_of_era = (unsigned int)(days - era * 146097);
    unsigned int year_of_era =
        (day_of_era - day_of_era / 1460U + day_of_era / 36524U -
         day_of_era / 146096U) /
        365U;
    int64_t year = (int64_t)year_of_era + era * 400;
    unsigned int day_of_year =
        day_of_era -
        (365U * year_of_era + year_of_era / 4U - year_of_era / 100U);
    unsigned int month_piece = (5U * day_of_year + 2U) / 153U;
    unsigned int day =
        day_of_year - (153U * month_piece + 2U) / 5U + 1U;
    unsigned int month = month_piece < 10U ? month_piece + 3U
                                           : month_piece - 9U;
    year += month <= 2U;
    if (year < 1970 || year > 9999 || month < 1U || month > 12U || day < 1U ||
        day > 31U) {
        snprintf(output, 20, "UNKNOWN");
        return;
    }
    unsigned int display_year = (unsigned int)year;
    output[0] = (char)('0' + display_year / 1000U);
    output[1] = (char)('0' + display_year / 100U % 10U);
    output[2] = (char)('0' + display_year / 10U % 10U);
    output[3] = (char)('0' + display_year % 10U);
    output[4] = '-';
    output[5] = (char)('0' + month / 10U);
    output[6] = (char)('0' + month % 10U);
    output[7] = '-';
    output[8] = (char)('0' + day / 10U);
    output[9] = (char)('0' + day % 10U);
    output[10] = '\0';
}

static void draw_app_detail(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *states[] = {"READY", "STARTING", "RUNNING", "FAILED"};
    static const char *permission_names[] = {
        "MICROPHONE", "AUDIO", "CAMERA", "DOCUMENTS",
        "GPIO",       "NETWORK", "NOTIFICATIONS", "LORA",
        "PHOTOS READ", "PHOTOS WRITE",
    };
    const struct cp0_ui_app *app = &ui->apps[ui->app_selected];
    char value[32];

    fill_rect(canvas, 8, 28, 304, 135, COLOR_SURFACE);
    fill_rect(canvas, 8, 28, 4, 135, COLOR_GREEN);
    if (ui->app_detail_page == 0) {
        draw_prompt_line(canvas, 36, app->name, 0, 46);
        draw_labeled_value(canvas, 57, "VERSION", app->version, COLOR_TEXT);
        draw_labeled_value(canvas, 76, "STATE", states[app->state],
                           app->state == CP0_UI_APP_FAILED ? COLOR_RED
                                                          : COLOR_GREEN);
        draw_labeled_value(canvas, 95, "DISPLAY",
                           app->immersive ? "IMMERSIVE" : "STANDARD",
                           COLOR_TEXT);
        format_install_date(value, app->installed_at_unix_seconds);
        draw_labeled_value(canvas, 114, "INSTALLED", value, COLOR_TEXT);
        draw_text(canvas, 20, 136, "ID", 1, COLOR_MUTED);
        draw_prompt_line(canvas, 148, app->app_id, 0, 42);
    } else if (ui->app_detail_page == 1) {
        char package[16];
        char data[16];
        char total[16];
        uint64_t total_bytes = UINT64_MAX - app->package_bytes < app->data_bytes
                                   ? UINT64_MAX
                                   : app->package_bytes + app->data_bytes;
        format_bytes(package, app->package_bytes);
        format_bytes(data, app->data_bytes);
        format_bytes(total, total_bytes);
        draw_labeled_value(canvas, 43, "APP PACKAGE", package, COLOR_TEXT);
        draw_labeled_value(canvas, 67, "PRIVATE DATA", data, COLOR_TEXT);
        draw_labeled_value(canvas, 91, "TOTAL", total, COLOR_GREEN);
        draw_labeled_value(canvas, 119, "ON UNINSTALL", "DATA RETAINED",
                           COLOR_YELLOW);
        draw_labeled_value(canvas, 141, "DATA LIMIT", "APP MANIFEST",
                           COLOR_MUTED);
    } else if (ui->app_detail_page == 2) {
        unsigned int row = 0;
        unsigned int seen = 0;
        unsigned int count = 0;
        for (unsigned int bit = 0; bit < 10; bit++)
            count += (app->permissions & (1U << bit)) != 0;
        for (unsigned int bit = 0; bit < 10 && row < 4; bit++) {
            if ((app->permissions & (1U << bit)) == 0)
                continue;
            if (seen++ < ui->app_permission_offset)
                continue;
            int y = 37 + (int)row * 27;
            fill_rect(canvas, 20, y, 280, 23, COLOR_BAR);
            draw_text(canvas, 31, y + 8, permission_names[bit], 1, COLOR_TEXT);
            draw_text(canvas, 245, y + 8, "DECLARED", 1, COLOR_GREEN);
            row++;
        }
        if (row == 0)
            draw_text(canvas, 28, 58, "NO PERMISSIONS DECLARED", 1,
                      COLOR_GREEN);
        if (count > 4)
            snprintf(value, sizeof(value), "%u-%u / %u",
                     ui->app_permission_offset + 1U,
                     ui->app_permission_offset + row, count);
        else
            snprintf(value, sizeof(value), "%u TOTAL", count);
        draw_text(canvas, 20, 151, value, 1, COLOR_MUTED);
    } else {
        static const char *actions[] = {"OPEN / STOP", "UNINSTALL"};
        for (unsigned int row = 0; row < 2; row++) {
            int y = 45 + (int)row * 43;
            bool selected = ui->app_action_selected == row;
            fill_rect(canvas, 24, y, 272, 34,
                      selected ? COLOR_SELECTED : COLOR_BAR);
            stroke_rect(canvas, 24, y, 272, 34, selected ? 2 : 1,
                        selected ? (row == 1 ? COLOR_RED : COLOR_GREEN)
                                 : COLOR_MUTED);
            draw_text(canvas, 40, y + 12, actions[row], 1,
                      selected ? COLOR_TEXT : COLOR_MUTED);
        }
        draw_text(canvas, 24, 139,
                  app->state == CP0_UI_APP_RUNNING
                      ? "STOP APP BEFORE UNINSTALL"
                      : "PRIVATE DATA WILL BE RETAINED",
                  1, app->state == CP0_UI_APP_RUNNING ? COLOR_YELLOW
                                                      : COLOR_MUTED);
    }
    snprintf(value, sizeof(value), "%u/4", ui->app_detail_page + 1U);
    draw_text(canvas, 286, 154, value, 1, COLOR_MUTED);
}

static const char *store_state_label(const struct cp0_ui_store_app *app,
                                     char label[16])
{
    static const char *states[] = {
        "GET", "UPDATE", "QUEUED", "DOWNLOAD", "PAUSED",
        "INSTALL", "INSTALLED", "CANCELED", "FAILED",
    };
    static const char *failures[] = {
        "", "NETWORK", "STORAGE", "VERIFY", "INSTALL", "CATALOG", "INTERNAL",
    };
    if (app->state == CP0_UI_STORE_DOWNLOADING) {
        snprintf(label, 16, "DOWN %u%%", app->progress_percent);
        return label;
    }
    if (app->state == CP0_UI_STORE_FAILED &&
        app->failure_reason > CP0_UI_STORE_FAILURE_NONE &&
        app->failure_reason <= CP0_UI_STORE_FAILURE_INTERNAL) {
        snprintf(label, 16, "FAIL %s", failures[app->failure_reason]);
        return label;
    }
    return states[app->state];
}

static bool store_operation_has_cancel(enum cp0_ui_store_state state)
{
    return state == CP0_UI_STORE_QUEUED ||
           state == CP0_UI_STORE_DOWNLOADING ||
           state == CP0_UI_STORE_PAUSED || state == CP0_UI_STORE_FAILED;
}

static const char *store_primary_action(enum cp0_ui_store_state state)
{
    switch (state) {
    case CP0_UI_STORE_AVAILABLE:
    case CP0_UI_STORE_UPDATE:
        return "INSTALL";
    case CP0_UI_STORE_QUEUED:
    case CP0_UI_STORE_DOWNLOADING:
        return "PAUSE";
    case CP0_UI_STORE_PAUSED:
        return "RESUME";
    case CP0_UI_STORE_CANCELED:
    case CP0_UI_STORE_FAILED:
        return "RETRY";
    default:
        return NULL;
    }
}

static bool store_update_state(const struct cp0_ui_store_app *app)
{
    return app->update_available;
}

static bool store_update_batch_eligible(const struct cp0_ui_store_app *app)
{
    return app->update_available &&
           (app->state == CP0_UI_STORE_UPDATE ||
            app->state == CP0_UI_STORE_FAILED ||
            app->state == CP0_UI_STORE_CANCELED);
}

static bool store_operation_active(enum cp0_ui_store_state state)
{
    return state == CP0_UI_STORE_QUEUED ||
           state == CP0_UI_STORE_DOWNLOADING ||
           state == CP0_UI_STORE_INSTALLING;
}

static unsigned int store_activity_priority(enum cp0_ui_store_state state)
{
    if (state == CP0_UI_STORE_DOWNLOADING)
        return 3;
    if (state == CP0_UI_STORE_INSTALLING)
        return 2;
    return state == CP0_UI_STORE_QUEUED ? 1 : 0;
}

static void sync_store_activity(struct cp0_ui *ui)
{
    unsigned int priority = 0;
    ui->store_activity = false;
    ui->store_activity_count = 0;
    ui->store_activity_progress_percent = 0;
    ui->store_activity_state = CP0_UI_STORE_AVAILABLE;
    for (unsigned int index = 0; index < ui->store_count; index++) {
        const struct cp0_ui_store_app *app = &ui->store_apps[index];
        if (!store_operation_active(app->operation_state))
            continue;
        ui->store_activity = true;
        if (ui->store_activity_count < UINT8_MAX)
            ui->store_activity_count++;
        unsigned int app_priority =
            store_activity_priority(app->operation_state);
        if (app_priority > priority) {
            priority = app_priority;
            ui->store_activity_state = app->operation_state;
            ui->store_activity_progress_percent = app->progress_percent;
        }
    }
}

static unsigned int store_update_batch_count(const struct cp0_ui *ui)
{
    unsigned int count = 0;
    for (unsigned int index = 0;
         index < ui->store_count && count < CP0_UI_STORE_UPDATE_BATCH_MAX;
         index++)
        count += store_update_batch_eligible(&ui->store_apps[index]);
    return count;
}

static unsigned int store_update_count(const struct cp0_ui *ui)
{
    unsigned int count = 0;
    for (unsigned int index = 0; index < ui->store_count; index++)
        count += store_update_state(&ui->store_apps[index]);
    return count;
}

static const struct cp0_ui_store_app *store_update_at(
    const struct cp0_ui *ui, unsigned int selected)
{
    for (unsigned int index = 0; index < ui->store_count; index++) {
        if (!store_update_state(&ui->store_apps[index]))
            continue;
        if (selected == 0)
            return &ui->store_apps[index];
        selected--;
    }
    return NULL;
}

static unsigned int store_section_count(const struct cp0_ui *ui)
{
    switch (ui->store_section) {
    case CP0_UI_STORE_TODAY:
        if (ui->store_today_available) {
            if (ui->store_today_collection_open)
                return (unsigned int)ui
                    ->store_today_collections[ui->store_today_open_collection]
                    .app_count;
            return 1U + (unsigned int)ui->store_today_collection_count;
        }
        return ui->store_count == 0 ? 0 : 1;
    case CP0_UI_STORE_APPS:
        return ui->store_browse_count;
    case CP0_UI_STORE_SEARCH:
        return ui->store_search_count;
    case CP0_UI_STORE_UPDATES:
        return store_update_count(ui);
    default:
        return 0;
    }
}

static const struct cp0_ui_store_app *store_section_app(
    const struct cp0_ui *ui, unsigned int index)
{
    if (ui->store_section == CP0_UI_STORE_TODAY &&
        ui->store_today_available) {
        if (ui->store_today_collection_open) {
            const struct cp0_ui_store_editorial_collection_state *collection =
                &ui->store_today_collections[ui->store_today_open_collection];
            return index < collection->app_count ? &collection->apps[index]
                                                 : NULL;
        }
        return index == 0 ? &ui->store_today_featured : NULL;
    }
    if (ui->store_section == CP0_UI_STORE_APPS)
        return index < ui->store_browse_count ? &ui->store_page_apps[index]
                                              : NULL;
    if (ui->store_section == CP0_UI_STORE_SEARCH)
        return index < ui->store_search_count ? &ui->store_page_apps[index]
                                              : NULL;
    if (ui->store_section == CP0_UI_STORE_UPDATES)
        return store_update_at(ui, index);
    return index < store_section_count(ui) ? &ui->store_apps[index] : NULL;
}

static unsigned int store_section_selected(const struct cp0_ui *ui)
{
    if (ui->store_section == CP0_UI_STORE_TODAY &&
        ui->store_today_available)
        return ui->store_today_collection_open
                   ? ui->store_today_collection_selected
                   : ui->store_today_selected;
    if (ui->store_section == CP0_UI_STORE_APPS)
        return ui->store_browse_selected;
    return ui->store_section == CP0_UI_STORE_SEARCH
               ? ui->store_search_selected
               : ui->store_selected;
}

static const struct cp0_ui_store_app *selected_store_app(
    const struct cp0_ui *ui)
{
    if (ui->store_section == CP0_UI_STORE_UPDATES &&
        ui->store_update_all_selected)
        return NULL;
    return store_section_app(ui, store_section_selected(ui));
}

static void draw_store_tabs(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"TODAY", "APPS", "SEARCH", "UPDATES"};
    for (unsigned int tab = 0; tab < 4; tab++) {
        int x = (int)tab * 80;
        bool selected = tab == ui->store_section;
        fill_rect(canvas, x, 22, 80, 18,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        if (selected)
            fill_rect(canvas, x, 38, 80, 2, COLOR_GREEN);
        draw_text(canvas, x + 10, 28, labels[tab], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_store_rows(struct canvas *canvas, const struct cp0_ui *ui,
                            int top, unsigned int maximum_rows)
{
    unsigned int count = store_section_count(ui);
    unsigned int selected = store_section_selected(ui);
    unsigned int first = selected >= maximum_rows
                             ? selected - maximum_rows + 1U
                             : 0;
    unsigned int visible = count > first ? count - first : 0;
    if (visible > maximum_rows)
        visible = maximum_rows;
    for (unsigned int row = 0; row < visible; row++) {
        unsigned int index = first + row;
        const struct cp0_ui_store_app *app = store_section_app(ui, index);
        int y = top + (int)row * 27;
        bool row_selected = index == selected &&
                            !(ui->store_section == CP0_UI_STORE_UPDATES &&
                              ui->store_update_all_selected);
        char state[16];

        if (app == NULL)
            continue;
        fill_rect(canvas, 8, y, 304, 24,
                  row_selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 24, row_selected ? 2 : 1,
                    row_selected ? COLOR_GREEN : COLOR_BAR);
        fill_rect(canvas, 17, y + 7, 9, 9,
                  app->state == CP0_UI_STORE_INSTALLED
                      ? COLOR_GREEN
                      : (app->state == CP0_UI_STORE_FAILED ? COLOR_RED
                                                           : COLOR_YELLOW));
        draw_prompt_line(canvas, y + 5, app->name, 0, 27);
        draw_text(canvas, 218, y + 14, store_state_label(app, state), 1,
                  app->state == CP0_UI_STORE_FAILED ? COLOR_RED : COLOR_MUTED);
    }
}

static void draw_store_message(struct canvas *canvas, const char *message,
                               uint32_t color)
{
    draw_text(canvas, 18, 84, message, 1, color);
}

static void draw_store_update_command(struct canvas *canvas,
                                      const struct cp0_ui *ui)
{
    unsigned int count = store_update_batch_count(ui);
    bool selected = ui->store_update_all_selected && count > 0;
    char label[24];

    fill_rect(canvas, 8, 44, 304, 24,
              selected ? COLOR_SELECTED : COLOR_SURFACE);
    stroke_rect(canvas, 8, 44, 304, 24, selected ? 2 : 1,
                selected ? COLOR_GREEN : COLOR_BAR);
    snprintf(label, sizeof(label), "UPDATE ALL %u", count);
    draw_text(canvas, 20, 52, label, 1,
              count == 0 || ui->store_catalog_stale ? COLOR_MUTED
                                                     : COLOR_GREEN);
    draw_text(canvas, 267, 52, "ENTER", 1,
              selected && !ui->store_catalog_stale ? COLOR_TEXT
                                                   : COLOR_MUTED);
}

static void draw_store_list(struct canvas *canvas, const struct cp0_ui *ui)
{
    enum cp0_ui_store_status status =
        ui->store_section == CP0_UI_STORE_APPS ? ui->store_browse_status
                                              : ui->store_status;
    if (status != CP0_UI_STORE_READY) {
        static const char *messages[] = {"LOADING CATALOG", "",
                                         "NOT CONFIGURED",
                                         "STORE UNAVAILABLE"};
        draw_store_message(canvas, messages[status],
                           status == CP0_UI_STORE_UNAVAILABLE
                               ? COLOR_RED
                               : COLOR_YELLOW);
        return;
    }
    if (store_section_count(ui) == 0) {
        draw_store_message(canvas,
                           ui->store_section == CP0_UI_STORE_UPDATES
                               ? "ALL APPS UP TO DATE"
                               : "NO APPS AVAILABLE",
                           COLOR_GREEN);
        return;
    }

    if (ui->store_section == CP0_UI_STORE_TODAY &&
        ui->store_today_available && ui->store_today_collection_open) {
        const struct cp0_ui_store_editorial_collection_state *collection =
            &ui->store_today_collections[ui->store_today_open_collection];
        draw_prompt_line(canvas, 47, collection->title, 0, 42);
        draw_store_rows(canvas, ui, 60, 4);
        draw_text(canvas, 257, 48, "ESC", 1, COLOR_MUTED);
    } else if (ui->store_section == CP0_UI_STORE_TODAY &&
               ui->store_today_available) {
        const struct cp0_ui_store_app *app = &ui->store_today_featured;
        char state[16];
        draw_prompt_line(canvas, 46, ui->store_today_headline, 0, 46);
        fill_rect(canvas, 8, 59, 304, 39,
                  ui->store_today_selected == 0 ? COLOR_SELECTED
                                                : COLOR_SURFACE);
        stroke_rect(canvas, 8, 59, 304, 39,
                    ui->store_today_selected == 0 ? 2 : 1,
                    ui->store_today_selected == 0 ? COLOR_GREEN : COLOR_BAR);
        draw_prompt_line(canvas, 68, app->name, 0, 31);
        draw_text(canvas, 220, 81, store_state_label(app, state), 1,
                  COLOR_YELLOW);
        for (unsigned int index = 0;
             index < ui->store_today_collection_count; index++) {
            const struct cp0_ui_store_editorial_collection_state *collection =
                &ui->store_today_collections[index];
            int y = 103 + (int)index * 27;
            bool selected = ui->store_today_selected == index + 1U;
            char count[8];
            fill_rect(canvas, 8, y, 304, 24,
                      selected ? COLOR_SELECTED : COLOR_SURFACE);
            stroke_rect(canvas, 8, y, 304, 24, selected ? 2 : 1,
                        selected ? COLOR_GREEN : COLOR_BAR);
            draw_prompt_line(canvas, y + 7, collection->title, 0, 34);
            snprintf(count, sizeof(count), "%zu >", collection->app_count);
            draw_text(canvas, 270, y + 7, count, 1,
                      selected ? COLOR_TEXT : COLOR_MUTED);
        }
    } else if (ui->store_section == CP0_UI_STORE_TODAY) {
        const struct cp0_ui_store_app *app = &ui->store_apps[0];
        draw_text(canvas, 14, 50, "FEATURED", 1, COLOR_GREEN);
        fill_rect(canvas, 8, 63, 304, 74, COLOR_SURFACE);
        fill_rect(canvas, 8, 63, 4, 74, COLOR_GREEN);
        draw_prompt_line(canvas, 73, app->name, 0, 42);
        draw_prompt_line(canvas, 91, app->summary, 0, 46);
        draw_prompt_line(canvas, 104, app->summary, 46, 46);
        char state[16];
        draw_text(canvas, 20, 121, store_state_label(app, state), 1,
                  COLOR_YELLOW);
    } else if (ui->store_section == CP0_UI_STORE_UPDATES) {
        draw_store_update_command(canvas, ui);
        draw_store_rows(canvas, ui, 72, 3);
    } else {
        draw_store_rows(canvas, ui, 44, 4);
    }
    if (ui->store_section == CP0_UI_STORE_APPS && ui->store_browse_count > 0) {
        char page[24];
        snprintf(page, sizeof(page), "%u-%u / %u",
                 (unsigned int)ui->store_browse_offset + 1U,
                 (unsigned int)ui->store_browse_offset +
                     ui->store_browse_count,
                 (unsigned int)ui->store_browse_total);
        draw_text(canvas, 236, 159, page, 1, COLOR_MUTED);
    }
    if ((ui->store_section == CP0_UI_STORE_APPS && ui->store_browse_stale) ||
        (ui->store_section != CP0_UI_STORE_APPS && ui->store_catalog_stale))
        draw_text(canvas, 8, 159, "STALE", 1, COLOR_YELLOW);
    if (ui->store_list_truncated &&
        ui->store_section != CP0_UI_STORE_APPS)
        draw_text(canvas, 284, 159, "32+", 1, COLOR_YELLOW);
}

static void draw_store_search(struct canvas *canvas, const struct cp0_ui *ui)
{
    char query[48];
    const char *shown = ui->store_search_query[0] == '\0'
                            ? "SEARCH"
                            : ui->store_search_query;
    snprintf(query, sizeof(query), "> %.40s%s", shown,
             ui->store_search_input ? "_" : "");
    fill_rect(canvas, 8, 44, 304, 23,
              ui->store_search_input ? COLOR_SELECTED : COLOR_SURFACE);
    stroke_rect(canvas, 8, 44, 304, 23,
                ui->store_search_input ? 2 : 1,
                ui->store_search_input ? COLOR_GREEN : COLOR_BAR);
    draw_text(canvas, 16, 52, query, 1,
              ui->store_search_input ? COLOR_TEXT : COLOR_MUTED);

    if (ui->store_search_query[0] == '\0') {
        if (ui->store_recent_count == 0) {
            draw_store_message(canvas, "NO RECENT SEARCHES", COLOR_MUTED);
            return;
        }
        for (unsigned int row = 0; row < ui->store_recent_count; row++) {
            int y = 72 + (int)row * 21;
            bool selected = !ui->store_search_input &&
                            row == ui->store_recent_selected;
            fill_rect(canvas, 12, y, 296, 18,
                      selected ? COLOR_SELECTED : COLOR_SURFACE);
            draw_text(canvas, 20, y + 6, ui->store_recent_queries[row], 1,
                      selected ? COLOR_TEXT : COLOR_MUTED);
        }
        return;
    }
    if (ui->store_search_status != CP0_UI_STORE_READY) {
        static const char *messages[] = {"SEARCHING", "", "NOT CONFIGURED",
                                         "SEARCH UNAVAILABLE"};
        draw_store_message(canvas, messages[ui->store_search_status],
                           ui->store_search_status == CP0_UI_STORE_UNAVAILABLE
                               ? COLOR_RED
                               : COLOR_YELLOW);
        return;
    }
    if (ui->store_search_count == 0) {
        draw_store_message(canvas, "NO RESULTS", COLOR_MUTED);
        return;
    }
    draw_store_rows(canvas, ui, 72, 3);
    char page[24];
    snprintf(page, sizeof(page), "%u-%u / %u",
             (unsigned int)ui->store_search_offset + 1U,
             (unsigned int)ui->store_search_offset + ui->store_search_count,
             (unsigned int)ui->store_search_total);
    draw_text(canvas, 236, 159, page, 1, COLOR_MUTED);
    if (ui->store_search_stale)
        draw_text(canvas, 8, 159, "STALE", 1, COLOR_YELLOW);
}

static void draw_store_detail_footer(struct canvas *canvas,
                                     const struct cp0_ui *ui)
{
    char page[12];
    snprintf(page, sizeof(page), "%u/5", ui->store_detail_page + 1U);
    fill_rect(canvas, 8, 150, 304, 13, COLOR_BAR);
    draw_text(canvas, 278, 154, page, 1, COLOR_MUTED);
}

static void draw_store_overview(struct canvas *canvas, const struct cp0_ui *ui,
                                const struct cp0_ui_store_app *app)
{
    char version[76];
    char state[16];
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    fill_rect(canvas, 8, 27, 4, 136, COLOR_GREEN);
    if (ui->store_icon_available)
        draw_scaled_image(canvas, 20, 36, 48, 48, ui->store_icon_pixels,
                          ui->store_icon_width, ui->store_icon_height);
    else
        draw_store_icon(canvas, 31, 48, COLOR_MUTED);
    draw_text_slice(canvas, 78, 35, app->name, 0, 36, COLOR_TEXT);
    snprintf(version, sizeof(version), "VERSION %s", app->version);
    draw_text_slice(canvas, 78, 49, version, 0, 36, COLOR_MUTED);
    if (ui->store_detail_status == CP0_UI_STORE_DETAIL_READY) {
        draw_text_slice(canvas, 78, 63, ui->store_developer, 0, 36,
                        COLOR_TEXT);
        snprintf(version, sizeof(version), "%s  AGE %s", ui->store_category,
                 ui->store_age_rating);
        draw_text_slice(canvas, 78, 77, version, 0, 36, COLOR_MUTED);
    } else {
        draw_text(canvas, 78, 63,
                  ui->store_detail_status == CP0_UI_STORE_DETAIL_LOADING
                      ? "LOADING DETAILS"
                      : "DETAILS UNAVAILABLE",
                  1, ui->store_detail_status == CP0_UI_STORE_DETAIL_LOADING
                         ? COLOR_YELLOW
                         : COLOR_RED);
    }
    draw_text_slice(canvas, 20, 96, app->summary, 0, 46, COLOR_TEXT);
    draw_text_slice(canvas, 20, 109, app->summary, 46, 46, COLOR_TEXT);
    draw_text(canvas, 20, 125, store_state_label(app, state), 1,
              app->state == CP0_UI_STORE_FAILED ? COLOR_RED : COLOR_GREEN);
    const char *primary = store_primary_action(app->state);
    if (primary != NULL) {
        bool cancel = store_operation_has_cancel(app->state);
        draw_text(canvas, 20, 138, primary, 1,
                  ui->store_operation_action_selected == 0 ? COLOR_GREEN
                                                           : COLOR_MUTED);
        if (cancel)
            draw_text(canvas, 112, 138, "CANCEL", 1,
                      ui->store_operation_action_selected == 1 ? COLOR_RED
                                                               : COLOR_MUTED);
    }
    draw_store_detail_footer(canvas, ui);
}

static void draw_store_prose(struct canvas *canvas, const struct cp0_ui *ui,
                             const char *title, const char *prose)
{
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    fill_rect(canvas, 8, 27, 4, 136, COLOR_GREEN);
    draw_text(canvas, 20, 34, title, 1, COLOR_GREEN);
    for (unsigned int line = 0; line < 7; line++)
        draw_wrapped_line(canvas, 20, 50 + (int)line * 13, prose,
                          ui->store_detail_text_offset + line, 46, COLOR_TEXT);
    draw_store_detail_footer(canvas, ui);
}

static void draw_store_screenshot(struct canvas *canvas,
                                  const struct cp0_ui *ui)
{
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    if (ui->store_screenshot_available) {
        draw_scaled_image(canvas, 32, 27, 256, 136,
                          ui->store_screenshot_pixels, 320, 170);
    } else {
        draw_text(canvas, ui->store_screenshot_loading ? 110 : 92, 86,
                  ui->store_screenshot_loading ? "LOADING SCREENSHOT"
                                               : "SCREENSHOT UNAVAILABLE",
                  1, ui->store_screenshot_loading ? COLOR_YELLOW : COLOR_RED);
    }
    char position[16];
    snprintf(position, sizeof(position), "%u/%u",
             ui->store_screenshot_index + 1U, ui->store_screenshot_count);
    draw_store_detail_footer(canvas, ui);
    draw_text(canvas, 20, 154, position, 1, COLOR_TEXT);
}

static void draw_store_permissions(struct canvas *canvas,
                                   const struct cp0_ui *ui,
                                   const struct cp0_ui_store_app *app)
{
    static const char *permission_names[] = {
        "AUDIO CAPTURE", "AUDIO PLAYBACK", "CAMERA", "DOCUMENTS",
        "GPIO",          "NETWORK",        "NOTIFICATIONS", "LORA",
        "PHOTOS READ",   "PHOTOS WRITE",
    };
    unsigned int visible = 0;
    unsigned int new_permissions = 0;
    bool update = app->update_available;
    fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
    fill_rect(canvas, 8, 27, 4, 136, COLOR_GREEN);
    for (unsigned int bit = 0; bit < 10; bit++)
        new_permissions += update && (app->permissions & (1U << bit)) != 0 &&
                           (app->installed_permissions & (1U << bit)) == 0;
    char heading[24];
    if (update)
        snprintf(heading, sizeof(heading), "%u NEW PERMISSIONS",
                 new_permissions);
    else
        snprintf(heading, sizeof(heading), "REQUESTED PERMISSIONS");
    draw_text(canvas, 20, 34, heading, 1,
              new_permissions > 0 ? COLOR_YELLOW : COLOR_GREEN);
    for (unsigned int bit = 0; bit < 10; bit++) {
        if ((app->permissions & (1U << bit)) == 0)
            continue;
        int x = 20 + (int)(visible % 2U) * 146;
        int y = 55 + (int)(visible / 2U) * 21;
        bool added = update &&
                     (app->installed_permissions & (1U << bit)) == 0;
        draw_text(canvas, x, y, permission_names[bit], 1,
                  added ? COLOR_YELLOW : COLOR_TEXT);
        if (added)
            draw_text(canvas, x, y + 10, "NEW", 1, COLOR_YELLOW);
        visible++;
    }
    if (visible == 0)
        draw_text(canvas, 20, 60, "NO PERMISSIONS", 1, COLOR_MUTED);
    draw_store_detail_footer(canvas, ui);
}

static void draw_store_detail(struct canvas *canvas, const struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app = selected_store_app(ui);
    if (ui->store_detail_page == 0) {
        draw_store_overview(canvas, ui, app);
    } else if (ui->store_detail_status != CP0_UI_STORE_DETAIL_READY) {
        fill_rect(canvas, 8, 27, 304, 136, COLOR_SURFACE);
        draw_text(canvas, 88, 86, "DETAILS UNAVAILABLE", 1, COLOR_RED);
        draw_store_detail_footer(canvas, ui);
    } else if (ui->store_detail_page == 1) {
        draw_store_prose(canvas, ui, "DESCRIPTION", ui->store_description);
    } else if (ui->store_detail_page == 2) {
        draw_store_screenshot(canvas, ui);
    } else if (ui->store_detail_page == 3) {
        draw_store_permissions(canvas, ui, app);
    } else {
        draw_store_prose(canvas, ui, "WHAT'S NEW", ui->store_release_notes);
    }
}

static void draw_store_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    if (ui->store_detail && selected_store_app(ui) != NULL) {
        draw_store_detail(canvas, ui);
        return;
    }
    draw_store_tabs(canvas, ui);
    if (ui->store_section == CP0_UI_STORE_SEARCH)
        draw_store_search(canvas, ui);
    else
        draw_store_list(canvas, ui);
}

static uint32_t rgb565_color(uint16_t pixel)
{
    uint32_t red = ((pixel >> 11) & 0x1fU) * 255U / 31U;
    uint32_t green = ((pixel >> 5) & 0x3fU) * 255U / 63U;
    uint32_t blue = (pixel & 0x1fU) * 255U / 31U;
    return (red << 16) | (green << 8) | blue;
}

static void draw_task_thumbnail(struct canvas *canvas,
                                const struct cp0_ui_task *task, int x, int y,
                                int width, int height)
{
    if (!task->thumbnail_available) {
        uint32_t accent = task->state == CP0_UI_TASK_CRASHED ? COLOR_RED
                           : task->state == CP0_UI_TASK_CHECKPOINTED
                               ? COLOR_YELLOW
                               : COLOR_GREEN;
        fill_rect(canvas, x, y, width, height, COLOR_BAR);
        for (int line = -height; line < width; line += 20) {
            for (int offset = 0; offset < 2; offset++) {
                for (int py = 0; py < height; py++) {
                    int px = line + py + offset;
                    int destination_x = x + px;
                    if (px >= 0 && px < width && destination_x >= 0 &&
                        destination_x < canvas->width && y + py >= 0 &&
                        y + py < canvas->height)
                        canvas->pixels[(y + py) * canvas->stride +
                                       destination_x] = accent;
                }
            }
        }
        return;
    }
    for (int dy = 0; dy < height; dy++) {
        unsigned int source_y =
            (unsigned int)((uint64_t)dy * CP0_UI_TASK_THUMBNAIL_HEIGHT /
                           (unsigned int)height);
        int py = y + dy;
        if (py < 0 || py >= canvas->height)
            continue;
        for (int dx = 0; dx < width; dx++) {
            unsigned int source_x =
                (unsigned int)((uint64_t)dx * CP0_UI_TASK_THUMBNAIL_WIDTH /
                               (unsigned int)width);
            int px = x + dx;
            if (px < 0 || px >= canvas->width)
                continue;
            canvas->pixels[py * canvas->stride + px] = rgb565_color(
                task->thumbnail_pixels[source_y * CP0_UI_TASK_THUMBNAIL_WIDTH +
                                       source_x]);
        }
    }
}

static const char *task_state_text(enum cp0_ui_task_state state)
{
    switch (state) {
    case CP0_UI_TASK_FOREGROUND:
        return "LIVE";
    case CP0_UI_TASK_BACKGROUND:
        return "BACKGROUND";
    case CP0_UI_TASK_FROZEN:
        return "PAUSED";
    case CP0_UI_TASK_CHECKPOINTED:
        return "SAVED";
    case CP0_UI_TASK_CRASHED:
        return "CRASHED";
    }
    return "UNKNOWN";
}

static void draw_task_card(struct canvas *canvas,
                           const struct cp0_ui_task *task, int x, int y,
                           int width, int thumbnail_height, bool selected)
{
    int card_height = thumbnail_height + 31;
    fill_rect(canvas, x, y, width, card_height, COLOR_SURFACE);
    draw_task_thumbnail(canvas, task, x + 4, y + 2, width - 8,
                        thumbnail_height);
    stroke_rect(canvas, x, y, width, card_height, selected ? 2 : 1,
                selected ? COLOR_GREEN : COLOR_MUTED);
    draw_text_slice(canvas, x + 7, y + thumbnail_height + 9, task->name, 0,
                    selected ? 20 : 15, selected ? COLOR_TEXT : COLOR_MUTED);
    if (selected)
        draw_text(canvas, x + 7, y + thumbnail_height + 21,
                  task_state_text(task->state), 1,
                  task->state == CP0_UI_TASK_CRASHED ? COLOR_RED
                  : task->state == CP0_UI_TASK_CHECKPOINTED
                      ? COLOR_YELLOW
                      : COLOR_GREEN);
}

static void draw_tasks_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    char count[24];
    if (ui->task_count == 0) {
        draw_empty_page(canvas, "TASKS", "NO ACTIVE TASKS", COLOR_YELLOW);
        return;
    }
    snprintf(count, sizeof(count), "%u/%u", ui->task_selected + 1,
             ui->task_count);
    draw_text(canvas, 284, 27, count, 1, COLOR_MUTED);

    if (ui->task_selected > 0)
        draw_task_card(canvas, &ui->tasks[ui->task_selected - 1], -86, 47,
                       148, 78, false);
    if (ui->task_selected + 1 < ui->task_count)
        draw_task_card(canvas, &ui->tasks[ui->task_selected + 1], 258, 47,
                       148, 78, false);
    draw_task_card(canvas, &ui->tasks[ui->task_selected], 76, 38, 168, 85,
                   true);
}

static const char *settings_state(bool enabled, bool allowed)
{
    if (!allowed)
        return "LOCKED";
    return enabled ? "ON" : "OFF";
}

static void format_bytes(char output[16], uint64_t bytes)
{
    const uint64_t mib = 1024U * 1024U;
    const uint64_t gib = 1024U * mib;
    uint64_t units = bytes >= gib ? bytes / gib : bytes / mib;
    unsigned int bounded_units =
        units > UINT_MAX ? UINT_MAX : (unsigned int)units;
    if (bytes >= gib)
        snprintf(output, 16, "%u.%u GB", bounded_units,
                 (unsigned int)((bytes % gib) * 10U / gib));
    else
        snprintf(output, 16, "%u MB", bounded_units);
}

static void draw_page_mark(struct canvas *canvas, unsigned int page)
{
    draw_text(canvas, 286, 154, page == 0 ? "1/2" : "2/2", 1, COLOR_MUTED);
}

static void draw_page_mark_count(struct canvas *canvas, unsigned int page,
                                 unsigned int count)
{
    char value[8];
    snprintf(value, sizeof(value), "%u/%u", page + 1U, count);
    draw_text(canvas, 286, 154, value, 1, COLOR_MUTED);
}

static const char *capability_state(unsigned int state)
{
    static const char *states[] = {"UNKNOWN", "UNAVAILABLE", "READY"};
    return state < 3 ? states[state] : "UNKNOWN";
}

static void draw_device_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    char value[32];
    fill_rect(canvas, 8, 28, 304, 135, COLOR_SURFACE);
    fill_rect(canvas, 8, 28, 4, 135, COLOR_YELLOW);
    if (!ui->device_available) {
        draw_text(canvas, 28, 57, "DEVICE DATA UNAVAILABLE", 1, COLOR_RED);
        return;
    }
    if (ui->device_page == 0) {
        draw_labeled_value(canvas, 39, "MODEL", ui->device_model, COLOR_TEXT);
        draw_labeled_value(canvas, 59, "HARDWARE", "V0.6 / CM0", COLOR_TEXT);
        draw_labeled_value(canvas, 79, "OS", ui->os_version, COLOR_TEXT);
        snprintf(value, sizeof(value), "%llu H %llu M",
                 (unsigned long long)(ui->uptime_seconds / 3600U),
                 (unsigned long long)((ui->uptime_seconds / 60U) % 60U));
        draw_labeled_value(canvas, 99, "UPTIME", value, COLOR_TEXT);
        if (ui->temperature_millicelsius >= 0)
            snprintf(value, sizeof(value), "%d.%d C",
                     ui->temperature_millicelsius / 1000,
                     (ui->temperature_millicelsius % 1000) / 100);
        else
            snprintf(value, sizeof(value), "UNKNOWN");
        draw_labeled_value(
            canvas, 119, "CPU TEMP", value,
            ui->temperature_millicelsius < 0
                ? COLOR_MUTED
                : (ui->temperature_millicelsius >= 80000 ? COLOR_RED
                                                          : COLOR_GREEN));
    } else if (ui->device_page == 1) {
        char total[16];
        char available[16];
        format_bytes(total, ui->memory_total_bytes);
        format_bytes(available, ui->memory_available_bytes);
        snprintf(value, sizeof(value), "%s FREE", available);
        draw_labeled_value(canvas, 43, "MEMORY", value, COLOR_TEXT);
        snprintf(value, sizeof(value), "%s TOTAL", total);
        draw_labeled_value(canvas, 61, "", value, COLOR_MUTED);
        format_bytes(total, ui->storage_total_bytes);
        format_bytes(available, ui->storage_available_bytes);
        snprintf(value, sizeof(value), "%s FREE", available);
        draw_labeled_value(canvas, 87, "STORAGE", value, COLOR_TEXT);
        snprintf(value, sizeof(value), "%s TOTAL", total);
        draw_labeled_value(canvas, 105, "", value, COLOR_MUTED);
        draw_labeled_value(canvas, 133, "APP STORAGE", "ISOLATED", COLOR_GREEN);
    } else if (ui->device_page == 2) {
        static const char *statuses[] = {"UNKNOWN", "CHARGING", "DISCHARGING",
                                         "FULL", "NOT CHARGING"};
        if (ui->battery_percent >= 0)
            snprintf(value, sizeof(value), "%d%%", ui->battery_percent);
        else
            snprintf(value, sizeof(value), "UNKNOWN");
        draw_labeled_value(canvas, 41, "CAPACITY", value,
                           ui->battery_percent < 0 ? COLOR_MUTED : COLOR_GREEN);
        draw_labeled_value(canvas, 65, "STATUS",
                           ui->battery_status < 5
                               ? statuses[ui->battery_status]
                               : "UNKNOWN",
                           ui->battery_status == 1 ? COLOR_GREEN : COLOR_TEXT);
        if (ui->battery_voltage_available)
            snprintf(value, sizeof(value), "%lld.%03lld V",
                     (long long)(ui->battery_voltage_microvolts / 1000000),
                     (long long)((ui->battery_voltage_microvolts % 1000000) /
                                 1000));
        else
            snprintf(value, sizeof(value), "UNKNOWN");
        draw_labeled_value(canvas, 89, "VOLTAGE", value,
                           ui->battery_voltage_available ? COLOR_TEXT
                                                         : COLOR_MUTED);
        if (ui->battery_current_available)
            snprintf(value, sizeof(value), "%+lld MA",
                     (long long)(ui->battery_current_microamps / 1000));
        else
            snprintf(value, sizeof(value), "UNKNOWN");
        draw_labeled_value(canvas, 113, "CURRENT", value,
                           ui->battery_current_available ? COLOR_TEXT
                                                         : COLOR_MUTED);
        if (ui->battery_voltage_available && ui->battery_current_available)
            snprintf(value, sizeof(value), "%lld MW",
                     (long long)((ui->battery_voltage_microvolts / 1000) *
                                 (ui->battery_current_microamps / 1000) /
                                 1000));
        else
            snprintf(value, sizeof(value), "UNKNOWN");
        draw_labeled_value(canvas, 137, "POWER", value, COLOR_MUTED);
    } else {
        static const char *bus_states[] = {"UNKNOWN", "UNAVAILABLE",
                                           "RESTRICTED", "READY"};
        draw_labeled_value(canvas, 41, "DISPLAY",
                           capability_state(ui->display_state),
                           ui->display_state == 2 ? COLOR_GREEN : COLOR_MUTED);
        draw_labeled_value(canvas, 63, "KEYBOARD",
                           capability_state(ui->keyboard_state),
                           ui->keyboard_state == 2 ? COLOR_GREEN : COLOR_MUTED);
        draw_labeled_value(canvas, 85, "AUDIO",
                           capability_state(ui->audio_state),
                           ui->audio_state == 2 ? COLOR_GREEN : COLOR_MUTED);
        draw_labeled_value(canvas, 107, "CAMERA",
                           capability_state(ui->camera_state),
                           ui->camera_state == 2 ? COLOR_GREEN : COLOR_MUTED);
        draw_labeled_value(canvas, 129, "I2C BUS",
                           ui->i2c_bus_state < 4
                               ? bus_states[ui->i2c_bus_state]
                               : "UNKNOWN",
                           ui->i2c_bus_state == 3 ? COLOR_GREEN : COLOR_MUTED);
    }
    draw_page_mark_count(canvas, ui->device_page, 4);
}

static void draw_network_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    const char *status = ui->network_online
                             ? "ONLINE"
                             : (ui->network_link_up ? "LINK ONLY" : "OFFLINE");
    uint32_t status_color = ui->network_online
                                ? COLOR_GREEN
                                : (ui->network_link_up ? COLOR_YELLOW : COLOR_RED);
    fill_rect(canvas, 8, 28, 304, 135, COLOR_SURFACE);
    fill_rect(canvas, 8, 28, 4, 135, status_color);
    if (!ui->network_available) {
        draw_text(canvas, 28, 57, "NO NETWORK INTERFACE", 1, COLOR_RED);
        return;
    }
    if (ui->network_page == 0) {
        draw_text(canvas, 28, 48, status, 2, status_color);
        draw_labeled_value(canvas, 84, "INTERFACE", ui->network_interface,
                           COLOR_TEXT);
        draw_labeled_value(canvas, 106, "ADDRESS", ui->network_ipv4[0] != '\0'
                                                    ? ui->network_ipv4
                                                    : "NO ADDRESS",
                           ui->network_online ? COLOR_TEXT : COLOR_MUTED);
    } else {
        draw_labeled_value(canvas, 47, "LINK",
                           ui->network_link_up ? "UP" : "DOWN", status_color);
        draw_labeled_value(canvas, 69, "CONNECTIVITY",
                           ui->network_online ? "READY" : "NOT READY",
                           status_color);
        draw_labeled_value(canvas, 91, "IPV4", ui->network_ipv4[0] != '\0'
                                               ? ui->network_ipv4
                                               : "UNASSIGNED",
                           COLOR_TEXT);
        draw_labeled_value(canvas, 113, "MANAGEMENT", "READ ONLY", COLOR_MUTED);
    }
    draw_page_mark(canvas, ui->network_page);
}

static void draw_settings_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *titles[] = {
        "CONNECTIVITY", "DISPLAY", "SOUND",  "CAMERA",
        "POWER",        "APPS & PRIVACY", "SYSTEM", "SECURITY",
    };
    static const char *details[] = {
        "WI-FI, AIRPLANE, NETWORK", "BRIGHTNESS, THEME, TIMEOUT",
        "VOLUME, MUTE, KEY SOUNDS", "RESOLUTION, ROTATION, MIRROR",
        "BATTERY AND POWER ACTIONS", "APPS, STORAGE, PERMISSIONS",
        "ABOUT, TIME, UPDATE, ACCESS", "POLICY, MODES, DEVICE LOCK",
    };
    unsigned int first = ui->settings_selected > 3 ? ui->settings_selected - 3 : 0;
    for (unsigned int row = 0; row < 4 && first + row < 8; row++) {
        unsigned int index = first + row;
        int y = 27 + (int)row * 32;
        bool selected = ui->settings_selected == index;
        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_BAR);
        draw_text(canvas, 20, y + 6, titles[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text_slice(canvas, 20, y + 17, details[index], 0, 42,
                        COLOR_MUTED);
        draw_text(canvas, 294, y + 10, ">", 1,
                  selected ? COLOR_GREEN : COLOR_MUTED);
    }
    draw_text(canvas, 8, 158, "8 CATEGORIES", 1, COLOR_MUTED);
}

static unsigned int settings_item_count(unsigned int category)
{
    static const unsigned int counts[] = {6, 3, 4, 4, 5, 6, 6, 8};
    return category < 8 ? counts[category] : 0;
}

static void settings_item(const struct cp0_ui *ui, unsigned int category,
                          unsigned int item, const char **label, char value[24],
                          bool *available)
{
    static const char *themes[] = {"DARK", "LIGHT", "HIGH CONTRAST"};
    static const char *timeouts[] = {"30 SEC", "1 MIN", "5 MIN", "NEVER"};
    static const char *rotations[] = {"0 DEG", "90 DEG", "180 DEG", "270 DEG"};
    static const char *authorities[] = {"PERSONAL", "PARENT", "ORGANIZATION"};
    *available = true;
    value[0] = '\0';
    switch (category) {
    case 0: {
        static const char *labels[] = {"WI-FI", "AIRPLANE MODE", "NETWORK DETAILS",
                                       "BLUETOOTH", "HOTSPOT", "VPN"};
        *label = labels[item];
        if (item == 0) {
            if (!ui->local_simulation && !ui->connectivity_available)
                snprintf(value, 24, "UNAVAILABLE");
            else if (!ui->local_simulation && !ui->wifi_available)
                snprintf(value, 24, "NO ADAPTER");
            else
                snprintf(value, 24, "%s", ui->wifi_enabled ? "ON" : "OFF");
        } else if (item == 1) {
            if (!ui->local_simulation && !ui->connectivity_available)
                snprintf(value, 24, "UNAVAILABLE");
            else
                snprintf(value, 24, "%s", ui->airplane_mode ? "ON" : "OFF");
        }
        else if (item == 2)
            snprintf(value, 24, "%s", ui->network_online ? "CONNECTED" : "OFFLINE");
        else {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        }
        if (item == 0 && !ui->local_simulation &&
            (!ui->connectivity_available || !ui->wifi_available)) {
            *available = false;
        } else if (item == 1 && !ui->local_simulation &&
                   !ui->connectivity_available) {
            *available = false;
        }
        break;
    }
    case 1: {
        static const char *labels[] = {"BRIGHTNESS", "THEME", "SCREEN TIMEOUT"};
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%u%%", ui->brightness_percent);
        else if (item == 1)
            snprintf(value, 24, "%s", themes[ui->theme % 3]);
        else
            snprintf(value, 24, "%s", timeouts[ui->screen_timeout % 4]);
        if (item == 0 && !ui->local_simulation &&
            !ui->brightness_available) {
            snprintf(value, 24, "UNAVAILABLE");
            *available = false;
        }
        break;
    }
    case 2: {
        static const char *labels[] = {"MEDIA VOLUME", "MUTE", "KEY SOUNDS", "OUTPUT"};
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%u%%", ui->volume_percent);
        else if (item == 1)
            snprintf(value, 24, "%s", ui->muted ? "ON" : "OFF");
        else if (item == 2)
            snprintf(value, 24, "%s", ui->key_sounds ? "ON" : "OFF");
        else {
            snprintf(value, 24, "%s", capability_state(ui->audio_state));
            *available = ui->audio_state == 2;
        }
        if (item <= 1 && !ui->local_simulation && !ui->volume_available) {
            snprintf(value, 24, "UNAVAILABLE");
            *available = false;
        }
        break;
    }
    case 3: {
        static const char *labels[] = {"RESOLUTION", "ROTATION", "MIRROR", "CAMERA ACCESS"};
        *label = labels[item];
        if (ui->camera_state != 2) {
            snprintf(value, 24, "NO CAMERA");
            *available = false;
        } else if (item == 0)
            snprintf(value, 24, "NOT CONFIGURED");
        else if (item == 1)
            snprintf(value, 24, "%s", rotations[ui->camera_rotation % 4]);
        else if (item == 2)
            snprintf(value, 24, "%s", ui->camera_mirror ? "ON" : "OFF");
        else
            snprintf(value, 24, "%s", capability_state(ui->camera_state));
        if (item <= 2 && !ui->local_simulation && ui->camera_state == 2) {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        }
        break;
    }
    case 4: {
        static const char *labels[] = {"BATTERY STATUS", "BATTERY SAVER",
                                       "CHARGE LIMIT", "RESTART", "POWER OFF"};
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%s", ui->battery_percent >= 0 ? "DETAILS" : "UNKNOWN");
        else if (item == 3 || item == 4)
            snprintf(value, 24, "ACTION");
        else {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        }
        break;
    }
    case 5: {
        static const char *labels[] = {
            "INSTALLED APPS", "PERMISSIONS", "STORAGE",
            "DOCUMENT ACCESS", "AUTO APP UPDATES", "APP METRICS",
        };
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%u APPS", ui->app_count);
        else if (item == 1)
            snprintf(value, 24, "%u POLICY DENIED", ui->denied_permission_count);
        else if (item == 2)
            snprintf(value, 24, "DETAILS");
        else if (item == 3) {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        } else if (item == 4 && !ui->auto_update_available) {
            snprintf(value, 24, "UNKNOWN");
            *available = false;
        } else if (item == 4 && !ui->auto_update_enabled) {
            snprintf(value, 24,
                     "%s", ui->auto_update_policy_allowed ? "OFF" : "LOCKED");
            *available = ui->auto_update_policy_allowed;
        } else if (item == 4 && !ui->auto_update_policy_allowed) {
            snprintf(value, 24, "LOCKED");
        } else if (item == 4 && ui->auto_update_checking) {
            snprintf(value, 24, "CHECKING");
        } else if (item == 4 && !ui->auto_update_charging) {
            snprintf(value, 24, "WAIT POWER");
        } else if (item == 4 && !ui->auto_update_unmetered_network) {
            snprintf(value, 24, "WAIT WIRED");
        } else if (item == 4 && ui->auto_update_due) {
            snprintf(value, 24, "DUE");
        } else if (item == 4) {
            snprintf(value, 24, "ON");
        } else if (!ui->metrics_available) {
            snprintf(value, 24, "UNKNOWN");
            *available = false;
        } else if (!ui->metrics_configured) {
            snprintf(value, 24, "NOT CONFIGURED");
            *available = false;
        } else if (!ui->metrics_policy_allowed) {
            snprintf(value, 24, "LOCKED");
            *available = false;
        } else if (!ui->metrics_enabled) {
            snprintf(value, 24, "OFF");
        } else {
            snprintf(value, 24, "%s", ui->metrics_pending ? "ON - PENDING" : "ON");
        }
        break;
    }
    case 6: {
        static const char *labels[] = {"ABOUT", "HARDWARE DIAGNOSTICS", "DATE & TIME",
                                       "LANGUAGE", "ACCESSIBILITY", "OS UPDATE"};
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%.23s", ui->os_version);
        else if (item == 1)
            snprintf(value, 24, "DETAILS");
        else if (item == 2)
            snprintf(value, 24, "AUTOMATIC");
        else if (item == 3)
            snprintf(value, 24, "ENGLISH");
        else if (item == 4)
            snprintf(value, 24, "DEFAULT");
        else {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        }
        break;
    }
    default: {
        static const char *labels[] = {
            "AUTHORITY", "DEVELOPER MODE", "PAIR NEW COMPUTER",
            "PAIRED COMPUTERS", "RECOVERY BOOT", "CHANGE PASSWORD",
            "SCREEN LOCK", "ENCRYPTION",
        };
        *label = labels[item];
        if (item == 0)
            snprintf(value, 24, "%s",
                     ui->settings_available ? authorities[ui->settings_authority]
                                            : "UNKNOWN");
        else if (item == 1)
            snprintf(value, 24, "%s",
                     settings_state(ui->developer_mode, ui->developer_mode_allowed));
        else if (item == 2)
            snprintf(value, 24, "%s",
                     ui->developer_pairing_open ? "OPEN - 10 MIN" : "ACTION");
        else if (item == 3)
            snprintf(value, 24, "%u PAIRED", ui->developer_host_count);
        else if (item == 4)
            snprintf(value, 24, "%s",
                     settings_state(ui->recovery_mode, ui->recovery_mode_allowed));
        else if (item == 5)
            snprintf(value, 24, "ACTION");
        else {
            snprintf(value, 24, "NOT SUPPORTED");
            *available = false;
        }
        if ((item == 2 || item == 3) &&
            (!ui->developer_mode || !ui->developer_access_available)) {
            snprintf(value, 24, "%s",
                     ui->developer_mode ? "UNAVAILABLE" : "ENABLE DEV MODE");
            *available = false;
        }
        break;
    }
    }
}

static void draw_developer_hosts(struct canvas *canvas,
                                 const struct cp0_ui *ui)
{
    unsigned int count = ui->developer_host_count + 1U;
    unsigned int first = ui->developer_host_selected > 3
                             ? ui->developer_host_selected - 3
                             : 0;
    for (unsigned int row = 0; row < 4 && first + row < count; row++) {
        unsigned int item = first + row;
        bool selected = item == ui->developer_host_selected;
        bool revoke_all = item == ui->developer_host_count;
        int y = 27 + (int)row * 32;
        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? (revoke_all ? COLOR_RED : COLOR_GREEN)
                             : COLOR_BAR);
        draw_text_slice(canvas, 20, y + 6,
                        revoke_all ? "REVOKE ALL" : ui->developer_host_labels[item],
                        0, 26, selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text(canvas, 20, y + 17,
                  revoke_all ? "REMOVE EVERY DEBUG AUTHORIZATION"
                             : "PRESS ENTER TO REVOKE",
                  1, revoke_all ? COLOR_RED : COLOR_MUTED);
    }
    char page[16];
    snprintf(page, sizeof(page), "%u/%u", ui->developer_host_selected + 1U,
             count);
    draw_text(canvas, 278, 158, page, 1, COLOR_MUTED);
}

static void draw_developer_revoke_confirm(struct canvas *canvas,
                                          const struct cp0_ui *ui)
{
    static const char *labels[] = {"REVOKE", "CANCEL"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 24, 35, 272, 108, COLOR_SURFACE);
    stroke_rect(canvas, 24, 35, 272, 108, 2, COLOR_RED);
    draw_text(canvas, 42, 50,
              ui->developer_revoke_all ? "REVOKE ALL COMPUTERS"
                                       : "REVOKE COMPUTER",
              1, COLOR_TEXT);
    draw_text(canvas, 42, 72, "DEPLOYMENT ACCESS ENDS IMMEDIATELY", 1,
              COLOR_MUTED);
    for (unsigned int index = 0; index < 2; index++) {
        int x = 48 + (int)index * 116;
        bool selected = ui->dialog_selected == index;
        fill_rect(canvas, x, 105, 104, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 105, 104, 24, selected ? 2 : 1,
                    selected ? COLOR_RED : COLOR_MUTED);
        draw_text(canvas, x + 24, 114, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_settings_detail(struct canvas *canvas, const struct cp0_ui *ui)
{
    unsigned int count = settings_item_count(ui->settings_selected);
    unsigned int first = ui->settings_item_selected > 3
                             ? ui->settings_item_selected - 3
                             : 0;
    for (unsigned int row = 0; row < 4 && first + row < count; row++) {
        unsigned int item = first + row;
        const char *label;
        char value[24];
        bool available;
        bool selected = item == ui->settings_item_selected;
        int y = 27 + (int)row * 32;
        settings_item(ui, ui->settings_selected, item, &label, value, &available);
        fill_rect(canvas, 8, y, 304, 28,
                  selected ? COLOR_SELECTED : COLOR_SURFACE);
        stroke_rect(canvas, 8, y, 304, 28, selected ? 2 : 1,
                    selected ? (available ? COLOR_GREEN : COLOR_MUTED) : COLOR_BAR);
        draw_text_slice(canvas, 20, y + 6, label, 0, 22,
                        selected ? COLOR_TEXT : COLOR_MUTED);
        draw_text_slice(canvas, 180, y + 17, value, 0, 20,
                        available ? COLOR_GREEN : COLOR_MUTED);
    }
    char page[16];
    snprintf(page, sizeof(page), "%u/%u", ui->settings_item_selected + 1U,
             count);
    draw_text(canvas, 278, 158, page, 1, COLOR_MUTED);
}

static void draw_settings_confirm(struct canvas *canvas,
                                  const struct cp0_ui *ui)
{
    static const char *labels[] = {"ENABLE", "CANCEL"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 24, 35, 272, 108, COLOR_SURFACE);
    stroke_rect(canvas, 24, 35, 272, 108, 2, COLOR_YELLOW);
    const char *heading = ui->settings_confirm_metrics
                              ? "SHARE APP METRICS"
                              : (ui->settings_confirm_recovery
                                     ? "ENABLE RECOVERY BOOT"
                                     : "ENABLE DEVELOPER MODE");
    const char *detail = ui->settings_confirm_metrics
                             ? "WEEKLY COUNTS; NO ID OR LOGS"
                             : (ui->settings_confirm_recovery
                                    ? "NEXT BOOT OPENS CONSOLE"
                                    : "DEV PACKAGES MAY INSTALL");
    draw_text(canvas, 42, 50, heading, 1, COLOR_TEXT);
    draw_text(canvas, 42, 72, detail, 1, COLOR_MUTED);
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

static void draw_password_change(struct canvas *canvas,
                                 const struct cp0_ui *ui)
{
    char status[48];
    const char *label;
    const char *value;

    draw_text(canvas, 12, 36, "OWNER CREDENTIAL", 2, COLOR_TEXT);
    if (ui->password_change_page == CP0_UI_PASSWORD_APPLYING) {
        draw_text(canvas, 12, 77, "UPDATING PASSWORD", 2, COLOR_GREEN);
        draw_text(canvas, 12, 109, "DO NOT POWER OFF", 1, COLOR_YELLOW);
        draw_setup_footer(canvas, "PLEASE WAIT", NULL);
        return;
    }
    if (ui->password_change_page == CP0_UI_PASSWORD_COMPLETE) {
        draw_text(canvas, 12, 75, "PASSWORD UPDATED", 2, COLOR_GREEN);
        draw_text(canvas, 12, 106, "NEW CREDENTIAL IS ACTIVE", 1,
                  COLOR_MUTED);
        draw_setup_footer(canvas, "ESC CLOSE", "ENTER DONE");
        return;
    }
    if (ui->password_change_page == CP0_UI_PASSWORD_CURRENT) {
        label = "CURRENT PASSWORD";
        value = ui->password_secrets->current;
    } else if (ui->password_change_page == CP0_UI_PASSWORD_NEW) {
        label = "NEW PASSWORD";
        value = ui->password_secrets->new_password;
    } else {
        label = "CONFIRM NEW PASSWORD";
        value = ui->password_secrets->confirm;
    }
    draw_text(canvas, 12, 63, label, 1, COLOR_MUTED);
    draw_setup_input(canvas, value, !ui->password_change_show);
    snprintf(status, sizeof(status), "%s  %u%s",
             ui->password_change_show ? "VISIBLE" : "HIDDEN",
             (unsigned int)strlen(value),
             ui->password_change_page == CP0_UI_PASSWORD_NEW ? "/10 MIN"
                                                              : " CHARS");
    draw_text(canvas, 12, 123, status, 1, COLOR_MUTED);
    if (ui->password_secrets->error[0] != '\0')
        draw_text_slice(canvas, 12, 136, ui->password_secrets->error, 0, 46,
                        COLOR_RED);
    draw_setup_footer(canvas, "RIGHT SHOW/HIDE", "ENTER NEXT");
}

static void draw_store_install_prompt(struct canvas *canvas,
                                      const struct cp0_ui *ui)
{
    static const char *labels[] = {"INSTALL", "CANCEL"};
    char heading[32];
    char permissions[32];
    char storage[48];
    char required[16];
    char available[16];
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 20, 29, 280, 126, COLOR_SURFACE);
    stroke_rect(canvas, 20, 29, 280, 126, 2,
                ui->store_preflight_error == CP0_UI_STORE_PREFLIGHT_NONE
                    ? COLOR_YELLOW
                    : COLOR_RED);
    if (ui->store_preflight_error != CP0_UI_STORE_PREFLIGHT_NONE) {
        static const char *errors[] = {
            "", "BLOCKED BY DEVICE POLICY", "NOT ENOUGH STORAGE",
            "CATALOG CHANGED - RETRY", "STORE PREFLIGHT UNAVAILABLE",
        };
        draw_text(canvas, 38, 45, "INSTALL BLOCKED", 1, COLOR_RED);
        draw_text(canvas, 38, 68, errors[ui->store_preflight_error], 1,
                  COLOR_TEXT);
        fill_rect(canvas, 104, 112, 112, 25, COLOR_SELECTED);
        stroke_rect(canvas, 104, 112, 112, 25, 2, COLOR_GREEN);
        draw_text(canvas, 143, 121, "CLOSE", 1, COLOR_TEXT);
        return;
    }
    snprintf(heading, sizeof(heading), "INSTALL %u APP%s?",
             ui->store_preflight_app_count,
             ui->store_preflight_app_count == 1 ? "" : "S");
    snprintf(permissions, sizeof(permissions), "%u NEW PERMISSIONS",
             ui->store_preflight_new_permissions);
    format_bytes(required, ui->store_preflight_required_bytes);
    format_bytes(available, ui->store_preflight_available_bytes);
    snprintf(storage, sizeof(storage), "NEED %s / FREE %s", required,
             available);
    draw_text(canvas, 38, 42, heading, 1, COLOR_TEXT);
    draw_text(canvas, 38, 61, permissions, 1,
              ui->store_preflight_new_permissions > 0 ? COLOR_YELLOW
                                                      : COLOR_GREEN);
    draw_text(canvas, 38, 77, storage, 1, COLOR_MUTED);
    if (ui->store_preflight_denied_permissions > 0) {
        char denied[32];
        snprintf(denied, sizeof(denied), "POLICY BLOCKS %u REQUESTS",
                 ui->store_preflight_denied_permissions);
        draw_text(canvas, 38, 92, denied, 1, COLOR_RED);
    }
    for (unsigned int index = 0; index < 2; index++) {
        int x = 38 + (int)index * 124;
        bool selected = ui->dialog_selected == index;
        fill_rect(canvas, x, 116, 112, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 116, 112, 24, selected ? 2 : 1,
                    selected ? (index == 0 ? COLOR_YELLOW : COLOR_GREEN)
                             : COLOR_MUTED);
        draw_text(canvas, x + 31, 125, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_page(struct canvas *canvas, const struct cp0_ui *ui)
{
    switch (ui->screen) {
    case CP0_UI_APPS:
        if (ui->app_detail && ui->app_selected < ui->app_count)
            draw_app_detail(canvas, ui);
        else
            draw_apps_page(canvas, ui);
        break;
    case CP0_UI_STORE:
        draw_store_page(canvas, ui);
        break;
    case CP0_UI_DEVICE:
        draw_device_page(canvas, ui);
        break;
    case CP0_UI_NETWORK:
        draw_network_page(canvas, ui);
        break;
    case CP0_UI_SETTINGS:
        if (ui->password_change_active)
            draw_password_change(canvas, ui);
        else if (ui->developer_hosts_view)
            draw_developer_hosts(canvas, ui);
        else if (ui->settings_detail)
            draw_settings_detail(canvas, ui);
        else
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

static void draw_uninstall_confirm(struct canvas *canvas,
                                   const struct cp0_ui *ui)
{
    static const char *labels[] = {"UNINSTALL", "CANCEL"};
    const struct cp0_ui_app *app = &ui->apps[ui->app_selected];
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 24, 33, 272, 116, COLOR_SURFACE);
    stroke_rect(canvas, 24, 33, 272, 116, 2, COLOR_RED);
    draw_text_slice(canvas, 42, 47, app->name, 0, 38, COLOR_TEXT);
    draw_text(canvas, 42, 68, "REMOVE APP PACKAGE?", 1, COLOR_RED);
    draw_text(canvas, 42, 84, "PRIVATE DATA IS RETAINED", 1, COLOR_YELLOW);
    for (unsigned int index = 0; index < 2; index++) {
        int x = 42 + (int)index * 122;
        bool selected = ui->dialog_selected == index;
        fill_rect(canvas, x, 111, 110, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 111, 110, 24, selected ? 2 : 1,
                    selected ? (index == 0 ? COLOR_RED : COLOR_GREEN)
                             : COLOR_MUTED);
        draw_text(canvas, x + 20, 120, labels[index], 1,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_system_action_overlay(struct canvas *canvas,
                                       const struct cp0_ui *ui)
{
    static const char *labels[] = {"BRIGHTNESS", "VOLUME", "MUTED", "PLAY / PAUSE",
                                   "PREVIOUS", "NEXT", "SCREENSHOT"};
    char value[24];
    unsigned int kind = ui->system_action_kind < 7 ? ui->system_action_kind : 0;
    fill_rect(canvas, 62, 25, 196, 60, 0x00090b0cu);
    stroke_rect(canvas, 62, 25, 196, 60, 2, COLOR_GREEN);
    draw_text(canvas, 82, 38, labels[kind], 1, COLOR_TEXT);
    if (kind == 0 && !ui->local_simulation && !ui->brightness_available)
        snprintf(value, sizeof(value), "UNAVAILABLE");
    else if (!ui->local_simulation && (kind == 1 || kind == 2) &&
             !ui->volume_available)
        snprintf(value, sizeof(value), "UNAVAILABLE");
    else if (kind == 0)
        snprintf(value, sizeof(value), "%u%%", ui->brightness_percent);
    else if (kind == 1)
        snprintf(value, sizeof(value), "%u%%", ui->volume_percent);
    else if (kind == 2)
        snprintf(value, sizeof(value), "%s", ui->muted ? "ON" : "OFF");
    else if (kind == 6) {
        static const char *states[] = {
            "REQUESTED", "SAVED", "FAILED", "UNAVAILABLE", "BUSY",
        };
        unsigned int status =
            ui->screenshot_status <= CP0_UI_SCREENSHOT_BUSY
                ? (unsigned int)ui->screenshot_status
                : (unsigned int)CP0_UI_SCREENSHOT_FAILED;
        snprintf(value, sizeof(value), "%s", states[status]);
    } else {
        static const char *states[] = {
            "REQUESTED", "SENT", "UNAVAILABLE", "BUSY", "FAILED",
        };
        unsigned int status =
            ui->media_status <= CP0_UI_MEDIA_FAILED
                ? (unsigned int)ui->media_status
                : (unsigned int)CP0_UI_MEDIA_FAILED;
        snprintf(value, sizeof(value), "%s", states[status]);
    }
    draw_text(canvas, 82, 61, value, 2,
              kind == 2 && ui->muted ? COLOR_YELLOW : COLOR_GREEN);
}

static void draw_help_overlay(struct canvas *canvas)
{
    fill_rect(canvas, 12, 27, 296, 135, COLOR_SURFACE);
    stroke_rect(canvas, 12, 27, 296, 135, 2, COLOR_GREEN);
    draw_text(canvas, 28, 39, "SYSTEM KEYS", 2, COLOR_TEXT);
    draw_labeled_value(canvas, 70, "F1 / F2", "HOME / BACK", COLOR_GREEN);
    draw_labeled_value(canvas, 90, "F3 / F4", "TASKS / POWER", COLOR_GREEN);
    draw_labeled_value(canvas, 110, "FN U / I", "BRIGHTNESS", COLOR_TEXT);
    draw_labeled_value(canvas, 130, "FN A / S / D", "MUTE / VOLUME", COLOR_TEXT);
    draw_text(canvas, 28, 150, "ESC CLOSE", 1, COLOR_MUTED);
}

static void draw_power_dialog(struct canvas *canvas, const struct cp0_ui *ui)
{
    static const char *labels[] = {"SLEEP", "RESTART", "POWER OFF", "CANCEL"};
    fill_rect(canvas, 0, 21, CP0_UI_WIDTH, CP0_UI_HEIGHT - 21, 0x00090b0cu);
    fill_rect(canvas, 36, 35, 248, 105, COLOR_SURFACE);
    stroke_rect(canvas, 36, 35, 248, 105, 2, COLOR_GREEN);
    draw_text(canvas, 54, 52, "POWER", 2, COLOR_TEXT);

    for (unsigned int index = 0; index < 4; index++) {
        int x = 43 + (int)index * 59;
        bool selected = index == ui->dialog_selected;
        fill_rect(canvas, x, 101, 54, 24,
                  selected ? COLOR_SELECTED : COLOR_BAR);
        stroke_rect(canvas, x, 101, 54, 24, selected ? 2 : 1,
                    selected ? COLOR_GREEN : COLOR_MUTED);
        draw_text_slice(canvas, x + 4, 110, labels[index], 0, 8,
                  selected ? COLOR_TEXT : COLOR_MUTED);
    }
}

static void draw_text_slice(struct canvas *canvas, int x, int y,
                            const char *text, size_t start, size_t maximum,
                            uint32_t color)
{
    char line[47];
    size_t length = strlen(text);
    size_t output = 0;

    while (start < length && output < maximum && output + 1 < sizeof(line)) {
        unsigned char byte = (unsigned char)text[start++];
        line[output++] = byte >= 0x20U && byte < 0x7fU ? (char)byte : ' ';
    }
    line[output] = '\0';
    draw_text(canvas, x, y, line, 1, color);
}

static size_t next_wrapped_offset(const char *text, size_t start,
                                  size_t maximum)
{
    size_t length = strlen(text);
    while (start < length && (text[start] == ' ' || text[start] == '\n'))
        start++;
    if (start >= length)
        return length;
    size_t limit = length - start < maximum ? length : start + maximum;
    size_t last_space = SIZE_MAX;
    for (size_t cursor = start; cursor < limit; cursor++) {
        if (text[cursor] == '\n')
            return cursor + 1U;
        if (text[cursor] == ' ')
            last_space = cursor;
    }
    size_t next = limit < length && last_space != SIZE_MAX && last_space > start
                      ? last_space + 1U
                      : limit;
    while (next < length && text[next] == ' ')
        next++;
    return next;
}

static size_t wrapped_line_start(const char *text, size_t line,
                                 size_t maximum)
{
    size_t start = 0;
    size_t length = strlen(text);
    while (start < length && (text[start] == ' ' || text[start] == '\n'))
        start++;
    for (size_t index = 0; index < line && start < length; index++) {
        size_t next = next_wrapped_offset(text, start, maximum);
        if (next <= start)
            return length;
        start = next;
        while (start < length &&
               (text[start] == ' ' || text[start] == '\n'))
            start++;
    }
    return start;
}

static size_t wrapped_line_count(const char *text, size_t maximum)
{
    size_t length = strlen(text);
    size_t start = wrapped_line_start(text, 0, maximum);
    size_t lines = 0;
    while (start < length) {
        size_t next = next_wrapped_offset(text, start, maximum);
        lines++;
        if (next <= start)
            break;
        start = next;
        while (start < length &&
               (text[start] == ' ' || text[start] == '\n'))
            start++;
    }
    return lines;
}

static void draw_wrapped_line(struct canvas *canvas, int x, int y,
                              const char *text, size_t line, size_t maximum,
                              uint32_t color)
{
    size_t start = wrapped_line_start(text, line, maximum);
    size_t next = next_wrapped_offset(text, start, maximum);
    size_t length = next > start ? next - start : 0;
    while (length > 0 &&
           (text[start + length - 1U] == ' ' ||
            text[start + length - 1U] == '\n'))
        length--;
    draw_text_slice(canvas, x, y, text, start,
                    length < maximum ? length : maximum, color);
}

static void draw_prompt_line(struct canvas *canvas, int y, const char *text,
                             size_t start, size_t maximum)
{
    draw_text_slice(canvas, 20, y, text, start, maximum, COLOR_TEXT);
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
    ui->tasks = calloc(CP0_UI_MAX_TASKS, sizeof(*ui->tasks));
    ui->navigation =
        calloc(CP0_UI_NAVIGATION_DEPTH, sizeof(*ui->navigation));
    ui->password_secrets = calloc(1, sizeof(*ui->password_secrets));
    ui->screen = CP0_UI_HOME;
    ui->store_status = CP0_UI_STORE_LOADING;
    ui->store_browse_status = CP0_UI_STORE_LOADING;
    ui->store_search_status = CP0_UI_STORE_LOADING;
    ui->battery_percent = -1;
    ui->temperature_millicelsius = -1;
    ui->wifi_enabled = true;
    ui->brightness_percent = 70;
    ui->volume_percent = 60;
    ui->key_sounds = true;
    ui->screen_timeout = 1;
    memcpy(ui->clock_text, "--:--", sizeof(ui->clock_text));
}

void cp0_ui_deinit(struct cp0_ui *ui)
{
    if (ui == NULL)
        return;
    clear_sensitive(ui->setup_password, sizeof(ui->setup_password));
    clear_sensitive(ui->setup_password_confirm,
                    sizeof(ui->setup_password_confirm));
    clear_sensitive(ui->setup_wifi_password,
                    sizeof(ui->setup_wifi_password));
    cancel_password_change(ui);
    free(ui->password_secrets);
    ui->password_secrets = NULL;
    free(ui->tasks);
    ui->tasks = NULL;
    free(ui->navigation);
    ui->navigation = NULL;
    ui->navigation_depth = 0;
    ui->task_count = 0;
    ui->task_selected = 0;
}

void cp0_ui_setup_begin(struct cp0_ui *ui, enum cp0_ui_setup_page page)
{
    if (ui == NULL || page > CP0_UI_SETUP_REPAIR)
        return;
    ui->setup_active = true;
    ui->setup_page = page;
    ui->setup_error_return_page = page;
    ui->setup_show_password = false;
    ui->setup_busy = false;
    ui->setup_error[0] = '\0';
    ui->power_dialog = false;
    ui->help_overlay = false;
    ui->system_action_overlay = false;
    ui->notification_banner = false;
}

void cp0_ui_setup_resume(struct cp0_ui *ui, unsigned int phase,
                         const char *hostname, const char *display_name,
                         const char *username, bool ssh_enabled)
{
    static const enum cp0_ui_setup_page pages[] = {
        CP0_UI_SETUP_WELCOME,      CP0_UI_SETUP_WELCOME,
        CP0_UI_SETUP_DISPLAY_NAME, CP0_UI_SETUP_PASSWORD,
        CP0_UI_SETUP_NETWORK,      CP0_UI_SETUP_SSH,
        CP0_UI_SETUP_REVIEW,       CP0_UI_SETUP_APPLYING,
        CP0_UI_SETUP_COMPLETE,     CP0_UI_SETUP_REPAIR,
    };
    if (ui == NULL || phase >= sizeof(pages) / sizeof(pages[0]))
        return;
    cp0_ui_setup_begin(ui, pages[phase]);
    copy_optional_text(ui->setup_hostname, sizeof(ui->setup_hostname),
                       hostname);
    copy_optional_text(ui->setup_display_name,
                       sizeof(ui->setup_display_name), display_name);
    copy_optional_text(ui->setup_username, sizeof(ui->setup_username),
                       username);
    ui->setup_ssh_enabled = ssh_enabled;
    if (phase >= 4U) {
        memset(ui->setup_password, 0, sizeof(ui->setup_password));
        memset(ui->setup_password_confirm, 0,
               sizeof(ui->setup_password_confirm));
    }
    if (phase >= 5U)
        memset(ui->setup_wifi_password, 0,
               sizeof(ui->setup_wifi_password));
}

void cp0_ui_setup_set_wifi(struct cp0_ui *ui,
                           const struct cp0_ui_setup_wifi *networks,
                           size_t network_count)
{
    if (ui == NULL || (networks == NULL && network_count > 0))
        return;
    if (network_count > CP0_UI_SETUP_WIFI_MAX)
        network_count = CP0_UI_SETUP_WIFI_MAX;
    memset(ui->setup_wifi_ssids, 0, sizeof(ui->setup_wifi_ssids));
    memset(ui->setup_wifi_security, 0, sizeof(ui->setup_wifi_security));
    memset(ui->setup_wifi_signal, 0, sizeof(ui->setup_wifi_signal));
    memset(ui->setup_wifi_connected, 0, sizeof(ui->setup_wifi_connected));
    ui->setup_wifi_count = 0;
    for (size_t index = 0; index < network_count; index++) {
        unsigned int destination = ui->setup_wifi_count;
        if (networks[index].security > 3 ||
            networks[index].signal_percent > 100 ||
            !copy_optional_text(ui->setup_wifi_ssids[destination],
                                sizeof(ui->setup_wifi_ssids[destination]),
                                networks[index].ssid) ||
            ui->setup_wifi_ssids[destination][0] == '\0')
            continue;
        ui->setup_wifi_security[destination] =
            (uint8_t)networks[index].security;
        ui->setup_wifi_signal[destination] =
            (uint8_t)networks[index].signal_percent;
        ui->setup_wifi_connected[destination] = networks[index].connected;
        ui->setup_wifi_count++;
    }
    ui->setup_wifi_selected = 0;
    ui->setup_page = CP0_UI_SETUP_WIFI_LIST;
    ui->setup_busy = false;
    ui->setup_error[0] = '\0';
}

void cp0_ui_setup_set_busy(struct cp0_ui *ui, const char *title,
                           const char *detail)
{
    if (ui == NULL)
        return;
    ui->setup_busy = title != NULL && title[0] != '\0';
    copy_optional_text(ui->setup_busy_title, sizeof(ui->setup_busy_title),
                       title);
    copy_optional_text(ui->setup_busy_detail, sizeof(ui->setup_busy_detail),
                       detail);
}

void cp0_ui_setup_set_network_status(struct cp0_ui *ui,
                                     bool manager_available,
                                     bool ethernet_connected,
                                     const char *ethernet_ipv4,
                                     bool wifi_available,
                                     bool wifi_connected,
                                     const char *wifi_ipv4)
{
    if (ui == NULL)
        return;
    ui->setup_network_manager_available = manager_available;
    ui->setup_ethernet_connected = ethernet_connected;
    ui->setup_wifi_available = wifi_available;
    ui->setup_wifi_link_connected = wifi_connected;
    copy_optional_text(ui->setup_ethernet_ipv4,
                       sizeof(ui->setup_ethernet_ipv4), ethernet_ipv4);
    copy_optional_text(ui->setup_wifi_ipv4, sizeof(ui->setup_wifi_ipv4),
                       wifi_ipv4);
}

void cp0_ui_setup_result(struct cp0_ui *ui, enum cp0_ui_event event,
                         bool success, const char *error)
{
    if (ui == NULL)
        return;
    ui->setup_busy = false;
    if (!success) {
        ui->setup_error_return_page = ui->setup_page;
        ui->setup_page = CP0_UI_SETUP_ERROR;
        copy_optional_text(ui->setup_error, sizeof(ui->setup_error), error);
        if (ui->setup_error[0] == '\0')
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Operation failed");
        return;
    }
    ui->setup_error[0] = '\0';
    switch (event) {
    case CP0_UI_EVENT_SETUP_SET_REGION:
        ui->setup_page = CP0_UI_SETUP_DISPLAY_NAME;
        break;
    case CP0_UI_EVENT_SETUP_SET_OWNER:
        ui->setup_page = CP0_UI_SETUP_PASSWORD;
        break;
    case CP0_UI_EVENT_SETUP_SET_PASSWORD:
        memset(ui->setup_password, 0, sizeof(ui->setup_password));
        memset(ui->setup_password_confirm, 0,
               sizeof(ui->setup_password_confirm));
        ui->setup_page = CP0_UI_SETUP_NETWORK;
        break;
    case CP0_UI_EVENT_SETUP_CONNECT_WIFI:
        memset(ui->setup_wifi_password, 0,
               sizeof(ui->setup_wifi_password));
        ui->setup_page = CP0_UI_SETUP_SSH;
        break;
    case CP0_UI_EVENT_SETUP_USE_ETHERNET:
    case CP0_UI_EVENT_SETUP_USE_OFFLINE:
        ui->setup_page = CP0_UI_SETUP_SSH;
        break;
    case CP0_UI_EVENT_SETUP_SET_SSH:
        ui->setup_page = CP0_UI_SETUP_REVIEW;
        break;
    case CP0_UI_EVENT_SETUP_COMMIT:
        ui->setup_page = CP0_UI_SETUP_COMPLETE;
        break;
    default:
        break;
    }
}

bool cp0_ui_setup_accepts_text(const struct cp0_ui *ui)
{
    if (ui == NULL || !ui->setup_active)
        return false;
    return ui->setup_page == CP0_UI_SETUP_HOSTNAME ||
           ui->setup_page == CP0_UI_SETUP_DISPLAY_NAME ||
           ui->setup_page == CP0_UI_SETUP_USERNAME ||
           ui->setup_page == CP0_UI_SETUP_PASSWORD ||
           ui->setup_page == CP0_UI_SETUP_PASSWORD_CONFIRM ||
           ui->setup_page == CP0_UI_SETUP_WIFI_PASSWORD;
}

static char *setup_input_buffer(struct cp0_ui *ui, size_t *capacity)
{
    switch (ui->setup_page) {
    case CP0_UI_SETUP_HOSTNAME:
        *capacity = sizeof(ui->setup_hostname);
        return ui->setup_hostname;
    case CP0_UI_SETUP_DISPLAY_NAME:
        *capacity = sizeof(ui->setup_display_name);
        return ui->setup_display_name;
    case CP0_UI_SETUP_USERNAME:
        *capacity = sizeof(ui->setup_username);
        return ui->setup_username;
    case CP0_UI_SETUP_PASSWORD:
        *capacity = sizeof(ui->setup_password);
        return ui->setup_password;
    case CP0_UI_SETUP_PASSWORD_CONFIRM:
        *capacity = sizeof(ui->setup_password_confirm);
        return ui->setup_password_confirm;
    case CP0_UI_SETUP_WIFI_PASSWORD:
        *capacity = sizeof(ui->setup_wifi_password);
        return ui->setup_wifi_password;
    default:
        *capacity = 0;
        return NULL;
    }
}

bool cp0_ui_setup_input_ascii(struct cp0_ui *ui, char character)
{
    size_t capacity;
    char *buffer;
    size_t length;
    if (!cp0_ui_setup_accepts_text(ui) || character < ' ' ||
        character > '~')
        return false;
    buffer = setup_input_buffer(ui, &capacity);
    length = strlen(buffer);
    if (length + 1U >= capacity)
        return false;
    if (ui->setup_page == CP0_UI_SETUP_WIFI_PASSWORD && length >= 63U)
        return false;
    if (ui->setup_page == CP0_UI_SETUP_HOSTNAME) {
        if (character >= 'A' && character <= 'Z')
            character = (char)(character - 'A' + 'a');
        if (!((character >= 'a' && character <= 'z') ||
              (character >= '0' && character <= '9') || character == '-') ||
            (length == 0 && character == '-'))
            return false;
    } else if (ui->setup_page == CP0_UI_SETUP_USERNAME) {
        if (character >= 'A' && character <= 'Z')
            character = (char)(character - 'A' + 'a');
        if (!((character >= 'a' && character <= 'z') ||
              (length > 0 && character >= '0' && character <= '9') ||
              (length > 0 && (character == '-' || character == '_'))))
            return false;
    } else if (ui->setup_page == CP0_UI_SETUP_DISPLAY_NAME &&
               character == ':') {
        return false;
    }
    buffer[length] = character;
    buffer[length + 1U] = '\0';
    ui->setup_error[0] = '\0';
    return true;
}

bool cp0_ui_setup_backspace(struct cp0_ui *ui)
{
    size_t capacity;
    char *buffer;
    size_t length;
    if (!cp0_ui_setup_accepts_text(ui))
        return false;
    buffer = setup_input_buffer(ui, &capacity);
    (void)capacity;
    length = strlen(buffer);
    if (length == 0)
        return false;
    buffer[length - 1U] = '\0';
    ui->setup_error[0] = '\0';
    return true;
}

static char *password_change_input_buffer(struct cp0_ui *ui)
{
    switch (ui->password_change_page) {
    case CP0_UI_PASSWORD_CURRENT:
        return ui->password_secrets->current;
    case CP0_UI_PASSWORD_NEW:
        return ui->password_secrets->new_password;
    case CP0_UI_PASSWORD_CONFIRM:
        return ui->password_secrets->confirm;
    default:
        return NULL;
    }
}

bool cp0_ui_password_accepts_text(const struct cp0_ui *ui)
{
    return ui != NULL && ui->password_change_active &&
           ui->password_secrets != NULL &&
           ui->password_change_page <= CP0_UI_PASSWORD_CONFIRM;
}

bool cp0_ui_password_input_ascii(struct cp0_ui *ui, char character)
{
    char *buffer;
    size_t length;

    if (!cp0_ui_password_accepts_text(ui) || character < ' ' ||
        character > '~')
        return false;
    buffer = password_change_input_buffer(ui);
    if (buffer == NULL)
        return false;
    length = strlen(buffer);
    if (length >= CP0_UI_PASSWORD_MAX)
        return false;
    buffer[length] = character;
    buffer[length + 1U] = '\0';
    clear_sensitive(ui->password_secrets->error,
                    sizeof(ui->password_secrets->error));
    return true;
}

bool cp0_ui_password_backspace(struct cp0_ui *ui)
{
    char *buffer;
    size_t length;

    if (!cp0_ui_password_accepts_text(ui))
        return false;
    buffer = password_change_input_buffer(ui);
    if (buffer == NULL)
        return false;
    length = strlen(buffer);
    if (length == 0)
        return false;
    buffer[length - 1U] = '\0';
    clear_sensitive(ui->password_secrets->error,
                    sizeof(ui->password_secrets->error));
    return true;
}

void cp0_ui_password_change_result(struct cp0_ui *ui, bool success,
                                   bool authentication_failed,
                                   const char *error)
{
    if (ui == NULL || !ui->password_change_active ||
        ui->password_change_page != CP0_UI_PASSWORD_APPLYING)
        return;
    clear_password_change_secrets(ui);
    ui->password_change_show = false;
    if (success) {
        ui->password_change_page = CP0_UI_PASSWORD_COMPLETE;
        return;
    }
    ui->password_change_page = CP0_UI_PASSWORD_CURRENT;
    copy_optional_text(ui->password_secrets->error,
                       sizeof(ui->password_secrets->error),
                       authentication_failed
                           ? "Current password is incorrect"
                           : (error != NULL && error[0] != '\0'
                                  ? error
                                  : "Password update failed"));
}

const char *cp0_ui_setup_locale(const struct cp0_ui *ui)
{
    return ui != NULL && ui->setup_language == 1 ? "zh_CN.UTF-8"
                                                  : "en_US.UTF-8";
}

const char *cp0_ui_setup_country_code(const struct cp0_ui *ui)
{
    static const char *values[] = {"CN", "US", "GB", "DE", "JP"};
    return ui != NULL && ui->setup_country < 5 ? values[ui->setup_country]
                                                : "US";
}

const char *cp0_ui_setup_timezone_name(const struct cp0_ui *ui)
{
    static const char *values[] = {
        "Asia/Shanghai", "America/Los_Angeles", "Europe/London",
        "Europe/Berlin", "Asia/Tokyo",
    };
    return ui != NULL && ui->setup_timezone < 5 ? values[ui->setup_timezone]
                                                 : "UTC";
}

static enum cp0_ui_event handle_setup_action(struct cp0_ui *ui,
                                              enum cp0_ui_action action)
{
    ui->setup_error[0] = '\0';
    if (action == CP0_UI_GO_HOME || action == CP0_UI_SHOW_TASKS ||
        action == CP0_UI_SHOW_POWER || action == CP0_UI_SCREENSHOT ||
        action == CP0_UI_HELP)
        return CP0_UI_EVENT_NONE;
    switch (ui->setup_page) {
    case CP0_UI_SETUP_WELCOME:
        if (action == CP0_UI_ACCEPT)
            ui->setup_page = CP0_UI_SETUP_LANGUAGE;
        break;
    case CP0_UI_SETUP_LANGUAGE:
        if (action == CP0_UI_LEFT || action == CP0_UI_RIGHT ||
            action == CP0_UI_UP || action == CP0_UI_DOWN)
            ui->setup_language = ui->setup_language == 0 ? 1 : 0;
        else if (action == CP0_UI_ACCEPT)
            ui->setup_page = CP0_UI_SETUP_COUNTRY;
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_WELCOME;
        break;
    case CP0_UI_SETUP_COUNTRY:
        if (action == CP0_UI_UP)
            ui->setup_country = (ui->setup_country + 4U) % 5U;
        else if (action == CP0_UI_DOWN)
            ui->setup_country = (ui->setup_country + 1U) % 5U;
        else if (action == CP0_UI_ACCEPT) {
            ui->setup_timezone = ui->setup_country;
            ui->setup_page = CP0_UI_SETUP_TIMEZONE;
        } else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_LANGUAGE;
        break;
    case CP0_UI_SETUP_TIMEZONE:
        if (action == CP0_UI_LEFT)
            ui->setup_timezone = (ui->setup_timezone + 4U) % 5U;
        else if (action == CP0_UI_RIGHT)
            ui->setup_timezone = (ui->setup_timezone + 1U) % 5U;
        else if (action == CP0_UI_ACCEPT)
            ui->setup_page = CP0_UI_SETUP_HOSTNAME;
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_COUNTRY;
        break;
    case CP0_UI_SETUP_HOSTNAME: {
        size_t length = strlen(ui->setup_hostname);
        if (action == CP0_UI_ACCEPT && length > 0 &&
            ui->setup_hostname[length - 1U] != '-')
            return CP0_UI_EVENT_SETUP_SET_REGION;
        if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Enter a valid device name");
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_TIMEZONE;
        break;
    }
    case CP0_UI_SETUP_DISPLAY_NAME:
        if (action == CP0_UI_ACCEPT && ui->setup_display_name[0] != '\0')
            ui->setup_page = CP0_UI_SETUP_USERNAME;
        else if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Enter the owner name");
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_HOSTNAME;
        break;
    case CP0_UI_SETUP_USERNAME:
        if (action == CP0_UI_ACCEPT && strlen(ui->setup_username) >= 2U &&
            strcmp(ui->setup_username, "root") != 0 &&
            strncmp(ui->setup_username, "cp0-", 4) != 0)
            return CP0_UI_EVENT_SETUP_SET_OWNER;
        if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Username is too short or reserved");
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_DISPLAY_NAME;
        break;
    case CP0_UI_SETUP_PASSWORD:
        if (action == CP0_UI_RIGHT)
            ui->setup_show_password = !ui->setup_show_password;
        else if (action == CP0_UI_ACCEPT && strlen(ui->setup_password) >= 10U) {
            ui->setup_show_password = false;
            ui->setup_page = CP0_UI_SETUP_PASSWORD_CONFIRM;
        } else if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Password must have at least 10 characters");
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_USERNAME;
        break;
    case CP0_UI_SETUP_PASSWORD_CONFIRM:
        if (action == CP0_UI_RIGHT)
            ui->setup_show_password = !ui->setup_show_password;
        else if (action == CP0_UI_ACCEPT &&
                 strcmp(ui->setup_password, ui->setup_password_confirm) == 0)
            return CP0_UI_EVENT_SETUP_SET_PASSWORD;
        else if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Passwords do not match");
        else if (action == CP0_UI_BACK) {
            memset(ui->setup_password_confirm, 0,
                   sizeof(ui->setup_password_confirm));
            ui->setup_page = CP0_UI_SETUP_PASSWORD;
        }
        break;
    case CP0_UI_SETUP_NETWORK:
        if (action == CP0_UI_UP && ui->setup_network > 0)
            ui->setup_network--;
        else if (action == CP0_UI_DOWN && ui->setup_network < 2)
            ui->setup_network++;
        else if (action == CP0_UI_ACCEPT && ui->setup_network == 0)
            return CP0_UI_EVENT_SETUP_USE_ETHERNET;
        else if (action == CP0_UI_ACCEPT && ui->setup_network == 1)
            return CP0_UI_EVENT_SETUP_LIST_WIFI;
        else if (action == CP0_UI_ACCEPT)
            return CP0_UI_EVENT_SETUP_USE_OFFLINE;
        break;
    case CP0_UI_SETUP_WIFI_LIST:
        if (action == CP0_UI_UP && ui->setup_wifi_selected > 0)
            ui->setup_wifi_selected--;
        else if (action == CP0_UI_DOWN &&
                 ui->setup_wifi_selected + 1U < ui->setup_wifi_count)
            ui->setup_wifi_selected++;
        else if (action == CP0_UI_RIGHT)
            return CP0_UI_EVENT_SETUP_LIST_WIFI;
        else if (action == CP0_UI_ACCEPT && ui->setup_wifi_count > 0) {
            if (ui->setup_wifi_security[ui->setup_wifi_selected] == 0)
                return CP0_UI_EVENT_SETUP_CONNECT_WIFI;
            if (ui->setup_wifi_security[ui->setup_wifi_selected] == 3)
                copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                                   "This Wi-Fi security is not supported");
            else
                ui->setup_page = CP0_UI_SETUP_WIFI_PASSWORD;
        } else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_NETWORK;
        break;
    case CP0_UI_SETUP_WIFI_PASSWORD:
        if (action == CP0_UI_RIGHT)
            ui->setup_show_password = !ui->setup_show_password;
        else if (action == CP0_UI_ACCEPT &&
                 strlen(ui->setup_wifi_password) >= 8U)
            return CP0_UI_EVENT_SETUP_CONNECT_WIFI;
        else if (action == CP0_UI_ACCEPT)
            copy_optional_text(ui->setup_error, sizeof(ui->setup_error),
                               "Wi-Fi password must have 8 characters");
        else if (action == CP0_UI_BACK) {
            memset(ui->setup_wifi_password, 0,
                   sizeof(ui->setup_wifi_password));
            ui->setup_page = CP0_UI_SETUP_WIFI_LIST;
        }
        break;
    case CP0_UI_SETUP_SSH:
        if (action == CP0_UI_LEFT || action == CP0_UI_RIGHT ||
            action == CP0_UI_UP || action == CP0_UI_DOWN)
            ui->setup_ssh_enabled = !ui->setup_ssh_enabled;
        else if (action == CP0_UI_ACCEPT)
            return CP0_UI_EVENT_SETUP_SET_SSH;
        else if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_NETWORK;
        break;
    case CP0_UI_SETUP_REVIEW:
        if (action == CP0_UI_ACCEPT) {
            ui->setup_page = CP0_UI_SETUP_APPLYING;
            return CP0_UI_EVENT_SETUP_COMMIT;
        }
        if (action == CP0_UI_BACK)
            ui->setup_page = CP0_UI_SETUP_SSH;
        break;
    case CP0_UI_SETUP_COMPLETE:
        if (action == CP0_UI_ACCEPT)
            return CP0_UI_EVENT_SETUP_START;
        break;
    case CP0_UI_SETUP_ERROR:
        if (action == CP0_UI_ACCEPT)
            return CP0_UI_EVENT_SETUP_RETRY;
        break;
    case CP0_UI_SETUP_APPLYING:
    case CP0_UI_SETUP_REPAIR:
        break;
    }
    return CP0_UI_EVENT_NONE;
}

void cp0_ui_set_device_info(struct cp0_ui *ui,
                            const struct cp0_ui_device_info *info)
{
    if (ui == NULL || info == NULL)
        return;
    ui->device_available = info->available;
    ui->battery_percent = info->battery_percent;
    ui->temperature_millicelsius = info->temperature_millicelsius;
    ui->battery_present = info->battery_present;
    ui->battery_voltage_available = info->battery_voltage_available;
    ui->battery_current_available = info->battery_current_available;
    ui->battery_voltage_microvolts = info->battery_voltage_microvolts;
    ui->battery_current_microamps = info->battery_current_microamps;
    ui->battery_status = info->battery_status;
    ui->i2c_bus_state = info->i2c_bus_state;
    ui->display_state = info->display_state;
    ui->keyboard_state = info->keyboard_state;
    ui->audio_state = info->audio_state;
    ui->camera_state = info->camera_state;
    ui->uptime_seconds = info->uptime_seconds;
    ui->memory_total_bytes = info->memory_total_bytes;
    ui->memory_available_bytes = info->memory_available_bytes;
    ui->storage_total_bytes = info->storage_total_bytes;
    ui->storage_available_bytes = info->storage_available_bytes;
    copy_optional_text(ui->device_model, sizeof(ui->device_model), info->model);
    copy_optional_text(ui->os_version, sizeof(ui->os_version), info->os_version);
}

void cp0_ui_set_display_state(struct cp0_ui *ui, bool available,
                              unsigned int brightness_percent)
{
    if (ui == NULL || (available && brightness_percent > 100U))
        return;
    ui->brightness_available = available;
    if (available)
        ui->brightness_percent = brightness_percent;
}

void cp0_ui_set_audio_output_state(struct cp0_ui *ui, bool available,
                                   unsigned int volume_percent, bool muted)
{
    if (ui == NULL || (available && volume_percent > 100U))
        return;
    ui->volume_available = available;
    if (available) {
        ui->volume_percent = volume_percent;
        ui->muted = muted;
    }
}

void cp0_ui_set_connectivity_state(struct cp0_ui *ui, bool available,
                                   bool wifi_available, bool wifi_enabled,
                                   bool airplane_mode)
{
    if (ui == NULL || (!available && (wifi_available || wifi_enabled ||
                                      airplane_mode)) ||
        (!wifi_available && wifi_enabled) ||
        (airplane_mode && wifi_enabled))
        return;
    ui->connectivity_available = available;
    ui->wifi_available = wifi_available;
    ui->wifi_enabled = wifi_enabled;
    ui->airplane_mode = airplane_mode;
}

void cp0_ui_set_preferences(struct cp0_ui *ui, unsigned int theme,
                            unsigned int screen_timeout, bool key_sounds)
{
    if (ui == NULL || theme >= 3U || screen_timeout >= 4U)
        return;
    ui->theme = theme;
    ui->screen_timeout = screen_timeout;
    ui->key_sounds = key_sounds;
}

void cp0_ui_set_network_info(struct cp0_ui *ui,
                             const struct cp0_ui_network_info *info)
{
    if (ui == NULL || info == NULL)
        return;
    ui->network_available = info->available;
    ui->network_online = info->online;
    ui->network_link_up = info->link_up;
    copy_optional_text(ui->network_interface, sizeof(ui->network_interface),
                       info->interface_name);
    copy_optional_text(ui->network_ipv4, sizeof(ui->network_ipv4),
                       info->ipv4_address);
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

void cp0_ui_set_foreground_app(struct cp0_ui *ui, const char *app_name)
{
    if (ui == NULL)
        return;
    if (app_name == NULL) {
        ui->foreground_app_name[0] = '\0';
        return;
    }
    snprintf(ui->foreground_app_name, sizeof(ui->foreground_app_name), "%.13s",
             app_name);
}

void cp0_ui_set_device_settings(
    struct cp0_ui *ui, enum cp0_ui_authority authority,
    bool developer_mode, bool developer_mode_allowed, bool recovery_mode,
    bool recovery_mode_allowed, bool store_install_allowed,
    bool app_launch_restricted, uint8_t denied_permission_count)
{
    if (ui == NULL || authority > CP0_UI_AUTHORITY_ORGANIZATION ||
        denied_permission_count > 10)
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

void cp0_ui_set_developer_access(
    struct cp0_ui *ui, bool pairing_open,
    const struct cp0_ui_developer_host *hosts, size_t host_count)
{
    if (ui == NULL || host_count > CP0_UI_MAX_DEVELOPER_HOSTS ||
        (host_count > 0 && hosts == NULL))
        return;
    ui->developer_access_available = true;
    ui->developer_pairing_open = pairing_open;
    ui->developer_host_count = (unsigned int)host_count;
    for (size_t index = 0; index < host_count; index++) {
        if (hosts[index].label == NULL || hosts[index].ssh_fingerprint == NULL) {
            ui->developer_access_available = false;
            ui->developer_host_count = 0;
            return;
        }
        int label_length = snprintf(ui->developer_host_labels[index],
                                    sizeof(ui->developer_host_labels[index]),
                                    "%s", hosts[index].label);
        int fingerprint_length = snprintf(
            ui->developer_host_fingerprints[index],
            sizeof(ui->developer_host_fingerprints[index]), "%s",
            hosts[index].ssh_fingerprint);
        if (label_length <= 0 ||
            (size_t)label_length >= sizeof(ui->developer_host_labels[index]) ||
            fingerprint_length <= 0 ||
            (size_t)fingerprint_length >=
                sizeof(ui->developer_host_fingerprints[index])) {
            ui->developer_access_available = false;
            ui->developer_host_count = 0;
            return;
        }
    }
    for (size_t index = host_count; index < CP0_UI_MAX_DEVELOPER_HOSTS;
         index++) {
        ui->developer_host_labels[index][0] = '\0';
        ui->developer_host_fingerprints[index][0] = '\0';
    }
    if (ui->developer_host_selected > ui->developer_host_count)
        ui->developer_host_selected = ui->developer_host_count;
}

const char *cp0_ui_selected_developer_fingerprint(const struct cp0_ui *ui)
{
    if (ui == NULL || ui->developer_revoke_all ||
        ui->developer_host_selected >= ui->developer_host_count)
        return NULL;
    return ui->developer_host_fingerprints[ui->developer_host_selected];
}

void cp0_ui_set_auto_update(
    struct cp0_ui *ui, bool available, bool enabled, bool policy_allowed,
    bool charging, bool unmetered_network, bool due, bool checking)
{
    if (ui == NULL)
        return;
    ui->auto_update_available = available;
    ui->auto_update_enabled = available && enabled;
    ui->auto_update_policy_allowed = available && policy_allowed;
    ui->auto_update_charging = available && charging;
    ui->auto_update_unmetered_network = available && unmetered_network;
    ui->auto_update_due = available && enabled && due;
    ui->auto_update_checking = available && enabled && checking;
}

void cp0_ui_set_metrics(struct cp0_ui *ui, bool available, bool enabled,
                        bool policy_allowed, bool configured, bool pending)
{
    if (ui == NULL)
        return;
    ui->metrics_available = available;
    ui->metrics_policy_allowed = available && policy_allowed;
    ui->metrics_configured = available && configured;
    ui->metrics_enabled = available && enabled && policy_allowed && configured;
    ui->metrics_pending = ui->metrics_enabled && pending;
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
        copy_text(ui->apps[index].version,
                  sizeof(ui->apps[index].version), "UNKNOWN");
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
            .permissions = apps[source].permissions,
            .installed_at_unix_seconds = apps[source].installed_at_unix_seconds,
            .package_bytes = apps[source].package_bytes,
            .data_bytes = apps[source].data_bytes,
            .state = apps[source].running ? CP0_UI_APP_RUNNING
                                          : CP0_UI_APP_STOPPED,
        };
        if (!copy_text(app.app_id, sizeof(app.app_id), apps[source].app_id) ||
            !copy_text(app.name, sizeof(app.name), apps[source].name))
            continue;
        if (!copy_text(app.version, sizeof(app.version), apps[source].version))
            copy_text(app.version, sizeof(app.version), "UNKNOWN");
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
    if (ui->app_count == 0)
        ui->app_detail = false;
}

void cp0_ui_sync_task_catalog(struct cp0_ui *ui,
                              const struct cp0_ui_catalog_task *tasks,
                              size_t task_count)
{
    uint64_t selected_id = cp0_ui_selected_task_id(ui);

    if (ui == NULL || ui->tasks == NULL ||
        (tasks == NULL && task_count != 0))
        return;
    if (task_count > CP0_UI_MAX_TASKS)
        task_count = CP0_UI_MAX_TASKS;
    for (size_t source = 0; source < task_count; source++) {
        size_t found = source;
        while (found < ui->task_count &&
               ui->tasks[found].task_id != tasks[source].task_id)
            found++;
        if (found < ui->task_count) {
            if (found != source) {
                struct cp0_ui_task swap = ui->tasks[source];
                ui->tasks[source] = ui->tasks[found];
                ui->tasks[found] = swap;
            }
        } else {
            size_t old_count = ui->task_count;
            if (old_count < CP0_UI_MAX_TASKS)
                ui->task_count++;
            else
                old_count--;
            if (old_count > source) {
                memmove(&ui->tasks[source + 1], &ui->tasks[source],
                        (old_count - source) * sizeof(ui->tasks[0]));
            }
            memset(&ui->tasks[source], 0, sizeof(ui->tasks[source]));
        }
        struct cp0_ui_task *target = &ui->tasks[source];
        if (target->thumbnail_generation != tasks[source].thumbnail_generation) {
            target->thumbnail_available = false;
            target->thumbnail_pixels = NULL;
        }
        target->task_id = tasks[source].task_id;
        target->account_uid = tasks[source].account_uid;
        target->created_sequence = tasks[source].created_sequence;
        target->last_activated_sequence = tasks[source].last_activated_sequence;
        target->runtime_generation = tasks[source].runtime_generation;
        target->thumbnail_generation = tasks[source].thumbnail_generation;
        target->state = tasks[source].state;
        target->immersive = tasks[source].immersive;
        target->checkpoint_available = tasks[source].checkpoint_available;
        if (tasks[source].app_id == NULL || tasks[source].app_id[0] == '\0' ||
            tasks[source].name == NULL || tasks[source].name[0] == '\0' ||
            tasks[source].version == NULL ||
            tasks[source].version[0] == '\0') {
            memset(target, 0, sizeof(*target));
        } else {
            target->app_id = tasks[source].app_id;
            target->name = tasks[source].name;
            target->version = tasks[source].version;
        }
    }
    if (task_count < CP0_UI_MAX_TASKS) {
        memset(&ui->tasks[task_count], 0,
               (CP0_UI_MAX_TASKS - task_count) * sizeof(ui->tasks[0]));
    }
    ui->task_count = (unsigned int)task_count;
    ui->task_selected = 0;
    for (unsigned int index = 0; index < ui->task_count; index++) {
        if (ui->tasks[index].task_id == selected_id) {
            ui->task_selected = index;
            break;
        }
    }
}

void cp0_ui_set_task_thumbnail(struct cp0_ui *ui, uint64_t task_id,
                               uint64_t generation, const uint16_t *pixels,
                               size_t pixel_count)
{
    if (ui == NULL || task_id == 0 || generation == 0 || pixels == NULL ||
        pixel_count != CP0_UI_TASK_THUMBNAIL_PIXELS)
        return;
    for (unsigned int index = 0; index < ui->task_count; index++) {
        struct cp0_ui_task *task = &ui->tasks[index];
        if (task->task_id != task_id)
            continue;
        task->thumbnail_pixels = pixels;
        task->thumbnail_generation = generation;
        task->thumbnail_available = true;
        return;
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

struct semver_slice {
    const char *text;
    size_t length;
};

struct semver_view {
    struct semver_slice core[3];
    struct semver_slice prerelease;
};

static bool parse_semver_view(const char *version, struct semver_view *view)
{
    const char *logical_end;
    const char *prerelease;
    const char *cursor;

    if (version == NULL || view == NULL || version[0] == '\0')
        return false;
    logical_end = strchr(version, '+');
    if (logical_end == NULL)
        logical_end = version + strlen(version);
    prerelease = memchr(version, '-', (size_t)(logical_end - version));
    const char *core_end = prerelease == NULL ? logical_end : prerelease;
    cursor = version;
    for (unsigned int index = 0; index < 3; index++) {
        const char *end = index == 2
                              ? core_end
                              : memchr(cursor, '.',
                                       (size_t)(core_end - cursor));
        if (end == NULL || end == cursor)
            return false;
        view->core[index] = (struct semver_slice){
            .text = cursor,
            .length = (size_t)(end - cursor),
        };
        cursor = end + (index == 2 ? 0 : 1);
    }
    if (cursor != core_end)
        return false;
    view->prerelease = (struct semver_slice){0};
    if (prerelease != NULL) {
        if (prerelease + 1 == logical_end)
            return false;
        view->prerelease.text = prerelease + 1;
        view->prerelease.length =
            (size_t)(logical_end - view->prerelease.text);
    }
    return true;
}

static bool slice_numeric(struct semver_slice value)
{
    if (value.length == 0)
        return false;
    for (size_t index = 0; index < value.length; index++) {
        if (value.text[index] < '0' || value.text[index] > '9')
            return false;
    }
    return true;
}

static int compare_numeric_slice(struct semver_slice left,
                                 struct semver_slice right)
{
    while (left.length > 1 && left.text[0] == '0')
        left.text++, left.length--;
    while (right.length > 1 && right.text[0] == '0')
        right.text++, right.length--;
    if (left.length != right.length)
        return left.length > right.length ? 1 : -1;
    int compared = memcmp(left.text, right.text, left.length);
    return compared > 0 ? 1 : (compared < 0 ? -1 : 0);
}

static int compare_identifier(struct semver_slice left,
                              struct semver_slice right)
{
    bool left_numeric = slice_numeric(left);
    bool right_numeric = slice_numeric(right);
    if (left_numeric && right_numeric)
        return compare_numeric_slice(left, right);
    if (left_numeric != right_numeric)
        return left_numeric ? -1 : 1;
    size_t shared = left.length < right.length ? left.length : right.length;
    int compared = memcmp(left.text, right.text, shared);
    if (compared != 0)
        return compared > 0 ? 1 : -1;
    return left.length > right.length ? 1
                                      : (left.length < right.length ? -1 : 0);
}

static struct semver_slice take_identifier(struct semver_slice *remaining)
{
    const char *separator = memchr(remaining->text, '.', remaining->length);
    size_t length = separator == NULL
                        ? remaining->length
                        : (size_t)(separator - remaining->text);
    struct semver_slice result = {.text = remaining->text, .length = length};
    size_t consumed = length + (separator == NULL ? 0U : 1U);
    remaining->text += consumed;
    remaining->length -= consumed;
    return result;
}

static bool store_version_is_newer(const char *candidate,
                                   const char *installed)
{
    struct semver_view left;
    struct semver_view right;
    if (!parse_semver_view(candidate, &left) ||
        !parse_semver_view(installed, &right))
        return false;
    for (unsigned int index = 0; index < 3; index++) {
        int compared = compare_numeric_slice(left.core[index],
                                             right.core[index]);
        if (compared != 0)
            return compared > 0;
    }
    if (left.prerelease.length == 0 || right.prerelease.length == 0)
        return left.prerelease.length == 0 && right.prerelease.length != 0;
    while (left.prerelease.length > 0 && right.prerelease.length > 0) {
        int compared = compare_identifier(take_identifier(&left.prerelease),
                                          take_identifier(&right.prerelease));
        if (compared != 0)
            return compared > 0;
    }
    return left.prerelease.length > 0;
}

static bool copy_store_app(struct cp0_ui_store_app *app,
                           const struct cp0_ui_store_catalog_app *source)
{
    if (source->state > CP0_UI_STORE_FAILED ||
        source->failure_reason > CP0_UI_STORE_FAILURE_INTERNAL ||
        source->progress_percent > 100 ||
        ((source->state == CP0_UI_STORE_FAILED) !=
         (source->failure_reason != CP0_UI_STORE_FAILURE_NONE)))
        return false;
    *app = (struct cp0_ui_store_app){
        .package_bytes = source->package_bytes,
        .permissions = source->permissions,
        .installed_permissions = source->installed_permissions,
        .progress_percent = source->progress_percent,
        .state = source->state,
        .operation_state = source->state,
        .failure_reason = source->failure_reason,
    };
    if (!copy_text(app->app_id, sizeof(app->app_id), source->app_id) ||
        !copy_text(app->name, sizeof(app->name), source->name) ||
        !copy_text(app->version, sizeof(app->version), source->version) ||
        !copy_text(app->summary, sizeof(app->summary), source->summary))
        return false;
    app->update_available = source->installed_version != NULL &&
                            store_version_is_newer(app->version,
                                                   source->installed_version);
    if (app->state == CP0_UI_STORE_AVAILABLE ||
        app->state == CP0_UI_STORE_INSTALLED) {
        if (source->installed_version == NULL) {
            app->state = CP0_UI_STORE_AVAILABLE;
        } else if (strcmp(source->installed_version, app->version) == 0) {
            app->state = CP0_UI_STORE_INSTALLED;
        } else if (app->update_available) {
            app->state = CP0_UI_STORE_UPDATE;
        } else {
            app->state = CP0_UI_STORE_INSTALLED;
        }
    }
    return true;
}

static void record_store_completions(
    struct cp0_ui *ui, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count)
{
    if (!ui->store_catalog_observed)
        return;
    for (size_t source_index = 0; source_index < app_count; source_index++) {
        struct cp0_ui_store_app current;
        if (!copy_store_app(&current, &apps[source_index]) ||
            current.operation_state != CP0_UI_STORE_INSTALLED)
            continue;
        for (unsigned int previous = 0; previous < ui->store_count; previous++) {
            const struct cp0_ui_store_app *old = &ui->store_apps[previous];
            if (strcmp(old->app_id, current.app_id) != 0 ||
                strcmp(old->version, current.version) != 0)
                continue;
            if (old->operation_state != CP0_UI_STORE_INSTALLED &&
                ui->store_completion_count < CP0_UI_MAX_APPS) {
                if (ui->store_completion_count == 0) {
                    memcpy(ui->store_completion_app_name, current.name,
                           sizeof(ui->store_completion_app_name));
                    memcpy(ui->store_completion_version, current.version,
                           sizeof(ui->store_completion_version));
                }
                ui->store_completion_count++;
            }
            break;
        }
    }
}

static void reconcile_store_detail_identity(struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app;
    if (ui == NULL || !ui->store_detail)
        return;
    app = selected_store_app(ui);
    if (app == NULL || strcmp(ui->store_detail_app_id, app->app_id) != 0 ||
        strcmp(ui->store_detail_version, app->version) != 0) {
        ui->store_detail = false;
    } else if (!store_operation_has_cancel(app->state)) {
        ui->store_operation_action_selected = 0;
    }
}

void cp0_ui_set_store_status(struct cp0_ui *ui,
                             enum cp0_ui_store_status status)
{
    if (ui == NULL || status > CP0_UI_STORE_UNAVAILABLE)
        return;
    ui->store_status = status;
    if (status != CP0_UI_STORE_READY) {
        ui->store_detail = false;
        ui->store_update_all_selected = false;
        ui->store_browse_status = status;
        ui->store_browse_count = 0;
        ui->store_browse_selected = 0;
    }
}

void cp0_ui_sync_store_catalog(
    struct cp0_ui *ui, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count, bool truncated, bool stale)
{
    char selected_id[CP0_UI_APP_ID_MAX + 1] = {0};
    const struct cp0_ui_store_app *selected_app;
    bool update_all_selected;

    if (ui == NULL || (apps == NULL && app_count > 0))
        return;
    if (app_count > CP0_UI_MAX_APPS)
        app_count = CP0_UI_MAX_APPS;
    record_store_completions(ui, apps, app_count);
    selected_app = selected_store_app(ui);
    update_all_selected = ui->store_update_all_selected;
    if (selected_app != NULL)
        copy_optional_text(selected_id, sizeof(selected_id),
                           selected_app->app_id);
    memset(ui->store_apps, 0, sizeof(ui->store_apps));
    ui->store_count = 0;
    for (size_t index = 0; index < app_count; index++) {
        struct cp0_ui_store_app app;
        if (!copy_store_app(&app, &apps[index]))
            continue;
        ui->store_apps[ui->store_count++] = app;
    }
    ui->store_status = CP0_UI_STORE_READY;
    ui->store_list_truncated = truncated;
    ui->store_catalog_stale = stale;
    ui->store_catalog_observed = true;
    ui->store_selected = 0;
    if (ui->store_section == CP0_UI_STORE_UPDATES) {
        unsigned int update_index = 0;
        for (unsigned int index = 0; index < ui->store_count; index++) {
            if (!store_update_state(&ui->store_apps[index]))
                continue;
            if (strcmp(ui->store_apps[index].app_id, selected_id) == 0) {
                ui->store_selected = update_index;
                break;
            }
            update_index++;
        }
    } else {
        for (unsigned int index = 0; index < ui->store_count; index++) {
            if (strcmp(ui->store_apps[index].app_id, selected_id) == 0) {
                ui->store_selected = index;
                break;
            }
        }
    }
    ui->store_update_all_selected =
        ui->store_section == CP0_UI_STORE_UPDATES && update_all_selected &&
        store_update_batch_count(ui) > 0;
    if (ui->store_count == 0)
        ui->store_detail = false;
    sync_store_activity(ui);
    reconcile_store_detail_identity(ui);
}

void cp0_ui_sync_store_today(
    struct cp0_ui *ui, const struct cp0_ui_store_editorial *editorial)
{
    struct cp0_ui_store_app featured = {0};
    struct cp0_ui_store_editorial_collection_state collections
        [CP0_UI_STORE_EDITORIAL_COLLECTION_MAX] = {0};
    char selected_collection_title[CP0_UI_STORE_EDITORIAL_TITLE_MAX + 1] = {0};
    char selected_app_id[CP0_UI_APP_ID_MAX + 1] = {0};
    bool collection_open = false;

    if (ui == NULL)
        return;
    if (ui->store_today_available) {
        if (ui->store_today_collection_open &&
            ui->store_today_open_collection <
                ui->store_today_collection_count) {
            const struct cp0_ui_store_editorial_collection_state *collection =
                &ui->store_today_collections[ui->store_today_open_collection];
            collection_open = true;
            copy_optional_text(selected_collection_title,
                               sizeof(selected_collection_title),
                               collection->title);
            if (ui->store_today_collection_selected < collection->app_count)
                copy_optional_text(
                    selected_app_id, sizeof(selected_app_id),
                    collection->apps[ui->store_today_collection_selected]
                        .app_id);
        } else if (ui->store_today_selected > 0 &&
                   ui->store_today_selected <=
                       ui->store_today_collection_count) {
            copy_optional_text(
                selected_collection_title,
                sizeof(selected_collection_title),
                ui->store_today_collections[ui->store_today_selected - 1U]
                    .title);
        }
    }
    if (editorial == NULL || editorial->headline == NULL ||
        editorial->featured == NULL || editorial->collections == NULL ||
        editorial->collection_count == 0 ||
        editorial->collection_count > CP0_UI_STORE_EDITORIAL_COLLECTION_MAX ||
        !copy_text(ui->store_today_headline,
                   sizeof(ui->store_today_headline), editorial->headline) ||
        !copy_store_app(&featured, editorial->featured)) {
        ui->store_today_available = false;
        ui->store_today_collection_open = false;
        ui->store_today_collection_count = 0;
        ui->store_today_selected = 0;
        ui->store_today_collection_selected = 0;
        ui->store_today_open_collection = 0;
        memset(ui->store_today_headline, 0,
               sizeof(ui->store_today_headline));
        memset(&ui->store_today_featured, 0,
               sizeof(ui->store_today_featured));
        memset(ui->store_today_collections, 0,
               sizeof(ui->store_today_collections));
        reconcile_store_detail_identity(ui);
        return;
    }
    for (size_t index = 0; index < editorial->collection_count; index++) {
        const struct cp0_ui_store_editorial_collection *source =
            &editorial->collections[index];
        if (source->title == NULL || source->apps == NULL ||
            source->app_count == 0 ||
            source->app_count > CP0_UI_STORE_EDITORIAL_COLLECTION_APP_MAX ||
            !copy_text(collections[index].title,
                       sizeof(collections[index].title), source->title)) {
            cp0_ui_sync_store_today(ui, NULL);
            return;
        }
        collections[index].app_count = source->app_count;
        for (size_t app = 0; app < source->app_count; app++) {
            if (!copy_store_app(&collections[index].apps[app],
                                &source->apps[app])) {
                cp0_ui_sync_store_today(ui, NULL);
                return;
            }
        }
    }
    ui->store_today_featured = featured;
    memcpy(ui->store_today_collections, collections, sizeof(collections));
    ui->store_today_collection_count = editorial->collection_count;
    ui->store_today_available = true;
    ui->store_today_collection_open = false;
    ui->store_today_selected = 0;
    ui->store_today_collection_selected = 0;
    ui->store_today_open_collection = 0;
    if (selected_collection_title[0] != '\0') {
        for (size_t collection = 0;
             collection < ui->store_today_collection_count; collection++) {
            if (strcmp(ui->store_today_collections[collection].title,
                       selected_collection_title) != 0)
                continue;
            if (!collection_open) {
                ui->store_today_selected = (unsigned int)collection + 1U;
                break;
            }
            ui->store_today_open_collection = (unsigned int)collection;
            ui->store_today_collection_open = true;
            if (selected_app_id[0] != '\0') {
                for (size_t app = 0;
                     app < ui->store_today_collections[collection].app_count;
                     app++) {
                    if (strcmp(ui->store_today_collections[collection]
                                   .apps[app]
                                   .app_id,
                               selected_app_id) == 0) {
                        ui->store_today_collection_selected =
                            (unsigned int)app;
                        break;
                    }
                }
            }
            break;
        }
    }
    reconcile_store_detail_identity(ui);
}

void cp0_ui_set_store_search_status(struct cp0_ui *ui,
                                    enum cp0_ui_store_status status)
{
    if (ui == NULL || status > CP0_UI_STORE_UNAVAILABLE)
        return;
    ui->store_search_status = status;
    if (status != CP0_UI_STORE_READY) {
        ui->store_search_count = 0;
        ui->store_search_selected = 0;
        ui->store_detail = false;
    }
}

void cp0_ui_set_store_browse_status(struct cp0_ui *ui,
                                    enum cp0_ui_store_status status)
{
    if (ui == NULL || status > CP0_UI_STORE_UNAVAILABLE)
        return;
    ui->store_browse_status = status;
    if (status != CP0_UI_STORE_READY) {
        ui->store_browse_count = 0;
        ui->store_browse_selected = 0;
        ui->store_detail = false;
    }
}

void cp0_ui_sync_store_browse(
    struct cp0_ui *ui, uint16_t offset, uint16_t total, bool has_next,
    uint16_t next_offset, const struct cp0_ui_store_catalog_app *apps,
    size_t app_count, bool stale)
{
    char selected_id[CP0_UI_APP_ID_MAX + 1] = {0};
    uint16_t expected_next = (uint16_t)(offset + app_count);

    if (ui == NULL || (apps == NULL && app_count > 0) ||
        app_count > CP0_UI_STORE_SEARCH_PAGE_MAX ||
        total > CP0_UI_STORE_CATALOG_MAX ||
        offset != ui->store_browse_offset || offset > total ||
        app_count != (size_t)((total - offset) < CP0_UI_STORE_SEARCH_PAGE_MAX
                                  ? (total - offset)
                                  : CP0_UI_STORE_SEARCH_PAGE_MAX) ||
        has_next != (expected_next < total) ||
        (has_next && next_offset != expected_next))
        return;
    if (ui->store_browse_selected < ui->store_browse_count)
        copy_optional_text(selected_id, sizeof(selected_id),
                           ui->store_page_apps[ui->store_browse_selected]
                               .app_id);
    memset(ui->store_page_apps, 0, sizeof(ui->store_page_apps));
    ui->store_browse_count = 0;
    ui->store_search_count = 0;
    for (size_t index = 0; index < app_count; index++) {
        struct cp0_ui_store_app app;
        if (!copy_store_app(&app, &apps[index]))
            return;
        ui->store_page_apps[ui->store_browse_count++] = app;
    }
    ui->store_browse_total = total;
    ui->store_browse_has_next = has_next;
    ui->store_browse_next_offset = has_next ? next_offset : 0;
    ui->store_browse_status = CP0_UI_STORE_READY;
    ui->store_browse_stale = stale;
    ui->store_browse_selected = 0;
    for (unsigned int index = 0; index < ui->store_browse_count; index++) {
        if (strcmp(ui->store_page_apps[index].app_id, selected_id) == 0) {
            ui->store_browse_selected = index;
            break;
        }
    }
    reconcile_store_detail_identity(ui);
}

void cp0_ui_sync_store_search(
    struct cp0_ui *ui, const char *query, uint16_t offset, uint16_t total,
    bool has_next, uint16_t next_offset,
    const struct cp0_ui_store_catalog_app *apps, size_t app_count, bool stale)
{
    char selected_id[CP0_UI_APP_ID_MAX + 1] = {0};
    uint16_t expected_next = (uint16_t)(offset + app_count);

    if (ui == NULL || query == NULL || strcmp(query, ui->store_search_query) != 0 ||
        (apps == NULL && app_count > 0) ||
        app_count > CP0_UI_STORE_SEARCH_PAGE_MAX ||
        total > CP0_UI_STORE_CATALOG_MAX ||
        offset != ui->store_search_offset || offset > total ||
        app_count != (size_t)((total - offset) < CP0_UI_STORE_SEARCH_PAGE_MAX
                                  ? (total - offset)
                                  : CP0_UI_STORE_SEARCH_PAGE_MAX) ||
        has_next != (expected_next < total) ||
        (has_next && next_offset != expected_next))
        return;
    if (ui->store_search_selected < ui->store_search_count)
        copy_optional_text(selected_id, sizeof(selected_id),
                           ui->store_page_apps[ui->store_search_selected]
                               .app_id);
    memset(ui->store_page_apps, 0, sizeof(ui->store_page_apps));
    ui->store_browse_count = 0;
    ui->store_search_count = 0;
    for (size_t index = 0; index < app_count; index++) {
        struct cp0_ui_store_app app;
        if (!copy_store_app(&app, &apps[index]))
            return;
        ui->store_page_apps[ui->store_search_count++] = app;
    }
    ui->store_search_total = total;
    ui->store_search_has_next = has_next;
    ui->store_search_next_offset = has_next ? next_offset : 0;
    ui->store_search_status = CP0_UI_STORE_READY;
    ui->store_search_stale = stale;
    ui->store_search_selected = 0;
    for (unsigned int index = 0; index < ui->store_search_count; index++) {
        if (strcmp(ui->store_page_apps[index].app_id, selected_id) == 0) {
            ui->store_search_selected = index;
            break;
        }
    }
    reconcile_store_detail_identity(ui);
}

void cp0_ui_set_store_app_state(struct cp0_ui *ui, const char *app_id,
                                enum cp0_ui_store_state state,
                                uint8_t progress_percent)
{
    int index = store_app_index_by_id(ui, app_id);
    if (ui == NULL || app_id == NULL || state > CP0_UI_STORE_FAILED ||
        progress_percent > 100)
        return;
    if (ui->store_detail && strcmp(ui->store_detail_app_id, app_id) == 0 &&
        !store_operation_has_cancel(state))
        ui->store_operation_action_selected = 0;
    if (index >= 0) {
        ui->store_apps[index].state = state;
        ui->store_apps[index].operation_state = state;
        ui->store_apps[index].progress_percent = progress_percent;
        ui->store_apps[index].failure_reason =
            state == CP0_UI_STORE_FAILED ? CP0_UI_STORE_FAILURE_INTERNAL
                                         : CP0_UI_STORE_FAILURE_NONE;
    }
    unsigned int page_count = ui->store_section == CP0_UI_STORE_APPS
                                  ? ui->store_browse_count
                                  : ui->store_search_count;
    for (unsigned int page = 0; page < page_count; page++) {
        if (strcmp(ui->store_page_apps[page].app_id, app_id) == 0) {
            ui->store_page_apps[page].state = state;
            ui->store_page_apps[page].operation_state = state;
            ui->store_page_apps[page].progress_percent = progress_percent;
            ui->store_page_apps[page].failure_reason =
                state == CP0_UI_STORE_FAILED ? CP0_UI_STORE_FAILURE_INTERNAL
                                             : CP0_UI_STORE_FAILURE_NONE;
        }
    }
    if (ui->store_today_available) {
        struct cp0_ui_store_app *today_apps
            [1U + CP0_UI_STORE_EDITORIAL_COLLECTION_MAX *
                      CP0_UI_STORE_EDITORIAL_COLLECTION_APP_MAX];
        size_t today_count = 0;
        today_apps[today_count++] = &ui->store_today_featured;
        for (size_t collection = 0;
             collection < ui->store_today_collection_count; collection++) {
            for (size_t app = 0;
                 app < ui->store_today_collections[collection].app_count; app++)
                today_apps[today_count++] =
                    &ui->store_today_collections[collection].apps[app];
        }
        for (size_t app = 0; app < today_count; app++) {
            if (strcmp(today_apps[app]->app_id, app_id) != 0)
                continue;
            today_apps[app]->state = state;
            today_apps[app]->operation_state = state;
            today_apps[app]->progress_percent = progress_percent;
            today_apps[app]->failure_reason =
                state == CP0_UI_STORE_FAILED ? CP0_UI_STORE_FAILURE_INTERNAL
                                             : CP0_UI_STORE_FAILURE_NONE;
        }
    }
    if (store_update_batch_count(ui) == 0)
        ui->store_update_all_selected = false;
    sync_store_activity(ui);
}

size_t cp0_ui_collect_store_update_batch(const struct cp0_ui *ui,
                                         const char *app_ids[],
                                         size_t app_capacity)
{
    size_t count = 0;
    size_t limit = app_capacity < CP0_UI_STORE_UPDATE_BATCH_MAX
                       ? app_capacity
                       : CP0_UI_STORE_UPDATE_BATCH_MAX;

    if (ui == NULL || app_ids == NULL || limit == 0 ||
        ui->screen != CP0_UI_STORE ||
        ui->store_section != CP0_UI_STORE_UPDATES ||
        ui->store_status != CP0_UI_STORE_READY || ui->store_catalog_stale ||
        !ui->store_update_all_selected)
        return 0;
    for (unsigned int index = 0; index < ui->store_count && count < limit;
         index++) {
        if (store_update_batch_eligible(&ui->store_apps[index]))
            app_ids[count++] = ui->store_apps[index].app_id;
    }
    for (size_t index = 1; index < count; index++) {
        const char *app_id = app_ids[index];
        size_t position = index;
        while (position > 0 && strcmp(app_ids[position - 1], app_id) > 0) {
            app_ids[position] = app_ids[position - 1];
            position--;
        }
        app_ids[position] = app_id;
    }
    return count;
}

bool cp0_ui_take_store_completion(
    struct cp0_ui *ui, struct cp0_ui_store_completion *completion)
{
    if (ui == NULL || completion == NULL || ui->store_completion_count == 0)
        return false;
    memset(completion, 0, sizeof(*completion));
    completion->count = ui->store_completion_count;
    memcpy(completion->app_name, ui->store_completion_app_name,
           sizeof(completion->app_name));
    memcpy(completion->version, ui->store_completion_version,
           sizeof(completion->version));
    ui->store_completion_count = 0;
    memset(ui->store_completion_app_name, 0,
           sizeof(ui->store_completion_app_name));
    memset(ui->store_completion_version, 0,
           sizeof(ui->store_completion_version));
    return true;
}

void cp0_ui_show_store_install_prompt(
    struct cp0_ui *ui, uint8_t app_count, uint8_t new_permissions,
    uint8_t denied_permissions, uint64_t required_bytes,
    uint64_t available_bytes)
{
    if (ui == NULL || app_count == 0 ||
        app_count > CP0_UI_STORE_UPDATE_BATCH_MAX || required_bytes == 0 ||
        available_bytes < required_bytes)
        return;
    ui->store_install_prompt = true;
    ui->store_preflight_error = CP0_UI_STORE_PREFLIGHT_NONE;
    ui->store_preflight_app_count = app_count;
    ui->store_preflight_new_permissions = new_permissions;
    ui->store_preflight_denied_permissions = denied_permissions;
    ui->store_preflight_required_bytes = required_bytes;
    ui->store_preflight_available_bytes = available_bytes;
    ui->dialog_selected = 1;
}

void cp0_ui_show_store_preflight_error(
    struct cp0_ui *ui, enum cp0_ui_store_preflight_error error)
{
    if (ui == NULL || error <= CP0_UI_STORE_PREFLIGHT_NONE ||
        error > CP0_UI_STORE_PREFLIGHT_UNAVAILABLE)
        return;
    ui->store_install_prompt = true;
    ui->store_preflight_error = error;
    ui->store_preflight_app_count = 0;
    ui->store_preflight_new_permissions = 0;
    ui->store_preflight_denied_permissions = 0;
    ui->store_preflight_required_bytes = 0;
    ui->store_preflight_available_bytes = 0;
    ui->dialog_selected = 0;
}

const char *cp0_ui_selected_store_app_id(const struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app;
    if (ui == NULL || ui->screen != CP0_UI_STORE ||
        (app = selected_store_app(ui)) == NULL)
        return NULL;
    return app->app_id;
}

enum cp0_ui_store_state cp0_ui_selected_store_app_state(
    const struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app =
        ui == NULL ? NULL : selected_store_app(ui);
    return app == NULL ? CP0_UI_STORE_AVAILABLE : app->state;
}

const char *cp0_ui_selected_store_app_version(const struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app =
        ui == NULL ? NULL : selected_store_app(ui);
    return app == NULL ? NULL : app->version;
}

uint8_t cp0_ui_selected_store_screenshot(const struct cp0_ui *ui)
{
    return ui == NULL ? 0 : (uint8_t)ui->store_screenshot_index;
}

static bool store_detail_identity_matches(const struct cp0_ui *ui,
                                          const char *app_id,
                                          const char *version)
{
    return ui != NULL && ui->store_detail && app_id != NULL &&
           version != NULL && strcmp(ui->store_detail_app_id, app_id) == 0 &&
           strcmp(ui->store_detail_version, version) == 0;
}

static void begin_store_detail(struct cp0_ui *ui)
{
    const struct cp0_ui_store_app *app = selected_store_app(ui);
    if (app == NULL)
        return;
    ui->store_detail = true;
    ui->store_detail_page = 0;
    ui->store_operation_action_selected = 0;
    ui->store_detail_text_offset = 0;
    ui->store_screenshot_index = 0;
    ui->store_screenshot_count = 0;
    ui->store_detail_status = CP0_UI_STORE_DETAIL_LOADING;
    ui->store_icon_available = false;
    ui->store_screenshot_available = false;
    ui->store_screenshot_loading = false;
    ui->store_icon_pixels = NULL;
    ui->store_screenshot_pixels = NULL;
    memset(ui->store_developer, 0, sizeof(ui->store_developer));
    memset(ui->store_category, 0, sizeof(ui->store_category));
    memset(ui->store_age_rating, 0, sizeof(ui->store_age_rating));
    memset(ui->store_description, 0, sizeof(ui->store_description));
    memset(ui->store_release_notes, 0, sizeof(ui->store_release_notes));
    copy_optional_text(ui->store_detail_app_id,
                       sizeof(ui->store_detail_app_id), app->app_id);
    copy_optional_text(ui->store_detail_version,
                       sizeof(ui->store_detail_version), app->version);
}

void cp0_ui_set_store_details(
    struct cp0_ui *ui, const char *app_id, const char *version,
    const char *developer, const char *category, const char *age_rating,
    const char *description, const char *release_notes,
    uint8_t screenshot_count)
{
    if (!store_detail_identity_matches(ui, app_id, version) ||
        screenshot_count == 0 || screenshot_count > 5 ||
        !copy_text(ui->store_developer, sizeof(ui->store_developer),
                   developer) ||
        !copy_text(ui->store_category, sizeof(ui->store_category), category) ||
        !copy_text(ui->store_age_rating, sizeof(ui->store_age_rating),
                   age_rating) ||
        !copy_text(ui->store_description, sizeof(ui->store_description),
                   description) ||
        !copy_text(ui->store_release_notes, sizeof(ui->store_release_notes),
                   release_notes))
        return;
    ui->store_screenshot_count = screenshot_count;
    ui->store_detail_status = CP0_UI_STORE_DETAIL_READY;
}

void cp0_ui_set_store_details_unavailable(struct cp0_ui *ui,
                                          const char *app_id,
                                          const char *version)
{
    if (!store_detail_identity_matches(ui, app_id, version))
        return;
    ui->store_detail_status = CP0_UI_STORE_DETAIL_UNAVAILABLE;
    ui->store_screenshot_count = 0;
}

void cp0_ui_set_store_icon(struct cp0_ui *ui, const char *app_id,
                           const char *version, const uint32_t *pixels,
                           uint16_t width, uint16_t height)
{
    if (!store_detail_identity_matches(ui, app_id, version) || pixels == NULL ||
        !((width == 32 && height == 32) || (width == 48 && height == 48)))
        return;
    ui->store_icon_pixels = pixels;
    ui->store_icon_width = width;
    ui->store_icon_height = height;
    ui->store_icon_available = true;
}

void cp0_ui_set_store_screenshot(struct cp0_ui *ui, const char *app_id,
                                 const char *version, uint8_t index,
                                 const uint32_t *pixels, uint16_t width,
                                 uint16_t height)
{
    if (!store_detail_identity_matches(ui, app_id, version) || pixels == NULL ||
        index != ui->store_screenshot_index || width != 320 || height != 170)
        return;
    ui->store_screenshot_pixels = pixels;
    ui->store_screenshot_available = true;
    ui->store_screenshot_loading = false;
}

void cp0_ui_set_store_screenshot_unavailable(struct cp0_ui *ui,
                                             const char *app_id,
                                             const char *version,
                                             uint8_t index)
{
    if (!store_detail_identity_matches(ui, app_id, version) ||
        index != ui->store_screenshot_index)
        return;
    ui->store_screenshot_available = false;
    ui->store_screenshot_loading = false;
    ui->store_screenshot_pixels = NULL;
}

static bool store_ascii_character(char character)
{
    return character >= ' ' && character <= '~';
}

bool cp0_ui_store_accepts_text(const struct cp0_ui *ui)
{
    return ui != NULL && ui->screen == CP0_UI_STORE && !ui->store_detail &&
           ui->store_section == CP0_UI_STORE_SEARCH && ui->store_search_input;
}

static void reset_store_search_results(struct cp0_ui *ui)
{
    ui->store_search_offset = 0;
    ui->store_search_total = 0;
    ui->store_search_next_offset = 0;
    ui->store_search_has_next = false;
    ui->store_search_selected = 0;
    ui->store_search_count = 0;
    ui->store_search_status = CP0_UI_STORE_LOADING;
}

enum cp0_ui_event cp0_ui_store_input_ascii(struct cp0_ui *ui, char character)
{
    size_t length;
    if (!cp0_ui_store_accepts_text(ui) || !store_ascii_character(character))
        return CP0_UI_EVENT_NONE;
    length = strlen(ui->store_search_query);
    if (length >= 32U || (character == ' ' && length == 0) ||
        (character == ' ' && ui->store_search_query[length - 1U] == ' '))
        return CP0_UI_EVENT_NONE;
    ui->store_search_query[length] = character;
    ui->store_search_query[length + 1U] = '\0';
    reset_store_search_results(ui);
    return character == ' ' ? CP0_UI_EVENT_NONE : CP0_UI_EVENT_STORE_SEARCH;
}

enum cp0_ui_event cp0_ui_store_backspace(struct cp0_ui *ui)
{
    size_t length;
    if (!cp0_ui_store_accepts_text(ui) ||
        (length = strlen(ui->store_search_query)) == 0)
        return CP0_UI_EVENT_NONE;
    ui->store_search_query[length - 1U] = '\0';
    reset_store_search_results(ui);
    length--;
    return length > 0 && ui->store_search_query[length - 1U] != ' '
               ? CP0_UI_EVENT_STORE_SEARCH
               : CP0_UI_EVENT_NONE;
}

const char *cp0_ui_store_search_query(const struct cp0_ui *ui)
{
    return ui == NULL ? NULL : ui->store_search_query;
}

uint16_t cp0_ui_store_search_offset(const struct cp0_ui *ui)
{
    return ui == NULL ? 0 : ui->store_search_offset;
}

uint16_t cp0_ui_store_browse_offset(const struct cp0_ui *ui)
{
    return ui == NULL ? 0 : ui->store_browse_offset;
}

static void remember_store_query(struct cp0_ui *ui)
{
    if (ui->store_search_query[0] == '\0')
        return;
    unsigned int existing = ui->store_recent_count;
    for (unsigned int index = 0; index < ui->store_recent_count; index++) {
        if (strcmp(ui->store_recent_queries[index],
                   ui->store_search_query) == 0) {
            existing = index;
            break;
        }
    }
    if (existing < ui->store_recent_count && existing > 0) {
        memmove(&ui->store_recent_queries[1], &ui->store_recent_queries[0],
                existing * sizeof(ui->store_recent_queries[0]));
    } else if (existing == ui->store_recent_count) {
        unsigned int move = ui->store_recent_count;
        if (move >= CP0_UI_STORE_RECENT_MAX)
            move = CP0_UI_STORE_RECENT_MAX - 1U;
        if (move > 0)
            memmove(&ui->store_recent_queries[1],
                    &ui->store_recent_queries[0],
                    move * sizeof(ui->store_recent_queries[0]));
        if (ui->store_recent_count < CP0_UI_STORE_RECENT_MAX)
            ui->store_recent_count++;
    }
    copy_optional_text(ui->store_recent_queries[0],
                       sizeof(ui->store_recent_queries[0]),
                       ui->store_search_query);
    ui->store_recent_selected = 0;
}

static int selected_app_index(const struct cp0_ui *ui)
{
    if (ui == NULL)
        return -1;
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

uint64_t cp0_ui_selected_task_id(const struct cp0_ui *ui)
{
    return ui != NULL && ui->task_selected < ui->task_count
               ? ui->tasks[ui->task_selected].task_id
               : 0;
}

uint64_t cp0_ui_selected_task_runtime_generation(const struct cp0_ui *ui)
{
    return ui != NULL && ui->task_selected < ui->task_count
               ? ui->tasks[ui->task_selected].runtime_generation
               : 0;
}

uint32_t cp0_ui_selected_task_account_uid(const struct cp0_ui *ui)
{
    return ui != NULL && ui->task_selected < ui->task_count
               ? ui->tasks[ui->task_selected].account_uid
               : 0;
}

const char *cp0_ui_selected_task_app_id(const struct cp0_ui *ui)
{
    return ui != NULL && ui->task_selected < ui->task_count
               ? ui->tasks[ui->task_selected].app_id
               : NULL;
}

bool cp0_ui_selected_task_is_immersive(const struct cp0_ui *ui)
{
    return ui != NULL && ui->task_selected < ui->task_count &&
           ui->tasks[ui->task_selected].immersive;
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

void cp0_ui_set_local_simulation(struct cp0_ui *ui, bool enabled)
{
    if (ui != NULL)
        ui->local_simulation = enabled;
}

void cp0_ui_set_screenshot_status(struct cp0_ui *ui,
                                  enum cp0_ui_screenshot_status status)
{
    if (ui == NULL || status > CP0_UI_SCREENSHOT_BUSY)
        return;
    ui->screenshot_status = status;
    ui->system_action_kind = 6;
    ui->system_action_overlay = true;
    ui->system_action_ticks = 2;
}

void cp0_ui_set_media_status(struct cp0_ui *ui,
                             enum cp0_ui_media_status status)
{
    if (ui == NULL || status > CP0_UI_MEDIA_FAILED)
        return;
    ui->media_status = status;
    ui->system_action_overlay = true;
    ui->system_action_ticks = 2;
}

bool cp0_ui_tick(struct cp0_ui *ui)
{
    if (ui == NULL || ui->system_action_ticks == 0)
        return false;
    ui->system_action_ticks--;
    if (ui->system_action_ticks == 0) {
        ui->system_action_overlay = false;
        return true;
    }
    return false;
}

static void push_navigation(struct cp0_ui *ui)
{
    struct cp0_ui_navigation_entry *entry;

    if (ui->navigation == NULL ||
        ui->navigation_depth >= CP0_UI_NAVIGATION_DEPTH)
        return;
    entry = &ui->navigation[ui->navigation_depth++];
    *entry = (struct cp0_ui_navigation_entry){
        .screen = ui->screen,
        .selected = ui->selected,
        .app_selected = ui->app_selected,
        .app_grid_view = ui->app_grid_view,
        .store_selected = ui->store_selected,
        .store_section = ui->store_section,
        .store_browse_selected = ui->store_browse_selected,
        .store_search_selected = ui->store_search_selected,
        .store_recent_selected = ui->store_recent_selected,
        .store_today_selected = ui->store_today_selected,
        .store_today_collection_selected =
            ui->store_today_collection_selected,
        .store_today_open_collection = ui->store_today_open_collection,
        .store_detail_page = ui->store_detail_page,
        .store_operation_action_selected =
            ui->store_operation_action_selected,
        .store_screenshot_index = ui->store_screenshot_index,
        .store_detail_text_offset = ui->store_detail_text_offset,
        .task_action_selected = ui->task_action_selected,
        .task_selected = ui->task_selected,
        .settings_selected = ui->settings_selected,
        .settings_item_selected = ui->settings_item_selected,
        .app_detail_page = ui->app_detail_page,
        .app_permission_offset = ui->app_permission_offset,
        .app_action_selected = ui->app_action_selected,
        .device_page = ui->device_page,
        .network_page = ui->network_page,
        .developer_host_selected = ui->developer_host_selected,
        .store_search_input = ui->store_search_input,
        .store_update_all_selected = ui->store_update_all_selected,
        .store_detail = ui->store_detail,
        .store_today_collection_open = ui->store_today_collection_open,
        .app_detail = ui->app_detail,
        .settings_detail = ui->settings_detail,
        .developer_hosts_view = ui->developer_hosts_view,
    };
}

static bool pop_navigation(struct cp0_ui *ui)
{
    const struct cp0_ui_navigation_entry *entry;

    if (ui->navigation == NULL || ui->navigation_depth == 0)
        return false;
    entry = &ui->navigation[--ui->navigation_depth];
    ui->screen = entry->screen;
    ui->selected = entry->selected;
    ui->app_selected = entry->app_selected;
    ui->app_grid_view = entry->app_grid_view;
    ui->store_selected = entry->store_selected;
    ui->store_section = entry->store_section;
    ui->store_browse_selected = entry->store_browse_selected;
    ui->store_search_selected = entry->store_search_selected;
    ui->store_recent_selected = entry->store_recent_selected;
    ui->store_today_selected = entry->store_today_selected;
    ui->store_today_collection_selected =
        entry->store_today_collection_selected;
    ui->store_today_open_collection = entry->store_today_open_collection;
    ui->store_detail_page = entry->store_detail_page;
    ui->store_operation_action_selected =
        entry->store_operation_action_selected;
    ui->store_screenshot_index = entry->store_screenshot_index;
    ui->store_detail_text_offset = entry->store_detail_text_offset;
    ui->task_action_selected = entry->task_action_selected;
    ui->task_selected = entry->task_selected;
    ui->settings_selected = entry->settings_selected;
    ui->settings_item_selected = entry->settings_item_selected;
    ui->app_detail_page = entry->app_detail_page;
    ui->app_permission_offset = entry->app_permission_offset;
    ui->app_action_selected = entry->app_action_selected;
    ui->device_page = entry->device_page;
    ui->network_page = entry->network_page;
    ui->developer_host_selected = entry->developer_host_selected;
    ui->store_search_input = entry->store_search_input;
    ui->store_update_all_selected = entry->store_update_all_selected;
    ui->store_detail = entry->store_detail;
    ui->store_today_collection_open = entry->store_today_collection_open;
    ui->app_detail = entry->app_detail;
    ui->settings_detail = entry->settings_detail;
    ui->developer_hosts_view = entry->developer_hosts_view;
    return true;
}

static void enter_screen(struct cp0_ui *ui, enum cp0_ui_screen screen)
{
    if (ui->screen == screen)
        return;
    push_navigation(ui);
    ui->screen = screen;
}

static enum cp0_ui_event handle_password_change_action(
    struct cp0_ui *ui, enum cp0_ui_action action)
{
    if (ui->password_change_page == CP0_UI_PASSWORD_APPLYING)
        return CP0_UI_EVENT_NONE;
    if (ui->password_change_page == CP0_UI_PASSWORD_COMPLETE) {
        if (action == CP0_UI_BACK || action == CP0_UI_ACCEPT)
            cancel_password_change(ui);
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_RIGHT) {
        ui->password_change_show = !ui->password_change_show;
        return CP0_UI_EVENT_NONE;
    }
    if (ui->password_change_page == CP0_UI_PASSWORD_CURRENT) {
        if (action == CP0_UI_BACK) {
            cancel_password_change(ui);
        } else if (action == CP0_UI_ACCEPT) {
            if (ui->password_secrets->current[0] == '\0') {
                copy_optional_text(ui->password_secrets->error,
                                   sizeof(ui->password_secrets->error),
                                   "Enter the current password");
            } else {
                clear_sensitive(ui->password_secrets->new_password,
                                sizeof(ui->password_secrets->new_password));
                clear_sensitive(ui->password_secrets->confirm,
                                sizeof(ui->password_secrets->confirm));
                clear_sensitive(ui->password_secrets->error,
                                sizeof(ui->password_secrets->error));
                ui->password_change_show = false;
                ui->password_change_page = CP0_UI_PASSWORD_NEW;
            }
        }
        return CP0_UI_EVENT_NONE;
    }
    if (ui->password_change_page == CP0_UI_PASSWORD_NEW) {
        if (action == CP0_UI_BACK) {
            clear_sensitive(ui->password_secrets->new_password,
                            sizeof(ui->password_secrets->new_password));
            clear_sensitive(ui->password_secrets->confirm,
                            sizeof(ui->password_secrets->confirm));
            clear_sensitive(ui->password_secrets->error,
                            sizeof(ui->password_secrets->error));
            ui->password_change_show = false;
            ui->password_change_page = CP0_UI_PASSWORD_CURRENT;
        } else if (action == CP0_UI_ACCEPT) {
            if (strlen(ui->password_secrets->new_password) < 10U) {
                copy_optional_text(ui->password_secrets->error,
                                   sizeof(ui->password_secrets->error),
                                   "Use at least 10 characters");
            } else {
                clear_sensitive(ui->password_secrets->confirm,
                                sizeof(ui->password_secrets->confirm));
                clear_sensitive(ui->password_secrets->error,
                                sizeof(ui->password_secrets->error));
                ui->password_change_show = false;
                ui->password_change_page = CP0_UI_PASSWORD_CONFIRM;
            }
        }
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_BACK) {
        clear_sensitive(ui->password_secrets->confirm,
                        sizeof(ui->password_secrets->confirm));
        clear_sensitive(ui->password_secrets->error,
                        sizeof(ui->password_secrets->error));
        ui->password_change_show = false;
        ui->password_change_page = CP0_UI_PASSWORD_NEW;
    } else if (action == CP0_UI_ACCEPT) {
        if (strcmp(ui->password_secrets->new_password,
                   ui->password_secrets->confirm) != 0) {
            clear_sensitive(ui->password_secrets->confirm,
                            sizeof(ui->password_secrets->confirm));
            copy_optional_text(ui->password_secrets->error,
                               sizeof(ui->password_secrets->error),
                               "New passwords do not match");
        } else {
            clear_sensitive(ui->password_secrets->error,
                            sizeof(ui->password_secrets->error));
            ui->password_change_show = false;
            ui->password_change_page = CP0_UI_PASSWORD_APPLYING;
            return CP0_UI_EVENT_CHANGE_PASSWORD;
        }
    }
    return CP0_UI_EVENT_NONE;
}

enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action)
{
    if (ui == NULL)
        return CP0_UI_EVENT_NONE;
    if (ui->setup_active)
        return handle_setup_action(ui, action);
    if (ui->password_change_active)
        return handle_password_change_action(ui, action);
    if (action == CP0_UI_BRIGHTNESS_DOWN || action == CP0_UI_BRIGHTNESS_UP) {
        if (ui->local_simulation && action == CP0_UI_BRIGHTNESS_DOWN)
            ui->brightness_percent = ui->brightness_percent >= 10
                                         ? ui->brightness_percent - 10
                                         : 0;
        else if (ui->local_simulation)
            ui->brightness_percent = ui->brightness_percent <= 90
                                         ? ui->brightness_percent + 10
                                         : 100;
        ui->system_action_kind = 0;
        ui->system_action_overlay = true;
        ui->system_action_ticks = 2;
        if (!ui->local_simulation)
            return action == CP0_UI_BRIGHTNESS_DOWN
                       ? CP0_UI_EVENT_BRIGHTNESS_DOWN
                       : CP0_UI_EVENT_BRIGHTNESS_UP;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_VOLUME_DOWN || action == CP0_UI_VOLUME_UP ||
        action == CP0_UI_MUTE) {
        if (ui->local_simulation && action == CP0_UI_VOLUME_DOWN) {
            ui->volume_percent = ui->volume_percent >= 10
                                     ? ui->volume_percent - 10
                                     : 0;
            ui->muted = false;
        } else if (ui->local_simulation && action == CP0_UI_VOLUME_UP) {
            ui->volume_percent = ui->volume_percent <= 90
                                     ? ui->volume_percent + 10
                                     : 100;
            ui->muted = false;
        } else if (ui->local_simulation) {
            ui->muted = !ui->muted;
        }
        ui->system_action_kind = action == CP0_UI_MUTE ? 2 : 1;
        ui->system_action_overlay = true;
        ui->system_action_ticks = 2;
        if (!ui->local_simulation) {
            if (action == CP0_UI_VOLUME_DOWN)
                return CP0_UI_EVENT_VOLUME_DOWN;
            if (action == CP0_UI_VOLUME_UP)
                return CP0_UI_EVENT_VOLUME_UP;
            return CP0_UI_EVENT_MUTE;
        }
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_MEDIA_PLAY_PAUSE || action == CP0_UI_MEDIA_PREVIOUS ||
        action == CP0_UI_MEDIA_NEXT || action == CP0_UI_SCREENSHOT) {
        ui->system_action_kind = action == CP0_UI_MEDIA_PLAY_PAUSE
                                     ? 3
                                     : (action == CP0_UI_MEDIA_PREVIOUS
                                            ? 4
                                            : (action == CP0_UI_MEDIA_NEXT ? 5 : 6));
        ui->system_action_overlay = true;
        ui->system_action_ticks = 2;
        if (action == CP0_UI_SCREENSHOT)
            ui->screenshot_status = CP0_UI_SCREENSHOT_REQUESTED;
        else
            ui->media_status = CP0_UI_MEDIA_REQUESTED;
        if (action == CP0_UI_MEDIA_PLAY_PAUSE)
            return CP0_UI_EVENT_MEDIA_PLAY_PAUSE;
        if (action == CP0_UI_MEDIA_PREVIOUS)
            return CP0_UI_EVENT_MEDIA_PREVIOUS;
        if (action == CP0_UI_MEDIA_NEXT)
            return CP0_UI_EVENT_MEDIA_NEXT;
        return CP0_UI_EVENT_SCREENSHOT;
    }
    if (action == CP0_UI_HELP && !ui->permission_prompt &&
        !ui->document_prompt && !ui->store_install_prompt) {
        ui->help_overlay = !ui->help_overlay;
        return CP0_UI_EVENT_NONE;
    }
    if (ui->help_overlay) {
        if (action == CP0_UI_BACK || action == CP0_UI_ACCEPT)
            ui->help_overlay = false;
        return CP0_UI_EVENT_NONE;
    }
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
    if (ui->store_install_prompt) {
        if (ui->store_preflight_error != CP0_UI_STORE_PREFLIGHT_NONE) {
            if (action == CP0_UI_BACK || action == CP0_UI_ACCEPT)
                ui->store_install_prompt = false;
            return CP0_UI_EVENT_NONE;
        }
        if (action == CP0_UI_LEFT && ui->dialog_selected > 0)
            ui->dialog_selected--;
        else if (action == CP0_UI_RIGHT && ui->dialog_selected < 1)
            ui->dialog_selected++;
        else if (action == CP0_UI_BACK) {
            ui->store_install_prompt = false;
        } else if (action == CP0_UI_ACCEPT) {
            unsigned int selected = ui->dialog_selected;
            ui->store_install_prompt = false;
            return selected == 0 ? CP0_UI_EVENT_STORE_INSTALL_CONFIRM
                                 : CP0_UI_EVENT_NONE;
        }
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_GO_HOME) {
        ui->power_dialog = false;
        ui->settings_confirm = false;
        ui->developer_hosts_view = false;
        ui->developer_revoke_confirm = false;
        ui->store_detail = false;
        ui->app_detail = false;
        ui->settings_detail = false;
        ui->screen = CP0_UI_HOME;
        ui->navigation_depth = 0;
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_TASKS) {
        enter_screen(ui, CP0_UI_TASKS);
        ui->power_dialog = false;
        ui->settings_confirm = false;
        ui->developer_hosts_view = false;
        ui->developer_revoke_confirm = false;
        ui->task_action_selected = 0;
        ui->task_selected = 0;
        for (unsigned int index = 0; index < ui->task_count; index++) {
            if (ui->tasks[index].state == CP0_UI_TASK_FOREGROUND) {
                ui->task_selected = index;
                break;
            }
        }
        return CP0_UI_EVENT_NONE;
    }
    if (action == CP0_UI_SHOW_POWER) {
        ui->settings_confirm = false;
        ui->developer_revoke_confirm = false;
        ui->power_dialog = true;
        ui->dialog_selected = 0;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->power_dialog) {
        if (action == CP0_UI_LEFT && ui->dialog_selected > 0)
            ui->dialog_selected--;
        else if (action == CP0_UI_RIGHT && ui->dialog_selected < 3)
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
            if (selected == 2)
                return CP0_UI_EVENT_POWER_OFF;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->app_uninstall_confirm) {
        if (action == CP0_UI_LEFT)
            ui->dialog_selected = 0;
        else if (action == CP0_UI_RIGHT)
            ui->dialog_selected = 1;
        else if (action == CP0_UI_BACK)
            ui->app_uninstall_confirm = false;
        else if (action == CP0_UI_ACCEPT) {
            unsigned int selected = ui->dialog_selected;
            ui->app_uninstall_confirm = false;
            if (selected == 0)
                return CP0_UI_EVENT_UNINSTALL_APP;
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
            bool metrics = ui->settings_confirm_metrics;
            unsigned int selected = ui->dialog_selected;
            ui->settings_confirm = false;
            if (selected == 0)
                return metrics ? CP0_UI_EVENT_METRICS_ENABLE
                               : (recovery ? CP0_UI_EVENT_RECOVERY_ENABLE
                                           : CP0_UI_EVENT_DEVELOPER_ENABLE);
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->developer_revoke_confirm) {
        if (action == CP0_UI_LEFT)
            ui->dialog_selected = 0;
        else if (action == CP0_UI_RIGHT)
            ui->dialog_selected = 1;
        else if (action == CP0_UI_BACK)
            ui->developer_revoke_confirm = false;
        else if (action == CP0_UI_ACCEPT) {
            unsigned int selected = ui->dialog_selected;
            bool revoke_all = ui->developer_revoke_all;
            ui->developer_revoke_confirm = false;
            if (selected == 0)
                return revoke_all ? CP0_UI_EVENT_DEVELOPER_UNPAIR_ALL
                                  : CP0_UI_EVENT_DEVELOPER_UNPAIR;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (action == CP0_UI_BACK) {
        if (ui->foreground_app_name[0] != '\0') {
            ui->foreground_app_name[0] = '\0';
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_APPS && ui->app_detail) {
            ui->app_detail = false;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_STORE && ui->store_detail) {
            ui->store_detail = false;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_STORE &&
            ui->store_section == CP0_UI_STORE_TODAY &&
            ui->store_today_collection_open) {
            ui->store_today_selected =
                ui->store_today_open_collection + 1U;
            ui->store_today_collection_open = false;
            ui->store_today_collection_selected = 0;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_STORE &&
            ui->store_section == CP0_UI_STORE_SEARCH &&
            ui->store_search_input) {
            ui->store_search_input = false;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_STORE &&
            ui->store_section == CP0_UI_STORE_SEARCH &&
            ui->store_search_query[0] != '\0') {
            ui->store_search_query[0] = '\0';
            reset_store_search_results(ui);
            ui->store_search_input = true;
            return CP0_UI_EVENT_NONE;
        }
        if (ui->screen == CP0_UI_SETTINGS && ui->settings_detail) {
            if (ui->developer_hosts_view) {
                ui->developer_hosts_view = false;
                return CP0_UI_EVENT_NONE;
            }
            ui->settings_detail = false;
            return CP0_UI_EVENT_NONE;
        }
        if (!pop_navigation(ui))
            ui->screen = CP0_UI_HOME;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_TASKS) {
        if (action == CP0_UI_LEFT && ui->task_selected > 0)
            ui->task_selected--;
        else if (action == CP0_UI_RIGHT &&
                 ui->task_selected + 1 < ui->task_count)
            ui->task_selected++;
        else if (action == CP0_UI_ACCEPT && ui->task_count > 0)
            return CP0_UI_EVENT_ACTIVATE_TASK;
        else if (action == CP0_UI_UP && ui->task_count > 0)
            return CP0_UI_EVENT_CLOSE_TASK;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_APPS) {
        if (ui->app_detail) {
            enum cp0_ui_app_state state = cp0_ui_selected_app_state(ui);
            if (action == CP0_UI_LEFT && ui->app_detail_page > 0) {
                ui->app_detail_page--;
                ui->app_permission_offset = 0;
            } else if (action == CP0_UI_RIGHT && ui->app_detail_page < 3) {
                ui->app_detail_page++;
                ui->app_permission_offset = 0;
            } else if (ui->app_detail_page == 2 && action == CP0_UI_UP &&
                       ui->app_permission_offset > 0) {
                ui->app_permission_offset--;
            } else if (ui->app_detail_page == 2 && action == CP0_UI_DOWN) {
                unsigned int permission_count = 0;
                for (unsigned int bit = 0; bit < 10; bit++)
                    permission_count +=
                        (ui->apps[ui->app_selected].permissions & (1U << bit)) !=
                        0;
                if (ui->app_permission_offset + 4U < permission_count)
                    ui->app_permission_offset++;
            } else if (ui->app_detail_page == 3 && action == CP0_UI_UP) {
                ui->app_action_selected = 0;
            } else if (ui->app_detail_page == 3 && action == CP0_UI_DOWN) {
                ui->app_action_selected = 1;
            } else if (ui->app_detail_page == 3 && action == CP0_UI_ACCEPT &&
                       ui->app_action_selected == 1 &&
                       state != CP0_UI_APP_RUNNING &&
                       state != CP0_UI_APP_STARTING) {
                ui->app_uninstall_confirm = true;
                ui->dialog_selected = 1;
            } else if (ui->app_detail_page == 3 && action == CP0_UI_ACCEPT &&
                       ui->app_action_selected == 0 &&
                       state == CP0_UI_APP_RUNNING) {
                return CP0_UI_EVENT_STOP_APP;
            } else if (ui->app_detail_page == 3 && action == CP0_UI_ACCEPT &&
                       ui->app_action_selected == 0 &&
                       (state == CP0_UI_APP_STOPPED ||
                        state == CP0_UI_APP_FAILED)) {
                return CP0_UI_EVENT_OPEN_APP;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (action == CP0_UI_TOGGLE_APP_VIEW) {
            ui->app_grid_view = !ui->app_grid_view;
        } else if (ui->app_grid_view && action == CP0_UI_LEFT &&
                   ui->app_selected > 0) {
            ui->app_selected--;
        } else if (ui->app_grid_view && action == CP0_UI_RIGHT &&
                   ui->app_selected + 1 < ui->app_count) {
            ui->app_selected++;
        } else if (ui->app_grid_view && action == CP0_UI_UP &&
                   ui->app_selected >= 4) {
            ui->app_selected -= 4;
        } else if (ui->app_grid_view && action == CP0_UI_DOWN &&
                   ui->app_selected + 4 < ui->app_count) {
            ui->app_selected += 4;
        } else if (action == CP0_UI_UP && ui->app_selected > 0) {
            ui->app_selected--;
        } else if (action == CP0_UI_DOWN &&
                   ui->app_selected + 1 < ui->app_count) {
            ui->app_selected++;
        }
        else if (action == CP0_UI_ACCEPT && ui->app_count > 0)
            return CP0_UI_EVENT_OPEN_APP;
        else if (!ui->app_grid_view && action == CP0_UI_RIGHT &&
                 ui->app_count > 0)
            ui->app_detail = true, ui->app_detail_page = 0,
            ui->app_permission_offset = 0, ui->app_action_selected = 0;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_DEVICE) {
        if (action == CP0_UI_LEFT && ui->device_page > 0)
            ui->device_page--;
        else if (action == CP0_UI_RIGHT && ui->device_page < 3)
            ui->device_page++;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_NETWORK) {
        if (action == CP0_UI_LEFT)
            ui->network_page = 0;
        else if (action == CP0_UI_RIGHT)
            ui->network_page = 1;
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_STORE) {
        if (ui->store_detail) {
            enum cp0_ui_store_state state =
                cp0_ui_selected_store_app_state(ui);
            if (action == CP0_UI_LEFT && ui->store_detail_page > 0) {
                ui->store_detail_page--;
                ui->store_detail_text_offset = 0;
                if (ui->store_detail_page == 0)
                    ui->store_operation_action_selected = 0;
                if (ui->store_detail_page == 2 &&
                    ui->store_detail_status == CP0_UI_STORE_DETAIL_READY) {
                    ui->store_screenshot_available = false;
                    ui->store_screenshot_loading = true;
                    return CP0_UI_EVENT_STORE_SCREENSHOT;
                }
            } else if (action == CP0_UI_RIGHT &&
                       ui->store_detail_page < 4) {
                ui->store_detail_page++;
                ui->store_detail_text_offset = 0;
                if (ui->store_detail_page == 2 &&
                    ui->store_detail_status == CP0_UI_STORE_DETAIL_READY) {
                    ui->store_screenshot_available = false;
                    ui->store_screenshot_loading = true;
                    return CP0_UI_EVENT_STORE_SCREENSHOT;
                }
            } else if (ui->store_detail_page == 2 && action == CP0_UI_UP &&
                       ui->store_screenshot_index > 0) {
                ui->store_screenshot_index--;
                ui->store_screenshot_available = false;
                ui->store_screenshot_loading = true;
                return CP0_UI_EVENT_STORE_SCREENSHOT;
            } else if (ui->store_detail_page == 2 && action == CP0_UI_DOWN &&
                       ui->store_screenshot_index + 1U <
                           ui->store_screenshot_count) {
                ui->store_screenshot_index++;
                ui->store_screenshot_available = false;
                ui->store_screenshot_loading = true;
                return CP0_UI_EVENT_STORE_SCREENSHOT;
            } else if ((ui->store_detail_page == 1 ||
                        ui->store_detail_page == 4) &&
                       action == CP0_UI_UP) {
                if (ui->store_detail_text_offset > 0)
                    ui->store_detail_text_offset--;
            } else if ((ui->store_detail_page == 1 ||
                        ui->store_detail_page == 4) &&
                       action == CP0_UI_DOWN) {
                const char *prose = ui->store_detail_page == 1
                                        ? ui->store_description
                                        : ui->store_release_notes;
                if (ui->store_detail_text_offset + 7U <
                    wrapped_line_count(prose, 46))
                    ui->store_detail_text_offset++;
            } else if (ui->store_detail_page == 0 && action == CP0_UI_UP &&
                       ui->store_operation_action_selected > 0) {
                ui->store_operation_action_selected = 0;
            } else if (ui->store_detail_page == 0 && action == CP0_UI_DOWN &&
                       store_operation_has_cancel(state)) {
                ui->store_operation_action_selected = 1;
            } else if (ui->store_detail_page == 0 &&
                       action == CP0_UI_ACCEPT) {
                bool stale = ui->store_section == CP0_UI_STORE_SEARCH
                                 ? ui->store_search_stale
                                 : (ui->store_section == CP0_UI_STORE_APPS
                                        ? ui->store_browse_stale
                                        : ui->store_catalog_stale);
                if (ui->store_operation_action_selected == 1 &&
                    store_operation_has_cancel(state))
                    return CP0_UI_EVENT_STORE_CANCEL;
                if (state == CP0_UI_STORE_QUEUED ||
                    state == CP0_UI_STORE_DOWNLOADING)
                    return CP0_UI_EVENT_STORE_PAUSE;
                if (!stale && state == CP0_UI_STORE_PAUSED)
                    return CP0_UI_EVENT_STORE_RESUME;
                if (!stale &&
                    (state == CP0_UI_STORE_AVAILABLE ||
                     state == CP0_UI_STORE_UPDATE ||
                     state == CP0_UI_STORE_CANCELED ||
                     state == CP0_UI_STORE_FAILED))
                    return CP0_UI_EVENT_STORE_INSTALL;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (action == CP0_UI_LEFT && !ui->store_search_input &&
            !ui->store_today_collection_open &&
            ui->store_section > CP0_UI_STORE_TODAY) {
            ui->store_section--;
            ui->store_update_all_selected = false;
            ui->store_selected = 0;
            ui->store_browse_selected = 0;
            ui->store_search_selected = 0;
            ui->store_search_input =
                ui->store_section == CP0_UI_STORE_SEARCH;
            if (ui->store_section == CP0_UI_STORE_APPS) {
                ui->store_browse_count = 0;
                ui->store_browse_status = CP0_UI_STORE_LOADING;
                return CP0_UI_EVENT_STORE_BROWSE;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (action == CP0_UI_RIGHT && !ui->store_search_input &&
            !ui->store_today_collection_open &&
            ui->store_section < CP0_UI_STORE_UPDATES) {
            ui->store_section++;
            ui->store_selected = 0;
            ui->store_browse_selected = 0;
            ui->store_search_selected = 0;
            ui->store_search_input =
                ui->store_section == CP0_UI_STORE_SEARCH;
            ui->store_update_all_selected =
                ui->store_section == CP0_UI_STORE_UPDATES &&
                store_update_batch_count(ui) > 0;
            if (ui->store_section == CP0_UI_STORE_APPS) {
                ui->store_browse_count = 0;
                ui->store_browse_status = CP0_UI_STORE_LOADING;
                return CP0_UI_EVENT_STORE_BROWSE;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (ui->store_section == CP0_UI_STORE_SEARCH) {
            if (action == CP0_UI_DOWN && ui->store_search_input &&
                (ui->store_search_count > 0 || ui->store_recent_count > 0)) {
                ui->store_search_input = false;
                return CP0_UI_EVENT_NONE;
            }
            if (action == CP0_UI_ACCEPT && ui->store_search_input &&
                ui->store_search_query[0] != '\0') {
                remember_store_query(ui);
                ui->store_search_input = false;
                return ui->store_search_status == CP0_UI_STORE_READY
                           ? CP0_UI_EVENT_NONE
                           : CP0_UI_EVENT_STORE_SEARCH;
            }
            if (ui->store_search_query[0] == '\0') {
                if (action == CP0_UI_UP && ui->store_recent_selected > 0)
                    ui->store_recent_selected--;
                else if (action == CP0_UI_DOWN &&
                         !ui->store_search_input &&
                         ui->store_recent_selected + 1 <
                             ui->store_recent_count)
                    ui->store_recent_selected++;
                else if (action == CP0_UI_ACCEPT &&
                         !ui->store_search_input &&
                         ui->store_recent_selected < ui->store_recent_count) {
                    copy_optional_text(
                        ui->store_search_query,
                        sizeof(ui->store_search_query),
                        ui->store_recent_queries[ui->store_recent_selected]);
                    reset_store_search_results(ui);
                    ui->store_search_input = true;
                    return CP0_UI_EVENT_STORE_SEARCH;
                }
                return CP0_UI_EVENT_NONE;
            }
            if (ui->store_search_input)
                return CP0_UI_EVENT_NONE;
            if (action == CP0_UI_UP && ui->store_search_selected > 0) {
                ui->store_search_selected--;
            } else if (action == CP0_UI_UP && ui->store_search_offset > 0) {
                ui->store_search_offset =
                    ui->store_search_offset > CP0_UI_STORE_SEARCH_PAGE_MAX
                        ? (uint16_t)(ui->store_search_offset -
                                     CP0_UI_STORE_SEARCH_PAGE_MAX)
                        : 0;
                ui->store_search_status = CP0_UI_STORE_LOADING;
                ui->store_search_count = 0;
                return CP0_UI_EVENT_STORE_SEARCH;
            } else if (action == CP0_UI_UP) {
                ui->store_search_input = true;
            } else if (action == CP0_UI_DOWN &&
                       ui->store_search_selected + 1 <
                           ui->store_search_count) {
                ui->store_search_selected++;
            } else if (action == CP0_UI_DOWN &&
                       ui->store_search_has_next) {
                ui->store_search_offset = ui->store_search_next_offset;
                ui->store_search_status = CP0_UI_STORE_LOADING;
                ui->store_search_count = 0;
                ui->store_search_selected = 0;
                return CP0_UI_EVENT_STORE_SEARCH;
            } else if (action == CP0_UI_ACCEPT &&
                       ui->store_search_count > 0) {
                remember_store_query(ui);
                begin_store_detail(ui);
                return CP0_UI_EVENT_STORE_DETAILS;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (ui->store_section == CP0_UI_STORE_APPS) {
            if (ui->store_browse_status != CP0_UI_STORE_READY) {
                if (action != CP0_UI_ACCEPT)
                    return CP0_UI_EVENT_NONE;
                return ui->store_browse_status == CP0_UI_STORE_UNCONFIGURED
                           ? CP0_UI_EVENT_STORE_REFRESH
                           : CP0_UI_EVENT_STORE_BROWSE;
            }
            if (action == CP0_UI_UP && ui->store_browse_selected > 0) {
                ui->store_browse_selected--;
            } else if (action == CP0_UI_UP && ui->store_browse_offset > 0) {
                ui->store_browse_offset =
                    ui->store_browse_offset > CP0_UI_STORE_SEARCH_PAGE_MAX
                        ? (uint16_t)(ui->store_browse_offset -
                                     CP0_UI_STORE_SEARCH_PAGE_MAX)
                        : 0;
                ui->store_browse_status = CP0_UI_STORE_LOADING;
                ui->store_browse_count = 0;
                return CP0_UI_EVENT_STORE_BROWSE;
            } else if (action == CP0_UI_DOWN &&
                       ui->store_browse_selected + 1 <
                           ui->store_browse_count) {
                ui->store_browse_selected++;
            } else if (action == CP0_UI_DOWN &&
                       ui->store_browse_has_next) {
                ui->store_browse_offset = ui->store_browse_next_offset;
                ui->store_browse_status = CP0_UI_STORE_LOADING;
                ui->store_browse_count = 0;
                ui->store_browse_selected = 0;
                return CP0_UI_EVENT_STORE_BROWSE;
            } else if (action == CP0_UI_ACCEPT &&
                       ui->store_browse_count > 0) {
                begin_store_detail(ui);
                return CP0_UI_EVENT_STORE_DETAILS;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (ui->store_status != CP0_UI_STORE_READY)
            return action == CP0_UI_ACCEPT ? CP0_UI_EVENT_STORE_REFRESH
                                           : CP0_UI_EVENT_NONE;
        unsigned int count = store_section_count(ui);
        if (ui->store_section == CP0_UI_STORE_TODAY &&
            ui->store_today_available) {
            unsigned int *selected = ui->store_today_collection_open
                                         ? &ui->store_today_collection_selected
                                         : &ui->store_today_selected;
            if (action == CP0_UI_UP && *selected > 0) {
                (*selected)--;
            } else if (action == CP0_UI_DOWN && *selected + 1U < count) {
                (*selected)++;
            } else if (action == CP0_UI_ACCEPT && count > 0) {
                if (!ui->store_today_collection_open && *selected > 0) {
                    ui->store_today_open_collection = *selected - 1U;
                    ui->store_today_collection_selected = 0;
                    ui->store_today_collection_open = true;
                } else {
                    begin_store_detail(ui);
                    return CP0_UI_EVENT_STORE_DETAILS;
                }
            }
            return CP0_UI_EVENT_NONE;
        }
        if (ui->store_section == CP0_UI_STORE_UPDATES &&
            ui->store_update_all_selected) {
            if (action == CP0_UI_DOWN && count > 0)
                ui->store_update_all_selected = false;
            else if (action == CP0_UI_ACCEPT && !ui->store_catalog_stale &&
                     store_update_batch_count(ui) > 0)
                return CP0_UI_EVENT_STORE_UPDATE_ALL;
        } else if (action == CP0_UI_UP && ui->store_selected > 0) {
            ui->store_selected--;
        } else if (action == CP0_UI_UP &&
                   ui->store_section == CP0_UI_STORE_UPDATES &&
                   store_update_batch_count(ui) > 0) {
            ui->store_update_all_selected = true;
        } else if (action == CP0_UI_DOWN && ui->store_selected + 1 < count) {
            ui->store_selected++;
        } else if (action == CP0_UI_ACCEPT && count > 0) {
            begin_store_detail(ui);
            return CP0_UI_EVENT_STORE_DETAILS;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen == CP0_UI_SETTINGS) {
        if (ui->developer_hosts_view) {
            unsigned int count = ui->developer_host_count + 1U;
            if (action == CP0_UI_UP && ui->developer_host_selected > 0)
                ui->developer_host_selected--;
            else if (action == CP0_UI_DOWN &&
                     ui->developer_host_selected + 1U < count)
                ui->developer_host_selected++;
            else if (action == CP0_UI_ACCEPT) {
                ui->developer_revoke_all =
                    ui->developer_host_selected == ui->developer_host_count;
                ui->developer_revoke_confirm = true;
                ui->dialog_selected = 1;
            }
            return CP0_UI_EVENT_NONE;
        }
        if (ui->settings_detail) {
            unsigned int count = settings_item_count(ui->settings_selected);
            unsigned int item = ui->settings_item_selected;
            if (action == CP0_UI_UP && item > 0)
                ui->settings_item_selected--;
            else if (action == CP0_UI_DOWN && item + 1 < count)
                ui->settings_item_selected++;
            else if ((ui->local_simulation ||
                      (ui->connectivity_available && ui->wifi_available)) &&
                     ui->settings_selected == 0 &&
                     item == 0 &&
                     (action == CP0_UI_ACCEPT || action == CP0_UI_LEFT ||
                      action == CP0_UI_RIGHT)) {
                if (ui->local_simulation) {
                    ui->wifi_enabled = !ui->wifi_enabled;
                    if (ui->wifi_enabled)
                        ui->airplane_mode = false;
                } else {
                    return ui->wifi_enabled ? CP0_UI_EVENT_WIFI_DISABLE
                                            : CP0_UI_EVENT_WIFI_ENABLE;
                }
            } else if ((ui->local_simulation || ui->connectivity_available) &&
                       ui->settings_selected == 0 &&
                       item == 1 &&
                       (action == CP0_UI_ACCEPT || action == CP0_UI_LEFT ||
                        action == CP0_UI_RIGHT)) {
                if (ui->local_simulation) {
                    ui->airplane_mode = !ui->airplane_mode;
                    if (ui->airplane_mode)
                        ui->wifi_enabled = false;
                } else {
                    return ui->airplane_mode
                               ? CP0_UI_EVENT_AIRPLANE_DISABLE
                               : CP0_UI_EVENT_AIRPLANE_ENABLE;
                }
            } else if (ui->settings_selected == 0 && item == 2 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_NETWORK);
                ui->network_page = 0;
                ui->settings_detail = false;
            } else if ((ui->local_simulation || ui->brightness_available) &&
                       ui->settings_selected == 1 &&
                       item == 0 &&
                       (action == CP0_UI_LEFT || action == CP0_UI_RIGHT)) {
                return cp0_ui_handle_action(ui, action == CP0_UI_LEFT
                                                    ? CP0_UI_BRIGHTNESS_DOWN
                                                    : CP0_UI_BRIGHTNESS_UP);
            } else if (ui->settings_selected == 1 && item == 1 &&
                       (action == CP0_UI_LEFT || action == CP0_UI_RIGHT ||
                        action == CP0_UI_ACCEPT)) {
                if (ui->local_simulation)
                    ui->theme =
                        (ui->theme + (action == CP0_UI_LEFT ? 2U : 1U)) % 3U;
                else
                    return action == CP0_UI_LEFT ? CP0_UI_EVENT_THEME_PREVIOUS
                                                : CP0_UI_EVENT_THEME_NEXT;
            } else if (ui->settings_selected == 1 && item == 2 &&
                       (action == CP0_UI_LEFT || action == CP0_UI_RIGHT ||
                        action == CP0_UI_ACCEPT)) {
                if (ui->local_simulation)
                    ui->screen_timeout =
                        (ui->screen_timeout + (action == CP0_UI_LEFT ? 3U : 1U)) % 4U;
                else
                    return action == CP0_UI_LEFT ? CP0_UI_EVENT_TIMEOUT_PREVIOUS
                                                : CP0_UI_EVENT_TIMEOUT_NEXT;
            } else if ((ui->local_simulation || ui->volume_available) &&
                       ui->settings_selected == 2 &&
                       item == 0 &&
                       (action == CP0_UI_LEFT || action == CP0_UI_RIGHT)) {
                return cp0_ui_handle_action(ui, action == CP0_UI_LEFT
                                                    ? CP0_UI_VOLUME_DOWN
                                                    : CP0_UI_VOLUME_UP);
            } else if ((ui->local_simulation || ui->volume_available) &&
                       ui->settings_selected == 2 &&
                       item == 1 &&
                       action == CP0_UI_ACCEPT) {
                return cp0_ui_handle_action(ui, CP0_UI_MUTE);
            } else if (ui->settings_selected == 2 && item == 2 &&
                       action == CP0_UI_ACCEPT) {
                if (ui->local_simulation)
                    ui->key_sounds = !ui->key_sounds;
                else
                    return CP0_UI_EVENT_KEY_SOUNDS_TOGGLE;
            } else if (ui->local_simulation && ui->settings_selected == 3 &&
                       item == 1 &&
                       (action == CP0_UI_LEFT || action == CP0_UI_RIGHT ||
                        action == CP0_UI_ACCEPT)) {
                ui->camera_rotation =
                    (ui->camera_rotation + (action == CP0_UI_LEFT ? 3U : 1U)) % 4U;
            } else if (ui->local_simulation && ui->settings_selected == 3 &&
                       item == 2 &&
                       action == CP0_UI_ACCEPT) {
                ui->camera_mirror = !ui->camera_mirror;
            } else if (ui->settings_selected == 4 && item == 0 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_DEVICE);
                ui->device_page = 2;
                ui->settings_detail = false;
            } else if (ui->settings_selected == 4 && item == 3 &&
                       action == CP0_UI_ACCEPT) {
                ui->power_dialog = true;
                ui->dialog_selected = 1;
            } else if (ui->settings_selected == 4 && item == 4 &&
                       action == CP0_UI_ACCEPT) {
                ui->power_dialog = true;
                ui->dialog_selected = 2;
            } else if (ui->settings_selected == 5 && item == 0 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_APPS);
                ui->app_detail = false;
                ui->settings_detail = false;
            } else if (ui->settings_selected == 5 && item == 1 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_APPS);
                ui->app_detail = false;
                ui->settings_detail = false;
            } else if (ui->settings_selected == 5 && item == 2 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_DEVICE);
                ui->device_page = 1;
                ui->settings_detail = false;
            } else if (ui->settings_selected == 5 && item == 4 &&
                       action == CP0_UI_ACCEPT &&
                       ui->auto_update_available &&
                       (ui->auto_update_policy_allowed ||
                        ui->auto_update_enabled)) {
                return ui->auto_update_enabled
                           ? CP0_UI_EVENT_AUTO_UPDATE_DISABLE
                           : CP0_UI_EVENT_AUTO_UPDATE_ENABLE;
            } else if (ui->settings_selected == 5 && item == 5 &&
                       action == CP0_UI_ACCEPT && ui->metrics_available) {
                if (ui->metrics_enabled)
                    return CP0_UI_EVENT_METRICS_DISABLE;
                if (ui->metrics_policy_allowed && ui->metrics_configured) {
                    ui->settings_confirm = true;
                    ui->settings_confirm_recovery = false;
                    ui->settings_confirm_metrics = true;
                    ui->dialog_selected = 1;
                }
            } else if (ui->settings_selected == 6 && item <= 1 &&
                       action == CP0_UI_ACCEPT) {
                enter_screen(ui, CP0_UI_DEVICE);
                ui->device_page = item == 0 ? 0 : 3;
                ui->settings_detail = false;
            } else if (ui->settings_selected == 7 && item == 2 &&
                       action == CP0_UI_ACCEPT && ui->developer_mode &&
                       ui->developer_access_available) {
                return CP0_UI_EVENT_DEVELOPER_OPEN_PAIRING;
            } else if (ui->settings_selected == 7 && item == 3 &&
                       action == CP0_UI_ACCEPT && ui->developer_mode &&
                       ui->developer_access_available) {
                ui->developer_hosts_view = true;
                ui->developer_host_selected = 0;
            } else if (ui->settings_selected == 7 && item == 5 &&
                       action == CP0_UI_ACCEPT &&
                       ui->password_secrets != NULL) {
                clear_password_change_secrets(ui);
                ui->password_change_active = true;
                ui->password_change_show = false;
                ui->password_change_page = CP0_UI_PASSWORD_CURRENT;
                ui->power_dialog = false;
                ui->help_overlay = false;
                ui->system_action_overlay = false;
            } else if (ui->settings_selected == 7 && (item == 1 || item == 4) &&
                       action == CP0_UI_ACCEPT) {
                bool recovery = item == 4;
                bool allowed = recovery ? ui->recovery_mode_allowed
                                        : ui->developer_mode_allowed;
                bool enabled = recovery ? ui->recovery_mode : ui->developer_mode;
                if (allowed && enabled)
                    return recovery ? CP0_UI_EVENT_RECOVERY_DISABLE
                                    : CP0_UI_EVENT_DEVELOPER_DISABLE;
                if (allowed) {
                    ui->settings_confirm = true;
                    ui->settings_confirm_recovery = recovery;
                    ui->settings_confirm_metrics = false;
                    ui->dialog_selected = 1;
                }
            }
            return CP0_UI_EVENT_NONE;
        }
        if (action == CP0_UI_UP && ui->settings_selected > 0)
            ui->settings_selected--;
        else if (action == CP0_UI_DOWN && ui->settings_selected < 7)
            ui->settings_selected++;
        else if (action == CP0_UI_ACCEPT) {
            ui->settings_detail = true;
            ui->settings_item_selected = 0;
        }
        return CP0_UI_EVENT_NONE;
    }

    if (ui->screen != CP0_UI_HOME)
        return CP0_UI_EVENT_NONE;

    if (action == CP0_UI_LEFT)
        ui->selected = ui->selected == 0 ? CP0_UI_HOME_ITEM_COUNT - 1
                                         : ui->selected - 1;
    else if (action == CP0_UI_RIGHT)
        ui->selected = (ui->selected + 1) % CP0_UI_HOME_ITEM_COUNT;
    else if (action == CP0_UI_UP && ui->selected >= CP0_UI_HOME_COLUMNS)
        ui->selected -= CP0_UI_HOME_COLUMNS;
    else if (action == CP0_UI_DOWN &&
             ui->selected + CP0_UI_HOME_COLUMNS < CP0_UI_HOME_ITEM_COUNT)
        ui->selected += CP0_UI_HOME_COLUMNS;
    else if (action == CP0_UI_ACCEPT) {
        push_navigation(ui);
        switch (ui->selected) {
        case 0:
            ui->screen = CP0_UI_APPS;
            ui->app_detail = false;
            break;
        case 1:
            ui->screen = CP0_UI_STORE;
            break;
        case 2:
            ui->screen = CP0_UI_DEVICE;
            ui->device_page = 0;
            break;
        case 3:
            ui->screen = CP0_UI_NETWORK;
            ui->network_page = 0;
            break;
        default:
            ui->screen = CP0_UI_SETTINGS;
            ui->settings_selected = 0;
            ui->settings_item_selected = 0;
            ui->settings_detail = false;
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
    if (ui->setup_active)
        draw_setup(&canvas, ui);
    else
        draw_page(&canvas, ui);
    if (ui->setup_active)
        goto apply_theme;
    if (ui->password_change_active)
        goto apply_theme;
    if (ui->notification_banner && !ui->power_dialog &&
        !ui->settings_confirm && !ui->developer_revoke_confirm &&
        !ui->permission_prompt &&
        !ui->document_prompt && !ui->store_install_prompt)
        draw_notification_banner(&canvas, ui);
    if (ui->power_dialog)
        draw_power_dialog(&canvas, ui);
    else if (ui->app_uninstall_confirm)
        draw_uninstall_confirm(&canvas, ui);
    else if (ui->settings_confirm)
        draw_settings_confirm(&canvas, ui);
    else if (ui->developer_revoke_confirm)
        draw_developer_revoke_confirm(&canvas, ui);
    if (ui->permission_prompt)
        draw_permission_dialog(&canvas, ui);
    else if (ui->document_prompt)
        draw_document_dialog(&canvas, ui);
    else if (ui->store_install_prompt)
        draw_store_install_prompt(&canvas, ui);
    else if (ui->help_overlay)
        draw_help_overlay(&canvas);
    else if (ui->system_action_overlay)
        draw_system_action_overlay(&canvas, ui);

apply_theme:
    if (ui->theme != 0) {
        static const uint32_t source[] = {
            COLOR_BG, COLOR_BAR, COLOR_SURFACE, COLOR_SELECTED, COLOR_TEXT,
            COLOR_MUTED, COLOR_GREEN, COLOR_YELLOW, COLOR_RED, 0x00090b0cu,
        };
        static const uint32_t light[] = {
            0x00e9eeecu, 0x00dce3e0u, 0x00f8faf9u, 0x00d8eee2u,
            0x00101515u, 0x0057645fu, 0x00087443u, 0x009a6a00u,
            0x00b4232du, 0x00ffffffu,
        };
        static const uint32_t contrast[] = {
            0x00000000u, 0x00000000u, 0x00101010u, 0x0000391fu,
            0x00ffffffu, 0x00c0c0c0u, 0x0000ff80u, 0x00ffff00u,
            0x00ff4040u, 0x00000000u,
        };
        const uint32_t *target = ui->theme == 1 ? light : contrast;
        for (int y = 0; y < height; y++) {
            for (int x = 0; x < width; x++) {
                uint32_t *pixel = &pixels[y * stride_pixels + x];
                for (size_t index = 0;
                     index < sizeof(source) / sizeof(source[0]); index++) {
                    if (*pixel == source[index]) {
                        *pixel = target[index];
                        break;
                    }
                }
            }
        }
    }
}
