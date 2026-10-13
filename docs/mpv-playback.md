# mpv playback on the G13

Click the screen in the editor and choose **mpv: what is playing** under **Shows**, then
Save/Use that profile. A profile shows one thing at a time; your saved image stays kept
and returns with **Picture**. Window rules can select a separate mpv
profile; another player such as Cantata can keep its own profile.

Install the passive mpv script once (the package supplies the source):

```sh
mkdir -p "${XDG_CONFIG_HOME:-$HOME/.config}/mpv/scripts"
cp /usr/share/g13pad/mpv/g13-lcd.lua "${XDG_CONFIG_HOME:-$HOME/.config}/mpv/scripts/g13-lcd.lua"
g13map profile create mpv music
g13map profile mpv mpv on
g13map focus set mpv mpv
```

New mpv processes load the script automatically. Existing processes need a restart.
It leaves playback, input bindings, OSC and video settings alone. To remove it,
delete only that script and turn off the profile's Program integration tickbox.

The 160×43 display has fixed rows:

- Codec, source height and **actual** decoding route (`SW` means software).
- `D` decoder drops and `V` video-output drops over the last second, followed by
  the current A/V timing difference in milliseconds. Seeking/file changes reset
  the recent-drop baseline. Drops are observations, not necessarily faults.
- `BUF` approximate buffered seconds and `GPU` the sum of average fresh-frame
  render-pass timings. GPU timing excludes decoding, presentation and unreported
  work; it is not whole-frame latency. Missing properties show dashes.
- Elapsed media time on the left and negative remaining media time on the right,
  above a progress bar spanning the entire bottom. Times are media time, without
  playback-speed scaling. A small pause symbol appears above the elapsed time;
  pausing freezes playback position. Seeking updates the
  bar directly. Unknown duration leaves an unfilled track and unknown remaining
  time; hours appear when necessary. Times beyond 99 hours show dashes.

The script writes atomic, bounded JSON snapshots four times per second under
`$XDG_STATE_HOME/g13map/mpv/` (default `~/.local/state/g13map/mpv/`), one per PID.
It removes its own snapshot on exit. Crashed-player snapshots expire after three
seconds. The watcher prefers a focused player, then the newest snapshot; equal
timestamps use PID for stable selection. A waiting display appears without a
live feed. Multiple background players cannot be identified by window class
alone; focus is supplied by mpv itself when its video output supports it.

Tests can override the script directory with
`--script-opts=g13-lcd-feed-dir=/private/test/mpv`. The feed's `build` field and
`script-message g13-lcd-version` identify the loaded script version. Collection
continues while the editor owns the LCD, and the watcher resumes after it closes.
The feeder never takes the editor lock or writes to the daemon directly.

Property semantics: [mpv manual](https://mpv.io/manual/stable/#properties).
