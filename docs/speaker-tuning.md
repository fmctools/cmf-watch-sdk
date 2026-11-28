# Speaker tuning (CMF Watch Pro 2)

How music is shaped for the watch's speaker, and why. Code: `src/wp2apps/tune.py` (the phone or Mac side,
`TUNING`), `src/wp2apps/play.py` (the stream). Loader side: docs/abi-1.4.md, "Audio".

Everything here was measured on a real watch (JX402, `1.0.0.73`) on 2026-11-21, from phone recordings 5 cm from the
speaker, and tuned by ear over 23 recordings. Mac-mic measurements were not usable (room noise, distance).

## What the speaker does

| Measure | Result |
|---|---|
| Response | -30 dB at 100 Hz, -14 dB at 200 Hz, -7 dB at 250 Hz, ~0 dB at 320 Hz; a **+6 to +12 dB hump over 500-1300 Hz** (resonance: the "tinny" sound); flat 1.3-4 kHz |
| Bass | Under ~250 Hz the driver only shakes: harmonics 10-14 dB under the tone; a buzz at -20 dB over 200-450 Hz |
| Level (track at 80) | 400, 1600, 3200 Hz clean up to -6 dBFS, then the output drops (amplifier protection); 800 Hz saturates at -18 dBFS; 250 Hz collapses above -15 dBFS |
| Volume | **The stock track level is all or nothing**: `audio_volume` either plays at full or not at all |

So most of a song's energy (under 150 Hz) cannot be played, and feeding it anyway makes the driver distort.

## The chain (`tune.for_speaker`)

In order, on 16 kHz mono before it is encoded:

1. **Loudness levelling** (optional, the "enhance" option): gated RMS to -14 dBFS, gain limited to -6..+12 dB, so
   songs sound alike and the compressor works the same on each.
2. **Virtual bass** (0.9): the band under the cut through a soft nonlinearity, band-passed to 380-900 Hz. The ear
   hears the missing fundamental from its harmonics; landing above 380 Hz keeps it out of the 200-450 Hz buzz.
3. **Cut** at 160 Hz, 2nd order: what would only shake the driver.
4. **Body**: +6 dB peak at 420 Hz (q 0.9).
5. **Resonance dip**: -5 dB at 900 Hz (q 0.8), against the hump.
6. **Top**: -4 dB high shelf from 2.8 kHz.
7. **Dynamic EQ on 0.9-1.8 kHz**: above -18 dB, 4:1 (5 ms attack, 150 ms release). It catches the hump only when a
   song pushes it.
8. **Compressor**: -24 dB, 2.5:1, then make-up to the ceiling.
9. **Low band limit**: under 500 Hz soft-limited (tanh) at 0.125 of full scale. The driver runs out of travel there
   first, and a hard clip would buzz.
10. **Ceiling**: 0.5 of full scale, above which the amplifier's protection cuts in.

Result (recording 23): 600 Hz-5 kHz within ±2 dB of the original song. The direction that was preferred: clean with
body, not bright.

## On the watch

What an app playing the stream should do (the receiver is not part of the SDK; this is what was tested):

- **Rate**: a Mac's BLE link carries about 0.78x of a 16 kHz ADPCM stream, so the stream is sent at **10.67 kHz**
  (2/3 of 16) and upsampled 2 → 3 on the watch.
- **Anti-image low-pass**: a 4th-order Butterworth at 4.8 kHz after the upsampling. Without it, the mirror image at
  5.3-8 kHz plays as harsh, noisy treble.
- **Exciter**: above 5.5 kHz, 40/128 by default, to restore some air the low-pass took. 160/128 puts 5.5-8 kHz near
  -23 dB, where the original song has it. It is sent with the stream (`x amount`).
- **Volume**: keep `audio_volume(100)` and scale the samples (Q15, squared so the crown's steps sound even), because
  the stock level is all or nothing.
- **Pacing**: the app reports credit (`F next:u16 room:u8`) and the host sends only what fits, so the stream follows
  the watch's DAC clock. Keep about 1 s ahead of the loader's 6 KiB queue: a real link has slow moments.

## The stream (`wp2apps play`)

Phone or Mac → watch: `o khz up` open (`up` 1: 2/3 rate, upsample), `a seq:u16 block` one IMA ADPCM block
(`cmf_wp2_app::adpcm`, 122 code bytes), `v percent`, `t title`, `x amount`, `c` stop. Watch → phone: `R` ready,
`V percent` after a crown turn, `F next:u16 room:u8` credit. The host sends on a sliding window (`Pipe.stream`):
stop-and-wait batches managed only 0.68x of real time.

## Retuning

Change `TUNING` in `tune.py` (each key is a step above) and check by ear: a phone recording right next to the watch,
compared with the source. Start from the measurements above, not from a Mac mic.
