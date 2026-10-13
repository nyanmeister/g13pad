# System tools and audio

The editor's **Shows** list (click the screen) has the machine-wide displays under
**This computer**: **Computer load** and **Microphone and volume**. A profile shows one
thing at a time; the kept picture stays underneath.

With **Microphone and volume** chosen, pick the **Mic** in the editor. **System
default** follows the desktop's current default input; the selector shows which
device that resolves to. Pick a specific input to keep using it across changes
to the desktop default. The same selector is available in the **Sound** binding
tab, including profiles with no audio display. **Save** stores the choice in
this profile; **Revert** restores it. **Refresh** updates available inputs.
Output monitors are labelled explicitly, and hovering a device shows its identity.

The status display, graph and all mic bindings use that choice through PulseAudio
or PipeWire-Pulse. Output controls still follow the default output. Selecting an
input does not change desktop defaults. If a chosen device disappears, it stays
selected and shows red/unavailable until it returns; it never switches to another
microphone automatically. The microphone's actual system mute state sets the
pad's backlight: **red when muted, green when unmuted**. Green means the input is
open, including while silent; it does not promise that a particular program is
transmitting or recording. An unavailable input/server is also red, labelled
**MIC UNAVAILABLE**. Yellow is reserved for future program integration states.

The LCD shows microphone mute/gain, output mute/volume and the selected input's
description. Its bottom trace covers 6.4 seconds on a fixed −60 to 0 dBFS scale. It uses
server-generated peaks, not speech recognition. Gaps mean unavailable readings;
a marker at the top indicates a peak near full scale. The graph resets when the
default input changes. No audio recording or PCM history is retained.

Select a pad control and open **Audio** to bind mic mute/unmute, explicit mute or
unmute, mic level up/down, output mute or listening volume up/down. Each press
changes the observed selected microphone or default output. Levels move by five percentage points,
preserving channel balance; raises stop at 100% and leave existing boosted levels
unchanged. Actions work through the watcher even when M-Sum and window rules are
off, and do not require the System audio screen. This stage has no OBS/Mumble
recording, deafen, per-application mute or transmit-state integration.

**Save** keeps bindings and the display choice; **Revert** restores saved choices.
The editor has display/backlight priority. When it closes, audio repaints actual
state. Leaving System audio restores the new profile's normal backlight/picture.
Only profiles that select it open the microphone level stream.

```sh
g13map profile audio PROFILE on
g13map audio inputs
g13map profile mic PROFILE SOURCE_NAME  # or default
g13map profile bind PROFILE G1 '>AUDIO:mic-toggle;'
g13map profile bind PROFILE G2 '>AUDIO:volume-down;'
g13map profile bind PROFILE G3 '>AUDIO:volume-up;'
g13map use PROFILE
g13map audio status
```

CLI controls: `mic-toggle`, `mic-mute`, `mic-unmute`, `mic-up`, `mic-down`,
`output-toggle`, `volume-up`, `volume-down`. `g13map audio preview FILE.lpbm`
captures a brief level preview without writing to the pad. Native libpulse
requires no audio service for `--version`, other displays or saved profile edits.

For a native monitoring sanity check, the opt-in Rust test
`peak_stream_keeps_device_queries_responsive` keeps the default input's level
stream and device queries running for 40 seconds. It reads levels without storing
PCM or changing mute, gain or routing. Run it explicitly with `cargo test
--release peak_stream_keeps_device_queries_responsive -- --ignored`, using the
intended audio server. Ordinary tests do not open a microphone.
