#ifndef CP0_UI_H
#define CP0_UI_H

#include <stdbool.h>
#include <stdint.h>

#define CP0_UI_WIDTH 320
#define CP0_UI_HEIGHT 170

enum cp0_ui_screen {
    CP0_UI_HOME,
    CP0_UI_APPS,
    CP0_UI_DEVICE,
    CP0_UI_NETWORK,
    CP0_UI_TASKS,
};

enum cp0_ui_action {
    CP0_UI_UP,
    CP0_UI_DOWN,
    CP0_UI_LEFT,
    CP0_UI_RIGHT,
    CP0_UI_ACCEPT,
    CP0_UI_BACK,
    CP0_UI_GO_HOME,
    CP0_UI_SHOW_TASKS,
    CP0_UI_SHOW_POWER,
};

enum cp0_ui_event {
    CP0_UI_EVENT_NONE,
    CP0_UI_EVENT_SLEEP,
    CP0_UI_EVENT_RESTART,
};

struct cp0_ui {
    enum cp0_ui_screen screen;
    unsigned int selected;
    unsigned int dialog_selected;
    bool power_dialog;
    bool network_online;
    int battery_percent;
    char clock_text[6];
};

void cp0_ui_init(struct cp0_ui *ui);
void cp0_ui_set_status(struct cp0_ui *ui, const char *clock_text,
                       bool network_online, int battery_percent);
enum cp0_ui_event cp0_ui_handle_action(struct cp0_ui *ui,
                                        enum cp0_ui_action action);
void cp0_ui_render(const struct cp0_ui *ui, uint32_t *pixels, int width,
                   int height, int stride_pixels);

#endif
