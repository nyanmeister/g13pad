# Iterating without packaging

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## Iterating without the package cycle

- **An isolated editor needs the whole sandbox set, not just the config.** `XDG_CONFIG_HOME`
  alone leaves the editor on the real `/run/g13d/g13-0` pipe and the real editor lock: it
  pushes its picture to the live pad and pauses the watcher (2026-10-12, one minute on the
  owner's pad). Use what `test_support::Sandbox` sets: `G13MAP_CONFIG`, `G13MAP_PIPE`,
  `G13MAP_OUT_PIPE` (with a reader draining the pipe), `XDG_RUNTIME_DIR`, `G13MAP_UNIT=0`,
  `G13MAP_ANALOG=1`, `G13MAP_ANALOG_MAP`; `tools/check-board-gui.sh` is the model.
- **`g13map edit` execs the sibling `g13map-editor`.** `cargo build --bin g13map` alone
  leaves the old editor binary in place; build `--bin g13map-editor` too and check its
  `--version` or mtime before believing a GUI check.
- **Build packages under a path without spaces** (`/tmp/g13-package-VERSION`): the
  GoldSource checks pass `LD_PRELOAD` paths, which split on spaces.
- **A row of labels wider than the 340 px pane squeezes the whole pane off the window**;
  `horizontal_wrapped` for any such row. Clicks from xdotool reach nothing on a bare Xvfb
  until `xdotool windowfocus`.
- `~/.config/g13map/meter` holds the look (the band ladder and its colours, the dark hold, beat
  spacing, the swell, the flash, where the heart and the readout sit, the CS2 port) and is re-read within two seconds while the meter runs.
- `tools/dev-watch.sh` runs `g13map-watch.service` from a warm `cargo build --release`
  through a systemd drop-in; `off` restores the installed package. Build the package only
  for what stays.
- `tools/fakedaemon.pl FIFO LOG` stands in for the daemon's pipe reader and logs every
  frame and command it gets; with `G13MAP_PIPE`, `G13MAP_OUT_PIPE`, `G13MAP_CONFIG` and
  `XDG_RUNTIME_DIR` pointed at a scratch directory, `g13map watch` plus `g13map health`
  exercise the whole path without hardware.
- `G13MAP_METER_DUMP=DIR cargo test --lib meter::dump -- --ignored` writes frames of
  every state as PBM files; `magick montage` makes a contact sheet to look at before
  anything reaches the glass.
- Log the feed while playing (a loop that records the file on change with a timestamp):
  it tells whether an oddity is the game, the feeder or the panel.
