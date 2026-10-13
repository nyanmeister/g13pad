# Controller bindings

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## Controller bindings

See [controller bindings](docs/controller-bindings.md). `G13_PAD_SEQ` and the
core's `gamepad::BUTTONS` share the fixed private channel order. TOP uses the
same refcounted channel as board buttons; mapping its old BTN_EXTRA to the same
xboxdrv output would let either release cancel the other held button. Disabling
TOP must not disable G-key L3. Keep modern west/north X/Y output codes explicit:
xboxdrv's historical aliases and jstest's printed names use the old convention.
The native `tools/controller-proof.rs` checks real xboxdrv in the isolated VM;
driver action proofs check shared holds and releases across rebindings.
