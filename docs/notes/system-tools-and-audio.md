# System tools and audio

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## System tools and audio

What the LCD shows is one choice per profile, `profile::Display` (since 2026-10-12: the
kept picture, or one live display), listed in the editor under **Shows**; the
machine-wide ones sit under **This computer**. Each live display keeps its own comment
line on disk (`# pressure on`, `# audio on`, ...); the saved picture stays kept.

Microphone choices are per-profile stable source names in JSON-escaped
`# audio-mic "NAME"` comments. Missing chosen devices stay unavailable/red; never
fall back to the system default. Display, graph and mic bindings must share the
same choice, including controls-only profiles. Enumerate inputs off the GUI thread
without opening peak streams; mark output monitors to prevent accidental selection.
Audio's native libpulse client
also works with PipeWire-Pulse. Its callbacks and boxed state stay on one owning
thread, with context/stream destruction before state destruction. Peak streams
store levels only. A missing level stream must not discard a known mute state.
Audio bindings use exact `>AUDIO:COMMAND;` tokens through the existing driver output
FIFO; the watcher reads it even when M-Sum is off. Test controls against an isolated
audio server or virtual devices, never mute the user's host microphone to test.

MPD playback uses compatible `# mpd on` under System tools. Native readonly
status/currentsong queries stay on one owning thread; bounded companion `track`
snapshots must match both URI and daemon endpoint. Never enumerate the share or
control playback from the display. Keep missing history unknown. The retained
collector survives profile changes; a synchronous visibility ack retires its LCD
writes before handoff. Editor ownership suppresses writes, not collection.
Audio capture is a server peak monitor of exactly one identified MPD sink input,
optionally pinned to the Unix socket peer PID via stream/client properties.
Never fall back to a microphone, a desktop mix or an ambiguous MPD stream.
Pauses/stops freeze the ring, hidden profiles continue it; transient reconnects
retain it and confirmed peer PID changes reset it. All native Pulse callbacks
and boxed state stay on one owning thread. Rasterize Unicode titles once in the
bounded headless editor helper; keep only the packed strip in the watcher.
Legacy `mpd status` retains daemon/library statistics. Test private sockets,
null audio, fake LCD FIFOs and silent virtual audio devices; never change the
user's live queue/audio routing. See [MPD playback](docs/mpd-statistics.md).

## Native audio mainloop

The native audio mainloop must drain a bounded batch of ready events before the
worker sleeps. A single `pa_mainloop_iterate` per 20 ms fell behind PipeWire-Pulse
peak traffic and timed out device queries after 10–16 seconds despite a present mic.
The opt-in `peak_stream_keeps_device_queries_responsive` test exercises 40 seconds of
real server traffic; run it explicitly against a suitable test input/server.
