#ifndef CP0_AUDIO_SETTINGS_CLIENT_H
#define CP0_AUDIO_SETTINGS_CLIENT_H

#include <stdbool.h>

enum cp0_audio_settings_result {
    CP0_AUDIO_SETTINGS_OK = 0,
    CP0_AUDIO_SETTINGS_FAILED = -1,
    CP0_AUDIO_SETTINGS_UNAVAILABLE = -2,
};

enum cp0_audio_settings_direction {
    CP0_AUDIO_SETTINGS_DECREASE = 0,
    CP0_AUDIO_SETTINGS_INCREASE = 1,
};

struct cp0_audio_output_state {
    bool available;
    bool muted;
    unsigned int volume_percent;
};

int cp0_audio_get_output_state(struct cp0_audio_output_state *state);
int cp0_audio_set_output_volume(unsigned int percent,
                                struct cp0_audio_output_state *state);
int cp0_audio_adjust_output_volume(enum cp0_audio_settings_direction direction,
                                   struct cp0_audio_output_state *state);
int cp0_audio_set_output_muted(bool muted,
                               struct cp0_audio_output_state *state);
int cp0_audio_play_key_click(void);

#endif
