#pragma once

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    float _private[32];
} VoiceWrapper;

void voice_init(VoiceWrapper* v, float sample_rate);
float voice_process(VoiceWrapper* v);
void note_on(VoiceWrapper* v, float freq);
void set_cutoff(VoiceWrapper* v, float cutoff);

#ifdef __cplusplus
}
#endif
