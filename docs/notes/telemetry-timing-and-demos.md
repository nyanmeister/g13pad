# Telemetry timing and demos

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## Telemetry timing is separate from display timing

Temporary file demos use their own runtime `.demo` lock, never the blocking editor
lock. Normal LCD writers keep the latest restore frame while the demo owns the
display; editor and error overlays retain priority. Set `.demo-pending` before the
first frame and clear it only after successful restoration. The watcher checks it
every 500 ms after the kernel releases the demo lock, including SIGKILL. Watching
only lock transitions misses a process that starts and dies between polls. Use
`tools/check-demo.sh` for actual lean CLI/helper/watcher and signal checks on private
FIFOs; `g13map demo --clear` restores a saved picture without a watcher.

**The G13 can display a new frame every 10 ms (up to 100 FPS), as reported
2026-10-10. A 100 ms telemetry poll does not limit animation to 10 FPS.** Special
events can animate from a single event without new live data. Keep collectors,
presentation clocks and USB input timeouts independent; skip missed frames instead
of accumulating a backlog. Actual throughput depends on the transport and driver.
See [the developer module map and extension contract](docs/health-architecture.md).

Tests that mutate process environment must use `test_support::Sandbox`, including
temporary `G13MAP_UNIT=0` changes. A session test that set/removed it without the
shared guard raced the cross-process overlay fixture (2026-10-10); serializing the
test and restoring the previous value through Sandbox fixed the demonstrated race.
