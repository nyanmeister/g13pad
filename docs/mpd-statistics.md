# MPD playback display

In the editor, click the screen, choose **Music (MPD): what is playing** under **Shows**, and Save/Use
the profile. Existing MPD profiles use the same `# mpd on` flag and migrate
without edits. The fixed 160×43 screen follows MPD even when Cantata is closed:

- **Artist - Title**, with case preserved and scrolling for long names. Missing
  tags use Unknown artist and the filename stem. Control characters are removed;
  the combined identity is bounded to 256 characters.
- **Qualified play count and last played date**, such as `23 PLAYS, LAST 2026-09-20`
  or `1 PLAY, LAST 2026-09-20`. Zero says `0 PLAYS, LAST NEVER`; unavailable
  history uses dashes. Large counts compact to K/M/B as space requires. Dates use
  the watcher's local timezone. The narrow built-in row uses uppercase glyphs.
- **Elapsed time and negative remaining time**, a pause icon and progress bar.
  Unknown durations remain unknown; hours appear for long recordings.
- **MPD-only audio level history**, sampled at 25 Hz across about 6.4 seconds.
  The amplitude histogram uses the microphone graph's -60 to 0 dBFS scale; it is
  a peak history, not a frequency spectrum. It freezes when paused/stopped.

The collector stays alive after first selecting MPD. Playing music continues
filling the graph while another profile is visible; returning restores it. The
editor temporarily owns LCD writes without stopping collection. Brief connection
failures retain the ring; confirmed Unix peer PID changes clear it. Restarting the
watcher starts a fresh graph. Playback metadata is cleared on disconnect. TCP
connections lack reliable peer process identity, so reconnects retain the ring.

History comes from the independent [g13music SQLite service](mpd-music.md), through
its bounded readonly `track` snapshot, only when URI and MPD endpoint both match.
The default qualifying rule is half the track or four minutes of actual listening,
whichever comes first; pauses and seek jumps do not contribute. Imported MPD
playcount stickers are retained as a baseline. A missing companion does not
prevent playback display, but its counts are unknown. MPD `stats.playtime` is a
separate daemon counter and is not used as a per-track play count.

The audio backend uses libpulse, including PipeWire-Pulse, to monitor exactly one
identified MPD playback stream. Unix socket peer PID is checked against stream or
owner-client properties when available. Ambiguous streams, remote MPD, unsupported
outputs or missing audio service produce gaps; there is no mic/desktop-mix fallback.
Native local PipeWire and Pulse outputs are supported without an MPD configuration
change. No PCM, media paths or audio recordings are stored by the display.

Unicode titles use `g13map-editor --lcd-row`, a bounded headless font helper that
runs once per changed title. It needs no display or GUI session. Installed fonts
provide optional script coverage (tested with WenQuanYi Japanese); without the
helper the watcher falls back to its ASCII bitmap font. The helper's transient
font atlas exits after rasterization; the watcher keeps only a small packed strip.

```sh
g13map profile mpd music on
g13map mpd preview /tmp/mpd.lpbm
g13music track
g13map mpd status
```

`mpd status` still prints daemon uptime, daemon listening time, library counts and
update age. Daemon listening time resets when MPD restarts. `mpd preview` writes a
960-byte now-playing LCD frame with roughly one second of newly collected audio.

The default endpoint is TCP `127.0.0.1:6600`. `MPD_HOST`/`MPD_PORT` can select a
numeric IPv4/IPv6 address or absolute Unix socket; `localhost` maps to loopback.
Other DNS names are unsupported to bound shutdown delays. libmpdclient handles
`password@address` authentication; no credentials enter profiles or snapshots.
The watcher and companion must inherit the same endpoint (service environments
can differ from your shell). No alternate-server fallback occurs.

Display ownership remains in `src/mpd/playback.rs`; native protocol transport is
in `src/mpd.rs`, the stream monitor in `src/audio/pulse.rs`, and Unicode helper
handoff in `src/mpd/playback/title.rs`. Listening/queue policy and SQLite remain
in `crates/g13pad-music`; the hardware driver has no history or audio policy.
This split allows later extraction of individual integrations.

Validation includes private real MPD/null audio, bounded API snapshots, native
permission/timeout/editor/profile tests, grammar/date/graph unit tests, five
visually reviewed screens including Japanese, and a private MPD playing into a
silent virtual sink alongside a much louder non-MPD stream. Native peak selection
and fake-FIFO pause/profile graph persistence were verified without changing the
real music queue. `tools/check-mpd.sh` and `tools/check-music.sh` cover the automated
native boundaries; opt-in peak and visual tests are ignored in ordinary CI because
they require an audio server or produce review artifacts.
