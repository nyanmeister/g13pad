# Obs overlay

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

## The OBS overlay and the driver's state files

`g13map obs` (src/obs.rs, 0.2.39) draws the pad for an OBS window capture. What it
took, for the next thing that wants to show the pad or the glass on a monitor:

- **The input-overlay plugin cannot drive it.** Its gamepad path (SDL2
  `SDL_GameController` in the installed 5.1.0; SDL3 `SDL_Gamepad` upstream) stops at
  the 21 mapped buttons, and its keyboard path (libuiohook) sees the profile's bindings,
  not the keys, so a key-code layout would change with every profile and mode. The
  overlay is therefore our own window, with the plugin's *asset shape* kept (a PNG
  sheet, a JSON layout, pressed sprite 3 px below) so the art can be repainted the
  same way as the plugin's presets.
- **The driver publishes its state as files beside the pipes**, like the health feed:
  `g13-0_keys` (`stick X Y`, `backlight R G B`, `keys NAME...`; the text is rewritten
  atomically on change, from the raw report bits in `G13_KEY_STRINGS` order) and
  `g13-0_lcd` (the 960 bytes last sent to the glass). Not the output FIFO: `g13map
  watch` already reads that, and a FIFO splits its bytes between readers. The names
  are set at the top of `RegisterContext`, because `LcdInit` sends the logo and
  `SetKeyColor` runs before the pipes exist and both are state. A proof
  (`tools/driver-keystate-proof.cpp`) pins the bit order; it is exempt from the
  `--wrap=write` the other proofs link with.
- **The sheet is generated from the editor's board** (`board::spots`, the traced
  outline) on a 4-pixel grid, the grammar read off the plugin's pixel keyboard: a
  1-unit outline, a bevel light at top-left and dark at bottom-right, 3x5 glyphs, cyan
  ink when pressed. `g13map obs --dump DIR` writes it; a test keeps `assets/obs/`
  equal to the generator, so regenerate after changing the art.
- **The glass table** (src/glass.rs, `~/.config/g13map/glass`) is the one place that
  translates LED values to monitor colours; draw the LCD through `Glass::render`
  rather than inventing colours. A lit pixel is the background's light `glow` times
  over (2 by default), the same colour brighter, going pale only where a channel has no
  headroom; the old blend toward white desaturated, which the owner saw at once beside
  the glass. `g13map glass` is the by-eye course that fills the file.
- **The OBS plugin** (`obs/g13pad-obs.c`, 0.2.41) is the real answer to "a source like
  the others": libobs C API, `gs_image_file4_init` with `GS_IMAGE_ALPHA_PREMULTIPLY`,
  blend ONE/INVSRCALPHA with sRGB framebuffer like image-source, one
  `gs_draw_sprite_subregion` per element, a dynamic 160x43 `GS_RGBA` texture for the
  LCD updated in `video_render` (the graphics context is sure there). jansson parses
  the layout: `obs_data` arrays hold objects only, so `pos: [x, y]` is unreadable
  through it. Arch's `obs-studio` package carries the headers and the CMake package
  (`OBS::libobs`).
- **Testing OBS headless touched the live config once.** `HOME=` alone is not enough:
  this session exports `XDG_CONFIG_HOME=$HOME/.config`, so an OBS started with a
  scratch HOME loaded and *saved* the real profile and scene collection (restored from
  `Untitled.json.bak`; the ini files were rewritten and could not be checked). Set
  `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` to the scratch tree, confirm
  the process's environ before trusting it, and kill it with `-9` if it is wrong, so it
  never saves on exit. User plugins load from
  `$XDG_CONFIG_HOME/obs-studio/plugins/NAME/bin/64bit/NAME.so` with `data/locale`
  beside; a scene-collection item copied from a group needs `group_item_backup: false`
  or it stays hidden.
- **Check the window carries alpha** with a raw `xwd` dump (ImageMagick drops the
  alpha byte of a depth-32 window): the corner pixel reads `00000000`, the body
  `2c2c2cff`. `import -window` composites over black and proves nothing.
