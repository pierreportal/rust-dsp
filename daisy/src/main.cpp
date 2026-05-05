#include "daisy_seed.h"
#include "dsp.h"
#include <math.h>

using namespace daisy;

DaisySeed hw;
VoiceWrapper voice;

float midi_to_freq(int note) {
    return 440.0f * powf(2.0f, (note - 69) / 12.0f);
}

void AudioCallback(float** in, float** out, size_t size)
{
    for (size_t i = 0; i < size; i++)
    {
        float sample = voice_process(&voice);

        out[0][i] = sample;
        out[1][i] = sample;
    }
}

int main(void)
{
    hw.Init();
    hw.StartAudio(AudioCallback);

    voice_init(&voice, 48000.0f);

    while(1)
    {
        // simulate note trigger (replace with MIDI later)
        note_on(&voice, midi_to_freq(60));
        System::Delay(500);
    }
}
