# MPD listening history and music controls

`g13music` is an independent, per-user Rust companion to MPD. The G13 driver
contains **no database, playback accounting or shuffle policy**. The companion
currently ships in the g13pad workspace/package for convenient installation; it
does not require a G13, a selected profile, Cantata, or an open editor.

Enable it with `systemctl --user enable --now g13music.service`. The daemon
observes the default MPD partition. It never changes playback, ratings or queue
order until a control is explicitly requested. It writes summary stickers while
collecting history. Music files and their timestamps are never modified.

In the G13 editor, select a G-key or thumb button and choose **Music**. The
available actions include play/pause, stop, next/previous track or album,
rating adjustments, favourite, bookmark/resume, random toggle, album shuffle and
smart album shuffle. Save the profile as usual. TOP retains its stick-mode role.
No existing profile gets bindings automatically.

The same controls work without the pad:

```
g13music --version
g13music status
g13music history 30
g13music shuffle-albums
g13music smart-shuffle
g13map music toggle
g13map profile bind Music G1 '>MPD:toggle;'
```

`history` reads SQLite without requiring the service and prints Unix start time,
title, recorded listening seconds, counted/partial status, and MPD URI.
Live status includes disconnected state and pending sticker writes.

## What is recorded

The default play threshold is **half of the track or four minutes of observed
listening, whichever comes first**, selected on 2026-10-12. Unknown duration uses
four minutes. Track starts and skipping forward do not count as plays. A paused
session remains open. A counted session counts once; an observed wrap from near
the end to near the beginning starts another session even if the queue ID is
unchanged. Stop, another track and a detected daemon restart end the session.

Listening duration comes from monotonic wall time corroborated by MPD elapsed
progress, rather than MPD's native `playtime` (which resets and can reflect
buffered audio). The collector samples once per second, queries only status and
the current song, and imports stickers only when first observing a URI.
It excludes pauses, seek discontinuities, long sampling gaps and disconnection
gaps. It deliberately does not reconstruct what happened while absent.

This is conservative **observation accounting**, not a claim of sample-accurate
listening measurement. Track transitions may lose up to one sampling interval;
very short tracks, a seek resembling natural progress, or whole tracks played
between observations cannot be distinguished. A backward seek from the last
2.5 seconds to the first 2.5 seconds resembles a repeat. Following a process
restart, an open session is reused when URI, queue ID and daemon epoch agree;
an elapsed reset after absence starts a new session, but unobserved repeats cannot be reconstructed. Document these limits when comparing
counts with another client or scrobbler. Muted outputs still advance MPD and are
recorded as playing; MPD cannot prove that a person heard the audio.

SQLite checkpoints every ten seconds and at count thresholds, track changes,
disconnects, control actions and graceful shutdown. SIGKILL/power loss can lose
up to ten seconds of recent observations. SQLite transactions use WAL and FULL
sync; threshold updates and their sticker outbox entries commit together.
No per-second event rows are stored, keeping memory, DB growth and writes small.

## Data ownership and compatibility

Default local database: `$XDG_STATE_HOME/g13music/history.sqlite`, falling back
to `~/.local/state/g13music/history.sqlite`. SQLite is the detailed source of
truth. Tables:

| Table | Responsibility |
| --- | --- |
| `tracks` | Internal integer identity, current URI and tags, counters, favourite, bookmark, cached rating |
| `sessions` | Start/last observation, observed duration, counted flag, queue ID/daemon epoch, close reason |
| `outbox` | Absolute sticker values awaiting MPD acknowledgement; one entry per URI/key |

`PRAGMA user_version=1` defines this schema. Newer schema versions are rejected.
Only one session can be open. A database lock prevents multiple collectors even
with different runtime directories. Native libmpdclient connections and all
borrowed pairs remain on the daemon's owning thread.

MPD's sticker database is accessed **only through its protocol**. The companion
never opens or edits MPD's live SQLite. It imports existing `playcount`,
`lastplayed`, `listeningtime` and `rating` when a URI first enters its local DB.
Summary projections are absolute `sticker set`, not increments: a crash between
MPD accepting a write and local acknowledgement can safely retry it.

| Sticker | Meaning / owner |
| --- | --- |
| `playcount` | Imported baseline plus locally observed qualified sessions |
| `lastplayed` | Unix seconds when the latest local session qualified |
| `listeningtime` | Imported baseline plus local observed listening, whole seconds |
| `rating` | Cantata-compatible 0–10; actions adjust by 2, preserving existing ratings |
| `favourite` | Companion toggle, separate from Cantata's stars |
| `bookmark` | Companion-saved elapsed seconds; resume applies to the current track |

Run **one play-count writer** per MPD collection. Another program independently
incrementing these same summary stickers will conflict with absolute projections.
Cantata remains the shared rating owner: rating actions read its current value,
and smart shuffle reads all current rating stickers in one MPD query. Missing
sticker support/permission does not prevent local listening history; pending
projections remain visible, with rate-limited diagnostics in the user journal.

The collector's internal track ID is currently **URI-based**, not embedded media
identity. A rename appears as another track. Do not claim rename preservation
until the later identity phase described below is implemented.

## Album operations

Both shuffles operate on the **current queue**, without enumerating the music
share, adding tracks or dropping queue entries. The prefix through the currently
playing song is retained. Remaining tracks from that album follow, ordered by
disc then track; the other albums are shuffled as groups with the same internal
ordering. Untagged albums use the parent directory; tagged albums use
MusicBrainz album/release ID where available, otherwise album artist, album name
and date. Fallback keys cannot perfectly distinguish incomplete/inconsistent tags.

Explicit shuffle disables MPD random so sequential album playback takes effect.
It leaves repeat unchanged and keeps the playing queue ID. Next/previous album
uses album boundaries in the current queue; normal MPD track controls retain
their native behaviour.

Smart shuffle gives an unrated track neutral preference, favours higher ratings,
reduces recently played tracks (seven-day freshness window), moderates frequently
played tracks, and boosts favourites. Album weight averages its tracks. Weighted
random ordering is followed by an artist-spacing pass when another artist is
available. This is a documented initial heuristic, not a learned taste model.
History before recording started is unknown unless imported from existing stickers.
The formula is in `actions.rs::album_order`; it can evolve independently of the
G13 protocol or DB schema.

The queue version/current ID are checked immediately before changing it.
Permutation swaps use queue IDs, not paths/positions, so duplicate files remain
distinct. Work is bounded to 20,000 queue songs, 32 MiB responses, eight-second
response deadlines and batches of 200 swaps. MPD lacks a conditional queue
transaction: another client editing during a shuffle may cause a partial
reorder/error. Entries are never intentionally removed. Retry after concurrent
editing stops; do not present this as an atomic queue replacement.

## Configuration and operation

Use a user service drop-in (`systemctl --user edit g13music.service`) for:

```
[Service]
Environment=MPD_HOST=127.0.0.1
Environment=MPD_PORT=6600
Environment=G13MUSIC_PLAY_FRACTION=0.5
Environment=G13MUSIC_PLAY_SECONDS=240
```

Threshold fraction must be greater than zero and at most one; seconds must be
greater than zero and at most 86400. MPD_HOST supports localhost, numeric IPv4/
IPv6 and absolute Unix sockets, plus libmpdclient's password@address syntax.
Avoid putting passwords in profile bindings or readable files. Custom DNS names
are rejected to keep reconnection bounded. Service environment differs from
interactive shell environment. Reconnection occurs every three seconds while
offline. No MPD configuration change or restart is required.

`G13MUSIC_DB` and `G13MUSIC_RUNTIME_DIR` allow isolated fixtures or a custom
local state location. Their parent directories must be owned/private (0700).
The action socket lives in `$XDG_RUNTIME_DIR/g13music/control.sock` by default,
is private to the user, accepts fixed allowlisted command names, and has bounded
requests/responses and deadlines. It cannot execute shell commands or arbitrary
SQL. The G13 watcher uses a bounded background worker, keeping LCD/UI responsive.

Disable with `systemctl --user disable --now g13music.service`; history remains
on disk. Existing G13 bindings then report an unavailable service. For a coherent
backup while running, use SQLite's `.backup`, not copying only the main file
while WAL is active. Restoring an older backup also restores older counter totals;
its next projections can overwrite newer MPD summaries. Stop the writer before
restoring and assess those totals. Uninstalling does not delete listening history.

## Boundaries for future extraction

| Layer | Current location | Stable boundary / extraction path |
| --- | --- | --- |
| Hardware driver | existing C++ action output | Emits opaque `MPD:COMMAND;` FIFO tokens; unchanged |
| G13 editor/profile | `src/gui.rs`, `profile.rs`, `keys.rs` | UI creates exact `>MPD:COMMAND;` tokens |
| G13 dispatcher | `src/music.rs`, `modes.rs` | Async allowlisted Unix-socket client; no collection or DB |
| Client action vocabulary | `crates/g13pad-music/src/lib.rs` | Small library; could become a separate protocol/client crate |
| MPD transport | `crates/g13pad-music/src/client.rs` | Native protocol wrapper, owning thread and bounded responses |
| Accounting/storage | `history.rs` | Observation engine + SQLite transactions/outbox; could be a standalone collector |
| Playback policy | `actions.rs` | Album grouping, weights, actions; independently replaceable |
| Service/CLI | `main.rs`, `packaging/g13music.service.in` | Runs without driver or Cantata; standalone package candidate |
| LCD stats | existing `src/mpd.rs` | Remains read-only and independent of history and controls |

No new action interpretation is embedded in the kernel-facing driver. Keeping
the companion in this workspace is packaging convenience, not a commitment to
these features belonging in the driver. A future extraction needs a small client
dependency/protocol copy and packaging dependency, rather than a hardware rewrite.

Future separate work: stable file identity and history-preserving renames;
format-native audio metadata carriers; optional LCD history view; library-wide
album selection/autofill; alternative shuffle policy; Cantata UI integration.
These are not prerequisites for the present service.

For identity, preserve memetag's distinction between persistent content identity
and whole-file edit/revision hashes. Store identity once with a supported audio
carrier, retain history locally, and migrate URI stickers after validated renames.
Do not rewrite audio on every listen. MP3/FLAC/Ogg/Opus carriers still require
format-specific research; memetag's current image/video carrier support is not
proof of music support. Copies/re-encodes/duplicates need an explicit policy.
Actual share mutations must run on the server with recoverable journal/locks,
restore original atime/mtime at filesystem precision and verify it. Preserved
mtime requires targeted MPD `rescan` after metadata changes; ordinary `update`
may skip them. Linux ctime generally cannot be preserved after a write.

## Cantata's hamburger menu

Cantata 3.5.0's Qt6 fork implements album shuffle already: classic menu
**Queue → Shuffle → Albums**; it affects the queue and also needs random off.
`Ctrl+M` toggles the classic menu bar; this workaround was verified with the installed 3.5.0 executable in a private Xvfb/private-MPD session. Its active configuration is `~/.config/Cantata/Cantata.conf`; the lowercase `cantata/cantata.conf` is the retained 2.x configuration. Its menu-button event filter clamps popup
X to `primaryScreen()->availableGeometry()`, rather than the button's screen.
On the observed right portrait monitor, that can move the popup left into the
primary monitor/empty desktop area. This is the leading diagnosis; the installed
Cantata binary has not been patched by this feature. A future fix should clamp
against the button's own screen, including negative desktop coordinates.

References: [Cantata menu positioning](https://github.com/nullobsi/cantata/blob/main/widgets/menubutton.cpp),
[album shuffle](https://github.com/nullobsi/cantata/blob/main/models/playqueuemodel.cpp),
[classic menu shortcut](https://github.com/nullobsi/cantata/blob/main/gui/mainwindow.cpp),
[MPD stickers](https://mpd.readthedocs.io/en/stable/protocol.html#stickers).

## Validation and maintenance

Version 0.2.84 passed all 51 package CTest checks, workspace Clippy with warnings
denied, Rust formatting and shell lint. The native music fixture starts an actual
private MPD with generated WAVs and null audio, then exercises collection,
pause/seek exclusions, imported counters, Cantata-scale ratings, bookmark and
favourite writes, graceful restart/singleton protection, both queue permutations,
album navigation, disconnected local history and SQLite integrity. Its optional
second argument tests the actual G13 watcher/FIFO dispatcher with M-Sum disabled.

```
cargo test --locked -p g13pad-music
sh tools/check-music.sh /path/to/g13music /path/to/g13map
```

The real editor was tested on private Xvfb/private D-Bus with fake driver FIFOs:
Music tokens auto-select the pane, all controls fit, choosing Rating +1 star
updates the label/token, and Ctrl+S persists `>MPD:rate-up;`. Tests do not require
desktop pointer movement or manipulating the real MPD queue/ratings. Private GUI
runtime/config/data/cache/state paths must be exported **before** dbus-run-session.

The deployment review/checkpoint lives under `~/Documents/algorithmic-reviews/MPD
Review/extensions-20261012/` on the development machine, including exact versions,
commits, installation state, rollback and empirical limits. Repository docs remain
the portable contract; private checkpoints carry machine-specific evidence.

## Readonly now-playing snapshot (0.2.85)

`g13music track` (socket command `track`) returns one JSON object containing
`uri`, normalized credential-free `endpoint`, SQLite `playcount`, and Unix
`lastplayed` (zero when never qualified). It uses the daemon's last validated
observation and a bound SQLite lookup; requesting it sends no MPD command and
performs no database/sticker mutation. Disconnect clears the observed URI. The
LCD rejects another URI/endpoint's snapshot rather than reusing stale counts.
The CLI keeps its normal control timeout; the LCD caps requests at 300 ms.

The [MPD playback screen](mpd-statistics.md) consumes this API without knowing the
SQLite schema. Native audio acquisition and presentation remain outside the
companion and hardware driver. This boundary is suitable for extracting later
features into separate discussions or tools.
