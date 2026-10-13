# Audio

`engine/src/{sdat,siren,snd,audio}.rs` play the game's music, sound effects and voice from the extracted files alone.

| module | role | verified against |
|---|---|---|
| `sdat.rs` | SDAT archive: sequences (SSEQ), sequence archives (SSAR), banks (SBNK), wave archives (SWAV/ADPCM), players | `tools/sdat.py` (1,791 summary lines: every wave's decoded samples and loop, every bank's instruments, all sequences, all 5 archives, the players) |
| `siren.rs` | Siren14 / G.722.1C voice decoder, tables from `content/engine/g7221.pack` | `tools/siren14dec.c` (itself bit-exact against the game's decoder): 65 clips, 0 mismatches |
| `snd.rs` | sound driver: sequencer, tracks, channel allocation, envelope, LFO, portamento, mixer | the game's own ARM7 driver code (decompiled for reference); note stream identical to `tools/sseq.py` on all 151 sequences; 14 unit tests with a synthetic bank |
| `audio.rs` | `AudioDirector`: host effects -> driver calls; dialogue pacing; offline timeline renderer; real-time output (cpal) | integration tests on real content |

## The driver
One driver frame is 5.2095 ms (2728 ticks of the 523 656 Hz timer, 191.96 Hz).  Per frame every running sequence advances `tempo / 240` ticks (commands execute on
ticks, 48 per quarter note), then each channel updates its envelope, LFO and portamento and derives volume, pan and pitch.  Between frames the 16 channels (PCM 0-15,
PSG pulse 8-13, noise 14-15; the player's channel mask from the SDAT decides which a sequence may use) mix their waves at the device rate.  Facts taken from the driver code:
- volume is a sum of centibels with the squared law: `sq_db(v) = max(-722, round(400 log10(v/127)))` for player volume, track volume, expression and velocity, plus the envelope; channel gain = `10^(cB/200)`, pan splits `(127-p)/127` and `p/127`
- envelope amplitude is in cB x 128, starting at -723 cB: attack `amp = -((-amp * a) >> 8)` per frame (`a = 255 - attack`, or the fast table above 108), decay subtracts a rate until the sustain level `sq_db(sustain)`, release subtracts until -723 cB frees the channel; rates `1 + 2v` (< 50), `0x1e00 / (126 - v)`, 0x3c00, 0xffff
- pitch is in 1/64 semitone: `(key - original key) * 64 + portamento sweep + bend * range * 64 / 128`; PSG base timer 0x1f46 (middle C at pitch 0); LFO: 128-step sine, speed/4 steps per frame, delay in frames, type 0 pitch (x64), 1 volume (x60), 2 pan (x64) >> 14
- channel stealing: the allowed channel (search order 4 5 6 7 2 0 3 1 8 9 10 11 14 12 15 13) with the lowest priority (then quietest) loses to a new note of equal or higher priority; priority = player priority + track priority; a releasing note has priority 1
- the PCM wave is decoded once; looping is by sample index (ADPCM state at the loop point is the same as on the first pass); playback is linearly interpolated at the device rate (the hardware holds samples)
- the sequence's configured volume is applied as a level offset (`info.volume * 106 / 128`, the game's `0x69` factor); fades requested by the game are a gain ramp on the whole sequence

## Script sound commands (from the GameSnd_* code)
`bgm id [fade]`: SSEQ number (1000 = per-heroine BGM `bgm_tables.json`, 1001 = by mode; the same id already playing is ignored); `se arc idx` / `ambient arc idx fade`: sub-sequence `idx`
of sequence archive `arc` (0..4: 70, 101, 198, 26, 19 entries; 226 of the 232 pairs used by scripts play, the rest are mute dummies: volume 0 or the unused SYS player; 2 pairs are out of range in the scripts);
`stop_seq` / `stop_ambient`: fade out; `voice clip`: Siren14 clip (index into `Siren14Voice.vtb`, 40553 is a silent placeholder).  Dialogue pacing (`audio::pace`): a line starts after the
previous one finished and lasts its voice clip plus 0.4 s, or an estimated reading time; its voice starts with it.

## Tools
`audio_render CONTENT out.wav --bgm N | --arc A:I | --voice N` renders one sound; `scene_audio CONTENT EVENT_ID [--wav out.wav] [--play]` renders or plays a whole script event
(transcript + paced timeline); `sdat_dump`, `siren_dump`, `note_dump` are the differential harnesses (see `tools/loader_check.sh`).

## Not done
Streams (381 ADPCM streams: puzzle-mode voices) are not played yet; the hardware's sample-and-hold and 16-bit timer rounding are not reproduced (linear interpolation is used);
output levels are mastered with a fixed gain (2.0) and the voice/music balance is a guess; the game-side volume/fade commands other than stop (`fade_bgm`, `fade_channel1`, `fade_ambient`) are not applied.
