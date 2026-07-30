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
         .name = "First Card"},
        {.running = true,
         .immersive = true,
         .app_id = "dev.cardputerzero.second",
         .name = "Second Card"},
    };
    cp0_ui_init(ui);
    cp0_ui_set_status(ui, "12:34", true, 73);
    write_snapshot(directory, "home", ui, frame);

    cp0_ui_sync_app_catalog(ui, apps, 2, false);
    cp0_ui_add_app(ui, 42, "dev.cardputerzero.second");
    cp0_ui_handle_action(ui, CP0_UI_ACCEPT);
    cp0_ui_handle_action(ui, CP0_UI_DOWN);
    write_snapshot(directory, "apps", ui, frame);

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
    static const struct cp0_ui_catalog_app catalog[] = {
        {.running = false,
         .immersive = false,
         .app_id = "dev.cardputerzero.first",
         .name = "First Card"},
        {.running = true,
         .immersive = true,
         .app_id = "dev.cardputerzero.second",
         .name = "Second Card"},
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
