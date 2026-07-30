#include "cp0_ui.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

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
    cp0_ui_init(ui);
    cp0_ui_set_status(ui, "12:34", true, 73);
    write_snapshot(directory, "home", ui, frame);

    cp0_ui_add_app(ui, 41, "dev.cardputerzero.first");
    cp0_ui_add_app(ui, 42, "dev.cardputerzero.second");
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "apps", ui, frame);

    cp0_ui_handle_action(ui, CP0_UI_SHOW_TASKS);
    write_snapshot(directory, "tasks", ui, frame);

    cp0_ui_handle_action(ui, CP0_UI_SHOW_POWER);
    cp0_ui_handle_action(ui, CP0_UI_RIGHT);
    write_snapshot(directory, "power", ui, frame);
}

int main(int argc, char **argv)
{
    struct cp0_ui ui;
    struct guarded_frame *frame = calloc(1, sizeof(*frame));
    assert(frame != NULL);

    cp0_ui_init(&ui);
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
    assert(ui.power_dialog);
    assert(pixel(frame, 36, 35) == GREEN);

    cp0_ui_handle_action(&ui, CP0_UI_RIGHT);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_RESTART);
    assert(!ui.power_dialog);

    cp0_ui_handle_action(&ui, CP0_UI_SHOW_TASKS);
    assert(ui.screen == CP0_UI_TASKS);
    cp0_ui_handle_action(&ui, CP0_UI_BACK);
    assert(ui.screen == CP0_UI_HOME);

    cp0_ui_init(&ui);
    cp0_ui_add_app(&ui, 41, "dev.cardputerzero.first");
    cp0_ui_add_app(&ui, 42, "dev.cardputerzero.second");
    cp0_ui_add_app(&ui, 42, "dev.cardputerzero.updated");
    assert(ui.app_count == 2);
    assert(cp0_ui_selected_app_token(&ui) == 41);
    cp0_ui_handle_action(&ui, CP0_UI_ACCEPT);
    assert(ui.screen == CP0_UI_APPS);
    cp0_ui_handle_action(&ui, CP0_UI_DOWN);
    assert(cp0_ui_selected_app_token(&ui) == 42);
    assert(cp0_ui_handle_action(&ui, CP0_UI_ACCEPT) ==
           CP0_UI_EVENT_OPEN_APP);
    render(&ui, frame);
    assert(pixel(frame, 8, 60) == GREEN);
    cp0_ui_remove_app(&ui, 42);
    assert(ui.app_count == 1);
    assert(cp0_ui_selected_app_token(&ui) == 41);
    cp0_ui_remove_app(&ui, 99);
    assert(ui.app_count == 1);

    render(&ui, frame);
    if (argc == 2)
        write_snapshots(argv[1], &ui, frame);
    free(frame);
    return 0;
}
