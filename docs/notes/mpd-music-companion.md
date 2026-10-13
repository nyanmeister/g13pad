# Mpd music companion

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## MPD music companion boundary

The driver emits opaque `MPD:COMMAND;` tokens only. Music bindings use exact
`>MPD:COMMAND;` strings; watcher dispatch remains asynchronous and independent
of M-Sum/LCD selection. Collection, SQLite, sticker projections and queue policy
belong to the standalone `g13music` user service in `crates/g13pad-music`, not the
stats collector or hardware driver. See [ownership/schema/limits/extraction](docs/mpd-music.md).
Test with private MPD/null audio and local music fixtures; never manipulate the
user's live queue or ratings to validate controls. Music files are untouched in
this phase; embedded identity/mtime-preserving rename support remains separate.
