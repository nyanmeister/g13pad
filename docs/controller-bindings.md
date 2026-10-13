# Controller bindings

G13pad 0.2.78 can send controller buttons from the G-keys and thumb buttons through
the same Xbox-compatible controller as the analog stick. Install the matching
driver, helper and service files together, with xboxdrv available. Existing
keyboard profiles and stick calibration remain compatible.

Select a control on the editor's board, open **Controller**, then pick a button.
This selects analog mode for the profile and applies the binding immediately;
**Save** keeps it. Other controls may keep their keyboard bindings. **Revert**
restores the saved bindings and stick mode. TOP remains the stick click configured
in the stick's own pane (click the stick on the board), which also selects the left/right
output stick and axis mapping.

The controller picker offers A/B/X/Y, LB/RB, LT/RT, all four D-pad directions,
Back/Start/Guide and L3/R3. LT and RT are digital: fully pressed while held and
released afterward, reported as controller trigger axes. Opposite D-pad directions
cancel on their axis; diagonals work. Multiple keys can share a controller button:
it remains pressed until the last of those keys is released. The release of a key
held across a profile change releases its original button.

The CLI saves the same bindings:

```sh
g13map profile create controller gaming
g13map profile bind controller G1 PAD_A
g13map profile bind controller G2 PAD_LT
g13map profile bind controller G3 PAD_UP
g13map use controller
```

Tokens are `PAD_A`, `PAD_B`, `PAD_X`, `PAD_Y`, `PAD_LB`, `PAD_RB`, `PAD_LT`,
`PAD_RT`, `PAD_UP`, `PAD_DOWN`, `PAD_LEFT`, `PAD_RIGHT`, `PAD_BACK`, `PAD_START`,
`PAD_GUIDE`, `PAD_L3` and `PAD_R3`. The CLI selects analog mode when saving one.
Hand-edited profiles should include `# stick analog`. A controller binding sends
no keyboard key; choosing a keyboard binding replaces it. If every gameplay
binding uses controller buttons, games receive controller input throughout.
M-Sum profile selectors may still use the physical M-keys. Legacy jstest names
its buttons using old Linux aliases: modern X is button 3 and Y is button 2.
Check the emitted positions/codes rather than those historical label names.

Bindings belong to their profiles, so M-Sum and focused-window profile changes
apply them through the existing profile path. In keyboard stick mode the adapter
is stopped and controller bindings have no gamepad output. Games may need to
enable controller support or select the device in their settings. Existing
Steam Input setup applies to this controller too.

## Adapter interface

The driver emits private `BTN_TRIGGER_HAPPY1` through `17` channels for `PAD_*`
actions. They do not add standard gamepad button capabilities to the keyboard
source. The fixed ordering is shared by `G13_PAD_SEQ` and
`g13pad_core::gamepad::BUTTONS`; xboxdrv maps them to its controller alongside the
stick axes. X and Y use the modern Linux west/north button codes instead of
xboxdrv's historical X/Y aliases. TOP is bound to the selected `PAD_*` channel during adapter preparation,
so a G-key and TOP sharing a button use the driver's shared hold count. Disabling
TOP binds it to `KEY_RESERVED`, leaving G-key L3/R3 bindings available.

The source is read without an exclusive grab, preserving keyboard bindings on
the board. Mouse buttons such as `MEXTRA` remain mouse actions rather than
controller clicks. No timer or additional background process is added.

The native adapter check is `tools/controller-proof.rs`, built with `rustc` and
run in the isolated x86_64 Arch-Virtual test guest. It creates its own input
source and checks real xboxdrv button/axis state without desktop input. The
driver's action proof separately checks shared physical holds and rebindings.
