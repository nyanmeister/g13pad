# G13 Analog in Steam Input

`packaging/steam-g13-analog.vdf` is an optional reusable template named **G13 Analog**.
The package stages it at `/usr/share/g13pad/steam-g13-analog.vdf`; installation does not
modify Steam or your game profiles.

It routes virtual left/right sticks to continuous gamepad joystick outputs. L3/R3,
A/B/X/Y, bumpers, Back and Start pass through. The G13's board still emits the keyboard
bindings configured in g13map. Guide is normally reserved by Steam. Stick calibration
and dead zone remain in the analog adapter; the template requests no extra inner dead
zone. Different game/client settings may add their own processing.

Copy it into your Steam client's `controller_base/templates` directory as
`controller_generic_g13_analog.vdf`. On a standard Linux Steam installation:

```sh
cp /usr/share/g13pad/steam-g13-analog.vdf \
  ~/.local/share/Steam/controller_base/templates/controller_generic_g13_analog.vdf
```

After Steam refreshes its templates (a client restart may be needed), select **G13 Analog**
from the game's controller layout Templates tab and apply it. Enable Steam Input for
that game, and set its g13map profile to analog. Save the g13map profile separately.
The template is a local reusable entry; it has no published community-layout ID. Steam
updates may replace client files, so keep the supplied copy as the source for reinstallation.

Check both axes and the selected click, then switch focus away and back. Steam's log
`logs/controller_ui.txt` should load the selected layout instead of a keyboard/WASD
template. This proves selection, not successful game input. Confirm variable movement
speed/direction in the game itself, including simultaneous board keyboard and mouse use.
To roll back, select your previous layout/Steam Input override and saved g13map stick mode.

During the 2026-09-30 CS2 investigation, Steam's fallback keyboard template reloaded on
focus changes and translated right into a layout-dependent voice key. A prior standard
gamepad-template trial loaded successfully but produced no movement in CS2. The virtual
G13 controller's continuous axes and SDL gamepad mapping were independently confirmed;
CS2's native controller path remained unresolved. After a client restart, **G13 Analog**
was verified in the Templates tab and applied specifically to the G13. Steam saved the
device-specific selection and loaded it again on CS2 focus, logging gamepad output
(`xinput: true`). A physical test still produced no stick movement or click response,
before or after refocusing; the working digital g13map profile was restored.
That initial result was superseded by the successful local test below.
The separate digital fallback, using physical `KEY_D` for Norman's E/right movement,
was confirmed by the user to work across focus changes.

A comparison in Deep Rock Galactic confirmed partial/full analog movement and
stick-click with its existing Steam Gamepad layout and g13map analog profile. That
layout was preserved. This establishes a working G13/adapter/Steam input path in that
game; a physical test of the named G13 Analog template in Deep Rock remains separate.

## Counter-Strike 2: confirmed local analog movement

Later user-assisted tests on 2026-09-30 confirmed continuous movement with the named
template. Three separate boundaries mattered: initializing controller input, binding
its movement axes, and preserving their magnitude at the server.

- Start CS2 with `-joy` so its controller input initializes. Valve documents this
  flag in its [May 29, 2024 release notes](https://store.steampowered.com/news/posts/?appids=730,240,10,80&enddate=1717536833&feed=steam_community_announcements).
- In the game console, set `bind "X_AXIS" "rightleft"` and
  `bind "Y_AXIS" "!forwardback"`. These bindings restored movement in the physical test.
- On a local practice server, `sv_quantize_movement_input 0` restored partial-to-full
  movement speed. With quantization enabled, the server rounds movement; a client
  template cannot override that rule. Valve describes the setting in its
  [November 27, 2024 release notes](https://store.steampowered.com/news/posts/?appids=730&enddate=1733260778&feed=steam_community_announcements).

The successful trial also retained `-console -condebug +cl_joystick_enabled 1
+joystick 1 +joy_advanced 1 +joy_advaxisx 3 +joy_advaxisy 1`. Their individual necessity
was not tested, so the shorter setup above is not a verified minimal launch recipe.
The local server command does not establish persistence across server starts or
permission to change a remote server's rule. Public-server settings were not surveyed.

The user ultimately chose the saved `cs-digital` profile for CS2; its window rule and
the separate analog profile were preserved. An inverted outer-ring walk binding was
discussed as a two-speed alternative, but was not implemented or physically tested.

Valve documents the distinction between keyboard emulation and gamepad joystick outputs
in [input source modes](https://partner.steamgames.com/doc/features/steam_controller/input_source_modes)
and [gamepad emulation](https://partner.steamgames.com/doc/features/steam_controller/steam_input_gamepad_emulation_bestpractices).
