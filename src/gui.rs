// SPDX-License-Identifier: GPL-3.0-or-later
//! The editor window: the board on the left, the selected control's binding on the right.
//! Every edit goes to the daemon at once (so the key can be tried), Save writes the file.
use crate::adjust::Adjust;
use crate::board::Board;
use crate::focus::{self, Rules};
use crate::gamepad::{Click, Mapping, Stick};
use crate::keys::{self, action_label, KEYS};
use crate::lcd;
use crate::modes::{self, Modes};
use crate::profile::{chord, chord_action, Profile, StickMode, MODIFIERS, UNBOUND};
use crate::{daemon, load, profile_names, save, set_active};
use egui::{Color32, ColorImage, Key, RichText, Sense, TextureHandle, TextureOptions, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Keys,
    Raw,
}

struct App {
    board: Board,
    name: String,
    profile: Profile,
    saved: Profile,
    base: Option<Profile>,
    analog: bool,
    layout: HashMap<String, String>,
    layout_checked: Instant,
    selected: Option<&'static str>,
    mode: Mode,
    mods: Vec<String>,
    main: Option<String>,
    raw: String,
    filter: String,
    capture: bool,
    new_name: String,
    status: String,
    status_err: bool,
    esc_armed: bool,
    /// A backlight change not yet sent: the picker fires every frame while dragging.
    rgb_pending: bool,
    rgb_sent: Instant,
    /// The profile's LCD image as a texture, and one per kept image for the history list.
    lcd_tex: Option<TextureHandle>,
    lcd_thumbs: HashMap<String, TextureHandle>,
    lcd_names: Vec<String>,
    /// Textures and the list need a reload (first frame, switch, pick, revert).
    lcd_dirty: bool,
    lcd_path: String,
    /// Frames per kept picture (one for a still), for the list.
    lcd_frames: HashMap<String, usize>,
    /// The profile's picture when animated: stepped through on the board on its own clock.
    lcd_anim: Option<lcd::Animation>,
    lcd_anim_at: Instant,
    lcd_anim_frame: usize,
    /// What is on the panel: for an animation, the thread sending its frames.
    player: Option<lcd::Player>,
    /// Held for the editor's life; the watcher's animation stays off the panel meanwhile.
    _editor_lock: Option<std::fs::File>,
    /// The LCD picture window, while open.
    adjust: Option<Adjust>,
    text_open: bool,
    /// The Animations window: the built-in scenes (`art`) with live previews, rendered
    /// on first open; each keeps the frame its texture shows.
    anim_open: bool,
    anim_previews: Vec<(lcd::Animation, TextureHandle, usize)>,
    anim_at: Instant,
    lcd_text: String,
    text_opts: crate::text_options::Options,
    text_fonts: Vec<(PathBuf, String)>,
    text_preview: Option<(TextureHandle, lcd::Animation, Instant)>,
    text_preview_dirty: bool,
    text_error: String,
    /// A file dialog in flight (zenity in a thread), delivering the path or why not.
    picker: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    /// M-keys as profile selectors (`~/.config/g13map/modes`, served by `g13map watch`).
    modes: Modes,
    /// Profiles by focused window (`~/.config/g13map/focus`, same watcher), and its window.
    rules: Rules,
    windows_open: bool,
    wins: Vec<focus::Win>,
    wins_at: Option<Instant>,
    wins_err: String,
    /// When the active profile was last compared with the file (the watcher switches too).
    active_checked: Instant,
    /// When the adapter service was last asked whether it runs (a fork per ask).
    analog_checked: Instant,
    gamepad_open: bool,
    gamepad_edit: Mapping,
}

pub fn run(name: String) -> Result<String, String> {
    let app = App::new(name)?;
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000., 900.])
            .with_min_inner_size([760., 700.])
            .with_title("G13 — key bindings"),
        ..Default::default()
    };
    eframe::run_native(
        "g13map",
        opts,
        Box::new(move |cc| {
            cc.egui_ctx.set_fonts(crate::fonts::with_characters(
                &app.layout.values().cloned().collect::<String>(),
            ));
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(String::new())
}

impl App {
    fn new(name: String) -> Result<Self, String> {
        let (profile, notes) = load(&name)?;
        let mut app = App {
            board: Board::new(),
            name,
            saved: profile.clone(),
            profile,
            base: crate::daemon_base(),
            analog: daemon::analog_active(),
            layout: keys::layout_map(),
            layout_checked: Instant::now(),
            selected: None,
            mode: Mode::Keys,
            mods: vec![],
            main: None,
            raw: String::new(),
            filter: String::new(),
            capture: false,
            new_name: String::new(),
            status: notes.join("; "),
            status_err: !notes.is_empty(),
            esc_armed: false,
            rgb_pending: false,
            rgb_sent: Instant::now(),
            lcd_tex: None,
            lcd_thumbs: HashMap::new(),
            lcd_names: vec![],
            lcd_dirty: true,
            lcd_path: String::new(),
            lcd_frames: HashMap::new(),
            lcd_anim: None,
            lcd_anim_at: Instant::now(),
            lcd_anim_frame: 0,
            player: None,
            _editor_lock: lcd::editor_lock(),
            adjust: None,
            text_open: false,
            anim_open: false,
            anim_previews: vec![],
            anim_at: Instant::now(),
            lcd_text: String::new(),
            text_opts: crate::text_options::Options::default(),
            text_fonts: vec![],
            text_preview: None,
            text_preview_dirty: false,
            text_error: String::new(),
            picker: None,
            modes: Modes::load(),
            rules: Rules::load(),
            windows_open: false,
            wins: vec![],
            wins_at: None,
            wins_err: String::new(),
            active_checked: Instant::now(),
            analog_checked: Instant::now(),
            gamepad_open: false,
            gamepad_edit: Mapping::default(),
        };
        app.board.layout = app.layout.clone();
        app.select(Some("G1"));
        // Taking the editor lock pauses the watcher. Own the saved picture immediately.
        if let Err(e) = app.lcd_send_profile() {
            app.report(Err(e), "");
        }
        Ok(app)
    }
}

impl App {
    fn dirty(&self) -> bool {
        self.profile != self.saved
    }
    fn report(&mut self, r: Result<(), String>, ok: &str) {
        match r {
            Ok(()) => {
                self.status = ok.to_string();
                self.status_err = false;
            }
            Err(e) => {
                if !self.status_err || self.status != e {
                    let _ = crate::overlay::notify(&e);
                }
                self.status = e;
                self.status_err = true;
            }
        }
    }
    fn select(&mut self, key: Option<&'static str>) {
        self.selected = key;
        self.capture = false;
        let action = key
            .and_then(|k| self.profile.binds.get(k))
            .cloned()
            .unwrap_or_default();
        self.raw = action.clone();
        match chord(&action) {
            Some((mods, main)) => {
                self.mode = Mode::Keys;
                self.mods = mods;
                self.main = main;
            }
            None if action.is_empty() => {
                self.mode = Mode::Keys;
                self.mods.clear();
                self.main = None;
            }
            None => self.mode = Mode::Raw,
        }
    }
    /// Sets (or clears) the selected control's action, and sends it to the daemon.
    fn set_action(&mut self, action: Option<String>) {
        let Some(key) = self.selected else { return };
        let was_bound = self.profile.binds.contains_key(key)
            || self
                .base
                .as_ref()
                .is_some_and(|b| b.binds.contains_key(key));
        let line = match &action {
            Some(a) => format!("bind {key} {a}\n"),
            None if was_bound => format!("bind {key} {UNBOUND}\n"),
            None => String::new(),
        };
        match action {
            Some(a) => {
                self.profile.binds.insert(key.to_string(), a);
            }
            None => {
                self.profile.binds.remove(key);
            }
        }
        let shown = self
            .profile
            .binds
            .get(key)
            .map(|a| action_label(a, &self.layout))
            .unwrap_or_else(|| "unbound".into());
        let r = daemon::send(&line);
        self.report(r, &format!("{key}: {shown} (live; not saved yet)"));
    }
    fn set_chord(&mut self) {
        let a = chord_action(&self.mods, self.main.as_deref());
        self.raw = a.clone();
        self.set_action(if a.is_empty() { None } else { Some(a) });
    }
    fn send_all(&mut self, ok: &str) {
        let r = self.send_profile(&self.profile.clone());
        self.report(r, ok);
    }
    fn send_profile(&mut self, p: &Profile) -> Result<(), String> {
        self.send_transition(p, None)
    }
    fn plan<'a>(
        &'a self,
        target: &'a Profile,
        previous: Option<&'a Profile>,
    ) -> crate::application::Plan<'a> {
        crate::application::Plan {
            target,
            previous,
            baseline: self.base.as_ref(),
            routing: if self.modes.on {
                crate::application::Routing::Sum(0)
            } else {
                crate::application::Routing::Profile
            },
        }
    }
    fn send_transition(&mut self, p: &Profile, previous: Option<&Profile>) -> Result<(), String> {
        let result = self.plan(p, previous).execute();
        // A command-write failure can follow a successful adapter transition.
        // Keep the editor's ownership display tied to the observed service state.
        self.analog = result
            .as_ref()
            .copied()
            .unwrap_or_else(|_| daemon::analog_active());
        result.map(|_| ())
    }
    /// Everything the daemon needs for `p` to be in force: its commands, and with M-key
    /// modes on the LEDs for this profile's bit sum and the routes that override its M-keys.
    fn full_commands(&self, p: &Profile) -> String {
        self.plan(p, None).commands(self.analog)
    }
    fn do_save(&mut self) {
        match save(&self.name, &self.profile) {
            Ok(()) => {
                self.saved = self.profile.clone();
                self.report(Ok(()), &format!("saved profile '{}'", self.name));
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    fn do_revert(&mut self) {
        // Keys bound in the edit but not in the saved copy need an explicit unbind.
        let lcd_changed = self.profile.lcd != self.saved.lcd;
        let previous = self.profile.clone();
        self.profile = self.saved.clone();
        let r = self
            .send_transition(&self.saved.clone(), Some(&previous))
            .and_then(|_| {
                if lcd_changed {
                    self.lcd_dirty = true;
                    self.lcd_send_profile()
                } else {
                    Ok(())
                }
            });
        self.report(r, "reverted to the saved profile");
        self.select(self.selected);
    }
    fn switch(&mut self, name: String) {
        if self.dirty() {
            self.report(Err("save or revert first, then switch profiles".into()), "");
            return;
        }
        match load(&name) {
            Ok((p, _)) => {
                // Unbind what the old profile bound and the new one does not.
                let previous = self.profile.clone();
                let r = self
                    .send_transition(&p, Some(&previous))
                    .and_then(|_| set_active(&name));
                if let Err(e) = r {
                    self.report(Err(e), "");
                    return;
                }
                self.profile = p.clone();
                self.saved = p;
                self.name = name.clone();
                self.gamepad_open = false;
                let r = self.lcd_send_profile();
                self.report(r, &format!("profile '{name}' is active"));
                self.lcd_dirty = true;
                self.select(self.selected);
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    fn create(&mut self) {
        let name: String = self
            .new_name
            .trim()
            .chars()
            .filter(|c| c.is_alphanumeric() || "-_".contains(*c))
            .collect();
        if name.is_empty() {
            self.report(
                Err("give the new profile a name (letters, digits, - _)".into()),
                "",
            );
            return;
        }
        if crate::profile_path(&name).exists() {
            self.report(Err(format!("profile '{name}' already exists")), "");
            return;
        }
        if self.dirty() {
            self.report(
                Err("save or revert first, then create a profile".into()),
                "",
            );
            return;
        }
        match save(&name, &self.profile).and_then(|_| set_active(&name)) {
            Ok(()) => {
                self.name = name.clone();
                self.new_name.clear();
                self.report(
                    Ok(()),
                    &format!("created profile '{name}' as a copy; it is active"),
                );
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    fn delete(&mut self) {
        let names = profile_names();
        if names.len() < 2 {
            self.report(Err("keep at least one profile".into()), "");
            return;
        }
        let gone = self.name.clone();
        let next = names.into_iter().find(|n| *n != gone).unwrap_or_default();
        if let Err(e) = std::fs::remove_file(crate::profile_path(&gone)) {
            self.report(Err(e.to_string()), "");
            return;
        }
        self.saved = self.profile.clone(); // not dirty: the file is gone either way
        self.switch(next);
        self.status = format!("deleted profile '{gone}'; {}", self.status);
    }
    fn handle_keys(&mut self, ctx: &egui::Context) {
        if self.capture {
            // Modifier presses arrive on their own (and with the main key in the same frame
            // under xdotool): skip them and take the first real key.
            let hit = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        modifiers,
                        ..
                    } if !is_modifier(*key) => Some((*key, *physical_key, *modifiers)),
                    // egui-winit turns Ctrl+X/C/V into these before any key event.
                    egui::Event::Cut => Some((Key::X, None, i.modifiers)),
                    egui::Event::Copy => Some((Key::C, None, i.modifiers)),
                    egui::Event::Paste(_) => Some((Key::V, None, i.modifiers)),
                    _ => None,
                })
            });
            if let Some((key, physical, modifiers)) = hit {
                if key == Key::Escape {
                    self.capture = false;
                    self.report(Ok(()), "capture cancelled");
                    return;
                }
                match physical.and_then(evdev_name).or_else(|| evdev_name(key)) {
                    Some(n) => {
                        self.mods = [
                            (modifiers.ctrl, "LEFTCTRL"),
                            (modifiers.shift, "LEFTSHIFT"),
                            (modifiers.alt, "LEFTALT"),
                        ]
                        .iter()
                        .filter(|(on, _)| *on)
                        .map(|(_, m)| m.to_string())
                        .collect();
                        self.main = Some(n.to_string());
                        self.mode = Mode::Keys;
                        self.capture = false;
                        self.set_chord();
                    }
                    None => self.report(
                        Err(format!(
                            "no Linux key name for {key:?}; pick it from the list"
                        )),
                        "",
                    ),
                }
            }
            return;
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(Key::S)) {
            self.do_save();
        }
        if self.windows_open && ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.windows_open = false;
            return;
        }
        if self.text_open {
            if ctx.input(|i| i.key_pressed(Key::Escape)) && !ctx.egui_wants_keyboard_input() {
                self.text_open = false;
            }
            return;
        }
        if self.anim_open {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.anim_open = false;
            }
            return;
        }
        // With the picture window open, Escape closes it and nothing else.
        if let Some(a) = &mut self.adjust {
            if ctx.input(|i| i.key_pressed(Key::Escape)) && !ctx.egui_wants_keyboard_input() {
                a.open = false;
            }
            return;
        }
        // Discarding takes two Escapes in a row; any other key or click in between disarms.
        let other_input = ctx.input(|i| {
            i.pointer.any_pressed()
                || i.events.iter().any(|e| {
                    matches!(e, egui::Event::Key { key, pressed: true, .. } if *key != Key::Escape)
                })
        });
        if other_input {
            self.esc_armed = false;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) && !ctx.egui_wants_keyboard_input() {
            if !self.dirty() || self.esc_armed {
                if self.dirty() {
                    self.do_revert();
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                self.esc_armed = true;
                self.report(
                    Err("unsaved changes: Save, or press Escape again to discard them".into()),
                    "",
                );
            }
        }
    }
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Profile");
            let mut pick = None;
            egui::ComboBox::from_id_salt("profile")
                .selected_text(&self.name)
                .show_ui(ui, |ui| {
                    for n in profile_names() {
                        if ui.selectable_label(n == self.name, &n).clicked() && n != self.name {
                            pick = Some(n);
                        }
                    }
                });
            if let Some(n) = pick {
                self.switch(n);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("new name")
                    .desired_width(90.0),
            );
            if ui.button("New (copy)").clicked() {
                self.create();
            }
            if ui
                .selectable_label(self.windows_open, "Windows…")
                .on_hover_text("Profiles by focused i3 window")
                .clicked()
            {
                self.windows_open = !self.windows_open;
            }
            if ui
                .button("Delete")
                .on_hover_text("Deletes this profile's file and switches to another")
                .clicked()
            {
                self.delete();
            }
            ui.separator();
            ui.label("Backlight");
            let mut rgb = self.profile.rgb.unwrap_or([31, 0, 127]);
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.profile.rgb = Some(rgb);
                self.rgb_pending = true;
            }
            // Live while dragging, but at most ten commands a second (each is a journal line),
            // and always once more when the button is released.
            if self.rgb_pending {
                let dragging = ui.input(|i| i.pointer.any_down());
                if !dragging || self.rgb_sent.elapsed() >= Duration::from_millis(100) {
                    self.rgb_pending = false;
                    self.rgb_sent = Instant::now();
                    let r = daemon::send(&format!("rgb {} {} {}\n", rgb[0], rgb[1], rgb[2]));
                    self.report(r, "backlight set (live; not saved yet)");
                } else {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
            ui.separator();
            ui.label("LEDs");
            if self.modes.on {
                ui.label(RichText::new("(while no sum is lit)").weak())
                    .on_hover_text(
                    "M-Sum mode: a lit sum takes the LEDs over; MR gives them back to the profile",
                );
            }
            {
                let mut leds = self.profile.leds.unwrap_or(0);
                let mut changed = false;
                for (bit, n) in [(1u8, "M1"), (2, "M2"), (4, "M3"), (8, "MR")] {
                    let mut on = leds & bit != 0;
                    if ui.checkbox(&mut on, n).changed() {
                        leds = if on { leds | bit } else { leds & !bit };
                        changed = true;
                    }
                }
                if changed {
                    self.profile.leds = Some(leds);
                    let r = daemon::send(&format!("mod {leds}\n"));
                    self.report(r, "M-key LEDs set (live; not saved yet)");
                }
            }
            ui.separator();
            let dirty = self.dirty();
            if ui
                .add_enabled(
                    dirty,
                    egui::Button::new(if dirty { "Save *" } else { "Save" }),
                )
                .on_hover_text("Ctrl+S")
                .clicked()
            {
                self.do_save();
            }
            if ui.add_enabled(dirty, egui::Button::new("Revert")).clicked() {
                self.do_revert();
            }
            if ui
                .button("Re-send")
                .on_hover_text(
                    "Send the whole profile to the daemon again (after a g13.service restart)",
                )
                .clicked()
            {
                self.send_all("profile re-sent to the daemon");
            }
        });
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.selected else {
            ui.label("Click a control on the board.");
            return;
        };
        if key == "LCD" {
            self.lcd_editor(ui);
            return;
        }
        ui.heading(key);
        if modes::KEYS.contains(&key) && self.modes_editor(ui) {
            return;
        }
        // Fixed two lines whatever the action's length, so the controls below never move.
        match self.profile.binds.get(key) {
            Some(a) => {
                ui.add(
                    egui::Label::new(RichText::new(action_label(a, &self.layout)).strong())
                        .truncate(),
                );
                ui.add(egui::Label::new(RichText::new(a).monospace().small()).truncate());
            }
            None => {
                ui.label(RichText::new("unbound").italics());
                ui.label(RichText::new(" ").small());
            }
        }
        if self.analog && key.starts_with("STICK_") {
            ui.colored_label(Color32::from_rgb(230, 170, 60), "The stick is an analog controller now (g13-analog.service); zone bindings only act in KEYS mode.");
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.mode, Mode::Keys, "Keys");
            ui.selectable_value(&mut self.mode, Mode::Raw, "Raw");
            if ui.button("Unbind").clicked() {
                self.mods.clear();
                self.main = None;
                self.raw.clear();
                self.set_action(None);
            }
        });
        ui.add_space(6.0);
        match self.mode {
            Mode::Keys => self.keys_editor(ui),
            Mode::Raw => self.raw_editor(ui),
        }
    }
    fn keys_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut changed = false;
            for (m, label) in MODIFIERS.iter().take(4) {
                let mut on = self.mods.iter().any(|x| x == m);
                if ui.checkbox(&mut on, *label).changed() {
                    if on {
                        self.mods.push(m.to_string());
                    } else {
                        self.mods.retain(|x| x != m);
                    }
                    changed = true;
                }
            }
            if changed {
                self.set_chord();
            }
        });
        ui.horizontal(|ui| {
            let label = if self.capture {
                "Press a key… (Esc cancels)"
            } else {
                "Press a key…"
            };
            if ui
                .selectable_label(self.capture, label)
                .on_hover_text("Records the physical key you press next, with Ctrl/Shift/Alt held")
                .clicked()
            {
                self.capture = !self.capture;
            }
        });
        // Its own line, truncated: with the capture label beside it the row outgrew the panel
        // and egui squeezed the panel off the window.
        if let Some(m) = &self.main {
            if let Some(sym) = self.layout.get(m) {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("KEY_{m} types “{sym}” in your layout")).weak(),
                    )
                    .truncate(),
                );
            }
        }
        ui.add_space(4.0);
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter keys"));
        let mut pick = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for &(_, name) in KEYS {
                    let label = keys::layout_label(name, &self.layout);
                    if !keys::matches_filter(name, &self.layout, &self.filter) {
                        continue;
                    }
                    let text = format!("{label}   KEY_{name}");
                    if ui
                        .selectable_label(self.main.as_deref() == Some(name), text)
                        .clicked()
                    {
                        pick = Some(name.to_string());
                    }
                }
            });
        if let Some(n) = pick {
            self.main = Some(n);
            self.set_chord();
        }
    }
    fn raw_editor(&mut self, ui: &mut egui::Ui) {
        ui.label("The daemon's action grammar: KEY_A+KEY_B chords, a second group sent on release (KEY_A KEY_B), MLEFT/MRIGHT/MMIDDLE/MSIDE/MEXTRA mouse buttons, >text to the output pipe, !command to the daemon.");
        let r = ui.add(egui::TextEdit::singleline(&mut self.raw).desired_width(f32::INFINITY));
        let submit = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if ui.button("Set").clicked() || submit {
            let a = self.raw.trim().to_string();
            self.set_action(if a.is_empty() { None } else { Some(a) });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _ = crate::overlay::tick();
        if self.layout_checked.elapsed() >= Duration::from_secs(2) {
            let layout = keys::layout_map();
            if layout != self.layout {
                ctx.set_fonts(crate::fonts::with_characters(
                    &layout.values().cloned().collect::<String>(),
                ));
                self.layout = layout;
                self.board.layout = self.layout.clone();
            }
            self.layout_checked = Instant::now();
        }
        self.handle_keys(&ctx);
        if self.active_checked.elapsed() >= Duration::from_secs(1) {
            self.follow_active();
        }
        // Unconditional: a timer that wakes a hair early would otherwise end the chain.
        ctx.request_repaint_after(Duration::from_millis(250));
        if self.lcd_dirty {
            self.lcd_refresh(&ctx);
        }
        // The board's LCD runs an animation on the same clock as the panel (frame by frame,
        // nothing skipped: the screen is faster than the daemon).
        if let (Some(a), Some(t)) = (&self.lcd_anim, &mut self.lcd_tex) {
            let (i, next) = a.due(self.lcd_anim_at.elapsed());
            if i != self.lcd_anim_frame {
                self.lcd_anim_frame = i;
                t.set(
                    ColorImage::from_gray([lcd::W, lcd::H], &a.frames[i].1.gray()),
                    TextureOptions::NEAREST,
                );
            }
            ctx.request_repaint_after(next.max(Duration::from_millis(10)));
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(p) = dropped {
            self.select(Some("LCD"));
            self.lcd_pick(&ctx, p);
        }
        if let Some(rx) = &self.picker {
            match rx.try_recv() {
                Ok(Ok(p)) => {
                    self.picker = None;
                    self.select(Some("LCD"));
                    self.lcd_pick(&ctx, p);
                }
                Ok(Err(e)) => {
                    self.picker = None;
                    self.report(Err(e), "");
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.picker = None,
            }
        }
        if !self.dirty() {
            self.esc_armed = false;
        }
        egui::Panel::top("bar").show(ui, |ui| {
            ui.add_space(4.0);
            self.top_bar(ui);
            ui.add_space(4.0);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let color = if self.status_err {
                    Color32::from_rgb(255, 120, 100)
                } else {
                    ui.visuals().text_color()
                };
                ui.colored_label(color, &self.status);
            });
        });
        egui::Panel::right("editor")
            .resizable(false)
            .exact_size(340.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                self.editor(ui);
            });
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            let lcd_tex = self
                .adjust
                .as_ref()
                .map(|a| a.texture())
                .or_else(|| self.lcd_tex.as_ref().map(|t| t.id()));
            let clicked = self.board.show(
                ui,
                &self.profile,
                self.selected,
                self.analog,
                lcd_tex,
                self.modes.on,
            );
            if let Some((k, double)) = clicked {
                self.select(Some(k));
                // A double click goes straight to capturing the key to bind.
                let bindable = k != "LCD" && !(self.modes.on && modes::KEYS.contains(&k));
                if double && bindable {
                    self.mode = Mode::Keys;
                    self.capture = true;
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Click a control to edit it. Edits reach the daemon at once; Save keeps them for the next login.").weak());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Profile stick:").weak());
                let mut mode = self.profile.stick;
                egui::ComboBox::from_id_salt("profile-stick")
                    .selected_text(mode.map_or("keep current", StickMode::label))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut mode, None, "keep current");
                        ui.selectable_value(&mut mode, Some(StickMode::Analog), "analog");
                        ui.selectable_value(&mut mode, Some(StickMode::Keys), "keys");
                    });
                if mode != self.profile.stick {
                    self.set_stick(mode);
                }
                if ui.button("Controller…").clicked() {
                    self.gamepad_edit = self.profile.gamepad.unwrap_or_default();
                    self.gamepad_open = true;
                }
                let mapping = self.profile.gamepad.unwrap_or_default();
                ui.label(RichText::new(if self.analog {
                    format!("analog; TOP: {}", mapping.click.label())
                } else {
                    "keys; zone bindings and TOP apply".into()
                }).weak());
            });
            let mut enabled = crate::overlay::enabled();
            if ui.checkbox(&mut enabled, "Show temporary LCD errors").changed() {
                let animated = self.profile.lcd.as_deref().is_some_and(|n| lcd::frame_count(n) > 1);
                let r = crate::overlay::set_enabled(enabled).and_then(|_| Self::watch_unit(enabled || self.modes.on || self.rules.on || animated));
                self.report(r, "LCD error preference saved");
            }
        });
        self.adjust_window(&ctx);
        self.text_window(&ctx);
        self.windows_window(&ctx);
        self.gamepad_window(&ctx);
        self.animations_window(&ctx);
    }
}

/// The LCD side of the editor: the profile's picture, choosing a new one, the history.
impl App {
    fn lcd_texture(ctx: &egui::Context, name: &str, bm: &lcd::Bitmap) -> TextureHandle {
        ctx.load_texture(
            format!("lcd:{name}"),
            ColorImage::from_gray([lcd::W, lcd::H], &bm.gray()),
            TextureOptions::NEAREST,
        )
    }
    /// Reloads the profile's image and the history list with their textures.
    fn lcd_refresh(&mut self, ctx: &egui::Context) {
        self.lcd_dirty = false;
        self.lcd_names = lcd::names();
        let current = match &self.profile.lcd {
            Some(n) => match lcd::load(n) {
                Ok(b) => Some((n.clone(), b)),
                Err(e) => {
                    self.report(Err(e), "");
                    None
                }
            },
            None => None,
        };
        self.lcd_tex = current.as_ref().map(|(n, b)| Self::lcd_texture(ctx, n, b));
        self.lcd_anim = current.as_ref().and_then(|(n, _)| lcd::load_anim(n));
        self.lcd_anim_at = Instant::now();
        self.lcd_anim_frame = 0;
        self.lcd_frames = self
            .lcd_names
            .iter()
            .map(|n| (n.clone(), lcd::frame_count(n)))
            .collect();
        for n in &self.lcd_names {
            // The current image may have been converted again under its old name.
            let stale = current.as_ref().is_some_and(|(c, _)| c == n);
            if stale || !self.lcd_thumbs.contains_key(n) {
                if let Ok(b) = lcd::load(n) {
                    self.lcd_thumbs
                        .insert(n.clone(), Self::lcd_texture(ctx, n, &b));
                }
            }
        }
    }
    /// Puts what the profile calls for on the panel: its picture (an animation keeps playing
    /// from a thread), or the daemon's logo when it has none.
    fn lcd_send_profile(&mut self) -> Result<(), String> {
        self.player = None; // stops and joins the old thread before the new first frame
        self.player = Some(lcd::show(self.profile.lcd.as_deref(), false)?);
        Ok(())
    }
    /// Health mode went on, off or to another reader: the watcher shows the meter once the
    /// profile is saved and active, so the unit must run for it.
    fn health_mode_changed(&mut self) {
        let mut msg = match self.profile.health {
            Some(r) => format!("Health mode: {} (not saved yet)", r.about()),
            None => "Health mode off: the picture shows again (not saved yet)".to_string(),
        };
        let mut r = Ok(());
        if self.profile.health.is_some() && !self.modes.on && !self.rules.on {
            r = Self::watch_unit(true);
            msg.push_str("; g13map-watch.service enabled to show it after the editor closes");
        }
        self.report(r, &msg);
    }
    /// Uses a kept image (or none) for this profile: live at once, saved with the profile.
    fn lcd_use(&mut self, name: Option<String>) {
        if let Some(n) = &name {
            lcd::touch(n);
        }
        self.profile.lcd = name.clone();
        let mut r = self.lcd_send_profile();
        let shown = name.as_deref().unwrap_or("the daemon's logo");
        let mut msg = format!("LCD: {shown} (live; not saved yet)");
        // Once the editor closes, an animation needs the watcher to keep it moving; the
        // health meter is the watcher's altogether.
        let animated = name
            .as_deref()
            .is_some_and(|n| lcd::frame_count(n) > 1 || crate::meter::selects(Some(n)).is_some());
        if r.is_ok() && animated && !self.modes.on && !self.rules.on {
            r = Self::watch_unit(true);
            msg.push_str("; g13map-watch.service enabled to play it after the editor closes");
        }
        self.report(r, &msg);
        self.lcd_dirty = true;
    }
    /// Takes a picture file: the history entry made from it before, or a fresh conversion
    /// (letterboxed, dithered) kept under its name; shows it and opens the adjust window.
    fn lcd_pick(&mut self, ctx: &egui::Context, path: PathBuf) {
        let picked = lcd::name_for(&path).and_then(|name| {
            let src = lcd::Source::open(&path)?;
            let opts = lcd::options(&name).unwrap_or_default();
            // New, or kept as a still before animations were kept: convert it now.
            if lcd::source(&name).is_none() || (src.animated() && lcd::frame_count(&name) == 1) {
                lcd::keep(&name, &path, &src.render_all(&opts), &opts)?;
            }
            Ok((name, src, opts))
        });
        match picked {
            Ok((name, src, opts)) => {
                self.lcd_path = path.display().to_string();
                self.lcd_use(Some(name.clone()));
                self.adjust = Some(Adjust::new(ctx, src, name, opts));
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    /// Opens the adjust window on the profile's picture, from the copy kept beside it.
    fn lcd_adjust(&mut self, ctx: &egui::Context) {
        let Some(name) = self.profile.lcd.clone() else {
            return;
        };
        let opened = lcd::source(&name)
            .ok_or_else(|| format!("no copy of the picture behind '{name}' is kept"))
            .and_then(|p| lcd::Source::open(&p));
        match opened {
            Ok(src) => {
                let opts = lcd::options(&name).unwrap_or_default();
                // Kept as a still before animations were kept: its frames go in now.
                if src.animated() && lcd::frame_count(&name) == 1 {
                    match lcd::keep(&name, &src.path, &src.render_all(&opts), &opts) {
                        Ok(()) => self.lcd_use(Some(name.clone())),
                        Err(e) => self.report(Err(e), ""),
                    }
                }
                self.adjust = Some(Adjust::new(ctx, src, name, opts));
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    /// Runs the adjust window while it is open; a kept frame is shown and sent from here.
    fn adjust_window(&mut self, ctx: &egui::Context) {
        let Some(mut a) = self.adjust.take() else {
            return;
        };
        match a.show(ctx, &self.name) {
            Ok(Some(name)) => self.lcd_use(Some(name)),
            Ok(None) => {}
            Err(e) => self.report(Err(e), ""),
        }
        if a.open {
            self.adjust = Some(a);
        } else {
            self.lcd_dirty = true;
        }
    }
    /// Opens the file dialog in a thread: zenity through xdg-desktop-portal, which on this
    /// desktop is the LXQt (libfm-qt) picker. The result arrives via `self.picker`.
    fn lcd_browse(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.picker = Some(rx);
        std::thread::spawn(move || {
            let out = std::process::Command::new("zenity")
                .env("GTK_USE_PORTAL", "1")
                .args([
                    "--file-selection",
                    "--title=Picture for the G13 LCD",
                    "--file-filter=Images | *.png *.PNG *.jpg *.JPG *.jpeg *.JPEG *.gif *.GIF *.webp *.WEBP *.bmp *.BMP",
                    "--file-filter=All files | *",
                ])
                .output();
            let r = match out {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        Err("nothing chosen".to_string())
                    } else {
                        Ok(PathBuf::from(s))
                    }
                }
                Ok(o) if o.status.code() == Some(1) => Err("cancelled".to_string()),
                Ok(o) => Err(format!(
                    "zenity: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                )),
                Err(e) => Err(format!("zenity: {e}; type or drop a path instead")),
            };
            let _ = tx.send(r);
        });
    }
    fn lcd_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("LCD");
        ui.label(
            RichText::new("160×43 pixels, one bit each. The picture is kept with the profile.")
                .weak(),
        );
        ui.add_space(6.0);
        match &self.lcd_tex {
            Some(t) => {
                ui.image((t.id(), Vec2::new(320.0, 86.0)));
                if let Some(a) = &self.lcd_anim {
                    let total: Duration = a.frames.iter().map(|(d, _)| *d).sum();
                    ui.label(
                        RichText::new(format!(
                            "{} frames, {:.1} s",
                            a.frames.len(),
                            total.as_secs_f32()
                        ))
                        .weak(),
                    );
                }
            }
            None => {
                let text = match &self.profile.lcd {
                    Some(n) => format!("image '{n}' is missing from the history"),
                    None => "no picture of its own: the daemon's logo".to_string(),
                };
                ui.label(RichText::new(text).italics());
                ui.add_space(68.0);
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button("Text…")
                .on_hover_text("Create reusable LCD text; long messages scroll")
                .clicked()
            {
                self.lcd_text = self
                    .profile
                    .lcd
                    .as_ref()
                    .and_then(|n| {
                        std::fs::read_to_string(lcd::dir().join(format!("{n}.text"))).ok()
                    })
                    .unwrap_or_default();
                self.text_open = true;
                self.text_opts = self
                    .profile
                    .lcd
                    .as_deref()
                    .map(crate::text_options::Options::load)
                    .unwrap_or_default();
                self.text_fonts = crate::text_options::fonts();
                self.text_preview_dirty = true;
                self.text_error.clear();
            }
            if ui
                .button("Animations…")
                .on_hover_text("Built-in looping pixel art, drawn in code")
                .clicked()
            {
                self.anim_open = true;
            }
            if ui
                .add_enabled(self.picker.is_none(), egui::Button::new("Browse…"))
                .on_hover_text("File dialog (any image format)")
                .clicked()
            {
                self.lcd_browse();
            }
            if ui
                .button("Logo")
                .on_hover_text("Back to the daemon's own picture")
                .clicked()
            {
                self.lcd_use(None);
            }
        });
        // Health mode: the meter instead of the picture while this profile is active,
        // with its reader; the picture stays kept for when the tick comes off.
        ui.horizontal(|ui| {
            use crate::meter::Reader;
            let mut on = self.profile.health.is_some();
            if ui
                .checkbox(&mut on, "Health mode")
                .on_hover_text(
                    "A live heartbeat and backlight that follow the game's health; \
                     g13map watch shows it while this profile is active",
                )
                .changed()
            {
                self.profile.health = on.then_some(Reader::Feed);
                self.health_mode_changed();
            }
            if let Some(current) = self.profile.health {
                let mut picked = current;
                egui::ComboBox::from_id_salt("health reader")
                    .selected_text(current.about())
                    .show_ui(ui, |ui| {
                        for r in [Reader::Feed, Reader::Cs2, Reader::Log] {
                            ui.selectable_value(&mut picked, r, r.about());
                        }
                    });
                if picked != current {
                    self.profile.health = Some(picked);
                    self.health_mode_changed();
                }
            }
        });
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.lcd_path)
                    .hint_text("or a path; or drop a file on the window")
                    .desired_width(250.0),
            );
            let submit = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Load").clicked() || submit) && !self.lcd_path.trim().is_empty() {
                self.lcd_pick(ui.ctx(), PathBuf::from(self.lcd_path.trim()));
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let has_source = self
                .profile
                .lcd
                .as_deref()
                .is_some_and(|n| lcd::source(n).is_some());
            if ui
                .add_enabled(has_source, egui::Button::new("Adjust…"))
                .on_hover_text("Choose the slice, scale, level and dithering")
                .clicked()
            {
                self.lcd_adjust(ui.ctx());
            }
            ui.label(RichText::new("slice, scale, level, dither").weak());
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Kept pictures, newest use first; click one to show it.").weak());
        let mut pick = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for n in &self.lcd_names {
                    let current = self.profile.lcd.as_deref() == Some(n.as_str());
                    ui.horizontal(|ui| {
                        if let Some(t) = self.lcd_thumbs.get(n) {
                            let img = egui::Image::new((t.id(), Vec2::new(160.0, 43.0)))
                                .sense(Sense::click());
                            if ui.add(img).clicked() {
                                pick = Some(n.clone());
                            }
                        }
                        // Truncated: a long name (a hash-named file) must not widen the row, or
                        // egui squeezes the whole panel off the window (asked 2026-09-29).
                        let text = if current {
                            RichText::new(n).strong()
                        } else {
                            RichText::new(n)
                        };
                        let label = egui::Label::new(text).truncate().sense(Sense::click());
                        let hover = match self.lcd_frames.get(n) {
                            Some(f) if *f > 1 => format!("{n} · {f} frames"),
                            _ => n.clone(),
                        };
                        if ui.add(label).on_hover_text(hover).clicked() {
                            pick = Some(n.clone());
                        }
                    });
                }
            });
        if let Some(n) = pick {
            self.lcd_use(Some(n));
        }
    }
    /// The built-in animations: live previews of every scene; a click keeps one as a
    /// picture (`NAME.anim` and `NAME.lpbm`, like `g13map-anim keep`) and shows it, so it
    /// then also appears among the kept pictures.
    fn animations_window(&mut self, ctx: &egui::Context) {
        if !self.anim_open {
            return;
        }
        if self.anim_previews.is_empty() {
            for s in crate::art::SCENES {
                let a = s.animation();
                let tex = Self::lcd_texture(ctx, &format!("scene:{}", s.name), a.first());
                self.anim_previews.push((a, tex, 0));
            }
            self.anim_at = Instant::now();
        }
        let elapsed = self.anim_at.elapsed();
        let mut open = self.anim_open;
        let mut pick = None;
        egui::Window::new("Animations")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(
                        "Looping pixel art drawn in code. Click one to keep it as a picture and show it.",
                    )
                    .weak(),
                );
                ui.add_space(4.0);
                for (s, (a, tex, shown)) in
                    crate::art::SCENES.iter().zip(self.anim_previews.iter_mut())
                {
                    let (i, _) = a.due(elapsed);
                    if *shown != i {
                        tex.set(
                            ColorImage::from_gray([lcd::W, lcd::H], &a.frames[i].1.gray()),
                            TextureOptions::NEAREST,
                        );
                        *shown = i;
                    }
                    ui.horizontal(|ui| {
                        let img = egui::Image::new((tex.id(), Vec2::new(200.0, 54.0)))
                            .sense(Sense::click());
                        if ui.add(img).on_hover_text("Keep and show").clicked() {
                            pick = Some(s.name);
                        }
                        ui.vertical(|ui| {
                            ui.set_min_width(330.0);
                            let title = if self.profile.lcd.as_deref() == Some(s.name) {
                                RichText::new(s.name).strong().underline()
                            } else {
                                RichText::new(s.name).strong()
                            };
                            ui.label(title);
                            let total: Duration = a.frames.iter().map(|(d, _)| *d).sum();
                            ui.label(
                                RichText::new(format!(
                                    "{}\n{} frames, {:.1} s",
                                    s.about,
                                    a.frames.len(),
                                    total.as_secs_f32()
                                ))
                                .weak(),
                            );
                        });
                    });
                }
                ctx.request_repaint_after(Duration::from_millis(30));
            });
        self.anim_open = open;
        if let Some(name) = pick {
            let anim = crate::art::SCENES
                .iter()
                .zip(&self.anim_previews)
                .find(|(s, _)| s.name == name)
                .map(|(_, (a, _, _))| a.clone());
            let kept = match (anim, lcd::origin(name)) {
                // The user's own picture or text under this name stays as it is.
                (Some(_), Some(what)) => {
                    Err(format!("'{name}' is {what}; rename or remove it first"))
                }
                (Some(a), None) => lcd::keep_animation(name, &a),
                (None, _) => Err(format!("no scene '{name}'")),
            };
            match kept {
                Ok(()) => self.lcd_use(Some(name.to_string())),
                Err(e) => self.report(Err(e), ""),
            }
        }
    }
    fn text_window(&mut self, ctx: &egui::Context) {
        use crate::text_options::Align;
        if !self.text_open {
            return;
        }
        if self.text_preview_dirty {
            self.text_preview_dirty = false;
            match crate::marquee::render_with(&self.lcd_text, &self.text_opts) {
                Ok(animation) => {
                    let texture = Self::lcd_texture(ctx, "text-preview", animation.first());
                    self.text_preview = Some((texture, animation, Instant::now()));
                    self.text_error.clear();
                }
                Err(e) => {
                    self.text_preview = None;
                    self.text_error = e;
                }
            }
        }
        if let Some((texture, animation, since)) = &mut self.text_preview {
            let (frame, wait) = animation.due(since.elapsed());
            texture.set(
                ColorImage::from_gray([lcd::W, lcd::H], &animation.frames[frame].1.gray()),
                TextureOptions::NEAREST,
            );
            if animation.animated() {
                ctx.request_repaint_after(wait);
            }
        }
        let mut open = self.text_open;
        let mut create = false;
        let before = (self.lcd_text.clone(), self.text_opts.clone());
        egui::Window::new("LCD text")
            .open(&mut open)
            .collapsible(false)
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.label("Up to 128 characters. Enter starts a new line. Tall text scales to the panel's 43 rows.");
                ui.add(
                    egui::TextEdit::multiline(&mut self.lcd_text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(3)
                        .char_limit(128),
                );
                let font_label = self.text_opts.font.as_ref().and_then(|path| self.text_fonts.iter()
                    .find(|(p, _)| p == path).map(|(_, label)| label.clone()))
                    .unwrap_or_else(|| self.text_opts.font.as_ref().map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Roboto Mono (default)".into()));
                egui::ComboBox::from_id_salt("text-font").width(460.0).selected_text(font_label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.text_opts.font, None, "Roboto Mono (default)");
                        for (path, label) in &self.text_fonts {
                            ui.selectable_value(&mut self.text_opts.font, Some(path.clone()), label);
                        }
                    });
                ui.add(egui::Slider::new(&mut self.text_opts.size, 6.0..=72.0).text("Size").suffix(" pt"));
                ui.add(egui::Slider::new(&mut self.text_opts.speed, 0..=120).text("Scroll").suffix(" px/s"))
                    .on_hover_text("Zero keeps a still slice; longer text scrolls horizontally otherwise");
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.text_opts.wrap, "Wrap to panel width");
                    ui.checkbox(&mut self.text_opts.invert, "Invert");
                    ui.label("Align:");
                    ui.selectable_value(&mut self.text_opts.align, Align::Left, "left");
                    ui.selectable_value(&mut self.text_opts.align, Align::Center, "centre");
                    ui.selectable_value(&mut self.text_opts.align, Align::Right, "right");
                });
                ui.separator();
                ui.label("On the panel");
                if let Some((texture, animation, _)) = &self.text_preview {
                    ui.image((texture.id(), Vec2::new(480.0, 129.0)));
                    ui.label(format!("{} frame(s)", animation.frames.len()));
                } else { ui.colored_label(Color32::from_rgb(255, 120, 100), &self.text_error); }
                ui.label(RichText::new("Use text creates a kept entry and sends it; Save keeps the profile's selection.").weak());
                if ui.button("Use text").clicked() {
                    create = true;
                }
            });
        if before != (self.lcd_text.clone(), self.text_opts.clone()) {
            self.text_preview_dirty = true;
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        self.text_open = open;
        if create {
            match crate::marquee::keep_with(&self.lcd_text, None, &self.text_opts) {
                Ok(name) => {
                    self.lcd_use(Some(name));
                    self.text_open = false;
                }
                Err(e) => self.report(Err(e), ""),
            }
        }
    }
}

/// M-keys as profile selectors: the switch, the sum-to-profile table, following the watcher.
impl App {
    /// Enables or disables the user unit that runs `g13map watch`. `G13MAP_UNIT=0` skips it
    /// (offscreen tests must not touch the real session).
    fn watch_unit(on: bool) -> Result<(), String> {
        if std::env::var("G13MAP_UNIT").is_ok_and(|v| v == "0") {
            return Ok(());
        }
        let verb = if on || crate::overlay::enabled() {
            "enable"
        } else {
            "disable"
        };
        let st = std::process::Command::new("systemctl")
            .args(["--user", verb, "--now", "g13map-watch.service"])
            .status()
            .map_err(|e| format!("systemctl: {e}"))?;
        if st.success() {
            Ok(())
        } else {
            Err(format!(
                "systemctl --user {verb} --now g13map-watch.service: {st}"
            ))
        }
    }
    /// The checkbox, and the table while on. Returns true when on: the M-key's own binding
    /// editor is hidden then, because the routes override it.
    /// The M-Sum mode tick, shared by the M-key panel and the Windows… panel.
    fn modes_tick(&mut self, ui: &mut egui::Ui) {
        let mut on = self.modes.on;
        if ui
            .checkbox(&mut on, "M-Sum mode")
            .on_hover_text("M1, M2 and M3 toggle and add up (M1+M3 = 5), MR clears; each sum picks a profile, over any window rule. The LEDs show the lit sum, or the profile's own pattern while none is lit. Served by g13map-watch.service.")
            .changed()
        {
            self.modes.on = on;
            let r = self
                .modes
                .save()
                .and_then(|_| Self::watch_unit(on || self.rules.on))
                .and_then(|_| {
                    if on {
                        daemon::send(&self.full_commands(&self.profile))
                    } else {
                        // Back to the profile's own M-key actions; unbound ones lose the route.
                        let mut p = self.profile.clone();
                        for k in modes::KEYS {
                            p.binds.entry(k.to_string()).or_insert_with(|| UNBOUND.into());
                        }
                        daemon::send(&self.full_commands(&p))
                    }
                });
            self.report(
                r,
                if on {
                    "M-Sum mode: M-keys select profiles now (g13map-watch.service enabled)"
                } else {
                    "M-keys are ordinary keys again"
                },
            );
        }
    }
    fn modes_editor(&mut self, ui: &mut egui::Ui) -> bool {
        self.modes_tick(ui);
        if !self.modes.on {
            return false;
        }
        ui.label(
            RichText::new("Lit M-LEDs add up to a sum; each sum picks a profile. A sum with no profile uses sum 0.")
                .weak(),
        );
        let names = profile_names();
        let mut changed = false;
        let m = &mut self.modes;
        egui::Grid::new("modes")
            .num_columns(2)
            .spacing([10.0, 4.0])
            .show(ui, |ui| {
                for bits in 0..8u8 {
                    ui.label(format!("{bits}: {}", modes::label(bits)));
                    let cur = m.profiles[bits as usize].clone();
                    let fallback = if bits == 0 { "default" } else { "(sum 0)" };
                    egui::ComboBox::from_id_salt(("mode", bits))
                        .selected_text(cur.clone().unwrap_or_else(|| fallback.to_string()))
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(cur.is_none(), fallback).clicked() {
                                m.profiles[bits as usize] = None;
                                changed = true;
                            }
                            for n in &names {
                                if ui
                                    .selectable_label(cur.as_deref() == Some(n.as_str()), n)
                                    .clicked()
                                {
                                    m.profiles[bits as usize] = Some(n.clone());
                                    changed = true;
                                }
                            }
                        });
                    ui.end_row();
                }
            });
        if changed {
            let r = self.modes.save();
            self.report(r, "modes saved; the watcher reads them within two seconds");
        }
        true
    }
    /// The watcher (or another editor) may have switched profiles: follow the active file.
    fn follow_active(&mut self) {
        self.active_checked = Instant::now();
        if self.analog_checked.elapsed() >= Duration::from_secs(10) {
            self.analog_checked = Instant::now();
            self.analog = daemon::analog_active();
        }
        let active = crate::active_name();
        if active == self.name || self.dirty() {
            return;
        }
        match load(&active) {
            Ok((p, _)) => {
                self.analog = daemon::analog_active();
                self.analog_checked = Instant::now();
                self.profile = p.clone();
                self.saved = p;
                self.name = active.clone();
                self.gamepad_open = false;
                self.lcd_dirty = true;
                self.select(self.selected);
                // The watcher sent the bindings; the editor owns LCD playback while open.
                // Stop the old thread before showing the new profile's first frame.
                let r = self.lcd_send_profile();
                self.report(
                    r,
                    &format!("profile '{active}' became active outside the editor"),
                );
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
}

/// The stick mode: analog (Codex's g13-analog.service, an Xbox-style controller) or keys.
impl App {
    fn set_gamepad(&mut self, mapping: Option<Mapping>) {
        let mut p = self.profile.clone();
        p.gamepad = mapping;
        match self.send_profile(&p) {
            Ok(()) => {
                self.profile = p;
                self.gamepad_open = false;
                self.report(Ok(()), "controller mapping applied; Save keeps it");
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    fn gamepad_window(&mut self, ctx: &egui::Context) {
        if !self.gamepad_open {
            return;
        }
        let mut open = true;
        let mut apply = false;
        let mut defaults = false;
        egui::Window::new("Controller mapping")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Profile: {}", self.name));
                ui.horizontal(|ui| {
                    ui.label("Stick output");
                    egui::ComboBox::from_id_salt("controller-stick")
                        .selected_text(if self.gamepad_edit.stick == Stick::Left {
                            "Left stick"
                        } else {
                            "Right stick"
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.gamepad_edit.stick,
                                Stick::Left,
                                "Left stick",
                            );
                            ui.selectable_value(
                                &mut self.gamepad_edit.stick,
                                Stick::Right,
                                "Right stick",
                            );
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("TOP click");
                    egui::ComboBox::from_id_salt("controller-click")
                        .selected_text(self.gamepad_edit.click.label())
                        .show_ui(ui, |ui| {
                            for click in Click::ALL {
                                ui.selectable_value(
                                    &mut self.gamepad_edit.click,
                                    click,
                                    click.label(),
                                );
                            }
                        });
                });
                ui.checkbox(&mut self.gamepad_edit.swap, "Swap X and Y");
                ui.checkbox(&mut self.gamepad_edit.invert_x, "Invert physical X");
                ui.checkbox(&mut self.gamepad_edit.invert_y, "Invert physical Y");
                ui.label("Applies in analog mode. Calibration follows the physical axes.");
                ui.horizontal(|ui| {
                    apply = ui.button("Apply").clicked();
                    defaults = ui.button("Use defaults").clicked();
                });
            });
        self.gamepad_open = open;
        if apply {
            self.set_gamepad(Some(self.gamepad_edit));
        }
        if defaults {
            self.set_gamepad(None);
        }
    }
    fn set_stick(&mut self, mode: Option<StickMode>) {
        let mut p = self.profile.clone();
        p.stick = mode;
        match self.send_profile(&p) {
            Ok(()) => {
                self.profile = p;
                self.report(Ok(()), "profile stick preference applied; Save keeps it");
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
}

fn is_modifier(k: Key) -> bool {
    matches!(
        k,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

/// egui key (physical, when winit reports it) to the Linux key name.
fn evdev_name(k: Key) -> Option<&'static str> {
    use Key::*;
    Some(match k {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        F13 => "F13",
        F14 => "F14",
        F15 => "F15",
        F16 => "F16",
        F17 => "F17",
        F18 => "F18",
        F19 => "F19",
        F20 => "F20",
        F21 => "F21",
        F22 => "F22",
        F23 => "F23",
        F24 => "F24",
        ArrowUp => "UP",
        ArrowDown => "DOWN",
        ArrowLeft => "LEFT",
        ArrowRight => "RIGHT",
        Escape => "ESC",
        Tab => "TAB",
        Backspace => "BACKSPACE",
        Enter => "ENTER",
        Space => "SPACE",
        Insert => "INSERT",
        Delete => "DELETE",
        Home => "HOME",
        End => "END",
        PageUp => "PAGEUP",
        PageDown => "PAGEDOWN",
        Minus => "MINUS",
        Equals | Plus => "EQUAL",
        Comma => "COMMA",
        Period => "DOT",
        Slash | Questionmark => "SLASH",
        Backslash | Pipe => "BACKSLASH",
        OpenBracket | OpenCurlyBracket => "LEFTBRACE",
        CloseBracket | CloseCurlyBracket => "RIGHTBRACE",
        Backtick => "GRAVE",
        Semicolon | Colon => "SEMICOLON",
        Quote => "APOSTROPHE",
        Exclamationmark => "1",
        IntlBackslash => "102ND",
        BrowserBack => "BACK",
        ShiftLeft => "LEFTSHIFT",
        ShiftRight => "RIGHTSHIFT",
        ControlLeft => "LEFTCTRL",
        ControlRight => "RIGHTCTRL",
        AltLeft => "LEFTALT",
        AltRight => "RIGHTALT",
        SuperLeft => "LEFTMETA",
        SuperRight => "RIGHTMETA",
        _ => return None,
    })
}

/// Profiles by focused window: the Windows… panel over i3's window list.
impl App {
    fn windows_window(&mut self, ctx: &egui::Context) {
        if !self.windows_open {
            self.wins_at = None;
            return;
        }
        if self
            .wins_at
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            self.wins_at = Some(Instant::now());
            match focus::windows() {
                Ok(w) => {
                    self.wins = w;
                    self.wins_err.clear();
                }
                Err(e) => self.wins_err = e,
            }
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        let mut open = self.windows_open;
        egui::Window::new("Windows and profiles")
            .collapsible(false)
            .resizable(true)
            .default_size([620.0, 480.0])
            .open(&mut open)
            .show(ctx, |ui| self.windows_contents(ui));
        self.windows_open = open;
    }

    fn windows_contents(&mut self, ui: &mut egui::Ui) {
        let mut on = self.rules.on;
        if ui
            .checkbox(&mut on, "Switch profiles by the focused window")
            .on_hover_text("Served by g13map-watch.service, the same watcher as the M-keys")
            .changed()
        {
            self.rules.on = on;
            let r = self
                .rules
                .save()
                .and_then(|_| Self::watch_unit(on || self.modes.on));
            self.report(
                r,
                if on {
                    "profiles follow the focused window now (g13map-watch.service enabled)"
                } else {
                    "profiles no longer follow the focused window"
                },
            );
        }
        self.modes_tick(ui);
        ui.label(
            RichText::new(
                "The focused window's class picks its profile; a window with no rule gets the \
                 default (sum 0). In M-Sum mode a lit sum overrides that and the LEDs show it; \
                 MR goes back to the window's profile and its own LEDs. Focusing this editor \
                 changes nothing.",
            )
            .weak(),
        );
        if !self.wins_err.is_empty() {
            ui.colored_label(Color32::from_rgb(255, 120, 100), &self.wins_err);
        }
        ui.add_space(6.0);
        let names = profile_names();
        // One row per class, in tree order, with the titles behind it.
        let mut classes: Vec<(String, Vec<&focus::Win>)> = vec![];
        for w in &self.wins {
            match classes.iter_mut().find(|(c, _)| *c == w.class) {
                Some((_, v)) => v.push(w),
                None => classes.push((w.class.clone(), vec![w])),
            }
        }
        let mut change: Option<(String, Option<String>)> = None;
        let rules = &self.rules;
        let combo =
            |ui: &mut egui::Ui, class: &str, change: &mut Option<(String, Option<String>)>| {
                let cur = rules.profile_for(class).map(String::from);
                egui::ComboBox::from_id_salt(("focus", class))
                    .selected_text(cur.clone().unwrap_or_else(|| "—".to_string()))
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(cur.is_none(), "—").clicked() {
                            *change = Some((class.to_string(), None));
                        }
                        for n in &names {
                            if ui
                                .selectable_label(cur.as_deref() == Some(n.as_str()), n)
                                .clicked()
                            {
                                *change = Some((class.to_string(), Some(n.clone())));
                            }
                        }
                    });
            };
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.label(RichText::new("Open now").strong());
                egui::Grid::new("focus-open")
                    .num_columns(3)
                    .spacing([12.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (class, wins) in &classes {
                            let focused = wins.iter().any(|w| w.focused);
                            let own = class == focus::EDITOR_CLASS;
                            let text = if focused {
                                format!("{class} (focused)")
                            } else {
                                class.clone()
                            };
                            ui.label(RichText::new(text).strong());
                            if own {
                                ui.label(RichText::new("(this editor)").weak());
                            } else {
                                combo(ui, class, &mut change);
                            }
                            let mut titles = wins[0].title.clone();
                            if titles.chars().count() > 48 {
                                titles = titles.chars().take(47).collect::<String>() + "…";
                            }
                            if wins.len() > 1 {
                                titles.push_str(&format!("  +{} more", wins.len() - 1));
                            }
                            ui.add(egui::Label::new(RichText::new(titles).weak()).truncate());
                            ui.end_row();
                        }
                    });
                let closed: Vec<String> = rules
                    .rules
                    .iter()
                    .map(|(c, _)| c.clone())
                    .filter(|c| !classes.iter().any(|(k, _)| k == c))
                    .collect();
                if !closed.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Rules for windows not open now").strong());
                    egui::Grid::new("focus-closed")
                        .num_columns(3)
                        .spacing([12.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for class in &closed {
                                ui.label(RichText::new(class).strong());
                                combo(ui, class, &mut change);
                                ui.label(RichText::new("not open").weak());
                                ui.end_row();
                            }
                        });
                }
            });
        if let Some((class, profile)) = change {
            self.rules.set(&class, profile.as_deref());
            let r = self.rules.save();
            self.report(
                r,
                "window rules saved; the watcher reads them within two seconds",
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revert_unbinds_unsaved_live_keys_without_saving_transition_commands() {
        let _sandbox = crate::test_support::Sandbox::new("revert-unsaved-key");
        let pipe = crate::test_support::PanelPipe::new();
        let saved = Profile::parse("# stick keys\nbind G1 KEY_A\n").0;
        crate::save("one", &saved).unwrap();
        crate::set_active("one").unwrap();
        let mut app = App::new("one".into()).unwrap();
        pipe.read(); // initial LCD
        app.select(Some("G2"));
        app.set_action(Some("KEY_B".into()));
        assert_eq!(String::from_utf8(pipe.read()).unwrap(), "bind G2 KEY_B\n");
        assert!(app.dirty());
        app.do_revert();
        assert!(!app.status_err, "{}", app.status);
        assert!(String::from_utf8(pipe.read())
            .unwrap()
            .contains("bind G2 KEY_RESERVED\n"));
        assert_eq!(app.profile, saved);
        assert_eq!(crate::load("one").unwrap().0, saved);
        assert_eq!(crate::active_name(), "one");
        assert!(!app.dirty());
    }
    #[test]
    fn gamepad_mapping_saves_reverts_resets_and_switches_per_profile() {
        let _sandbox = crate::test_support::Sandbox::new("profile-gamepad");
        let pipe = crate::test_support::PanelPipe::new();
        let profile = Profile::parse("# stick analog\nbind TOP KEY_C\n").0;
        crate::save("one", &profile).unwrap();
        crate::save("two", &profile).unwrap();
        crate::set_active("one").unwrap();
        let mut app = App::new("one".into()).unwrap();
        pipe.read();
        let custom = Mapping {
            stick: Stick::Right,
            click: Click::R3,
            swap: true,
            invert_x: true,
            invert_y: false,
        };
        app.set_gamepad(Some(custom));
        assert!(!app.status_err, "{}", app.status);
        pipe.read();
        assert!(app.dirty());
        assert_eq!(
            Mapping::read(&std::path::PathBuf::from(
                std::env::var_os("G13MAP_ANALOG_MAP").unwrap()
            ))
            .unwrap(),
            custom
        );
        app.do_save();
        assert_eq!(crate::load("one").unwrap().0.gamepad, Some(custom));
        app.set_gamepad(None);
        pipe.read();
        assert!(app.dirty());
        app.do_revert();
        pipe.read();
        assert_eq!(app.profile.gamepad, Some(custom));
        app.switch("two".into());
        pipe.read();
        pipe.read();
        assert_eq!(app.profile.gamepad, None);
        assert_eq!(
            Mapping::read(&std::path::PathBuf::from(
                std::env::var_os("G13MAP_ANALOG_MAP").unwrap()
            ))
            .unwrap(),
            Mapping::default()
        );
        drop(pipe);
        app.set_gamepad(Some(custom));
        assert!(app.status_err);
        assert_eq!(app.profile.gamepad, None);
    }
    #[test]
    fn stick_preference_saves_reverts_and_switches_with_top_ownership() {
        let _sandbox = crate::test_support::Sandbox::new("profile-stick");
        let pipe = crate::test_support::PanelPipe::new();
        let keys = Profile::parse("# stick keys\nbind TOP KEY_C\n").0;
        let analog = Profile::parse("# stick analog\nbind TOP KEY_D\n").0;
        crate::save("keys", &keys).unwrap();
        crate::save("analog", &analog).unwrap();
        crate::set_active("analog").unwrap();
        let mut app = App::new("analog".into()).unwrap();
        assert_eq!(pipe.read(), lcd::Bitmap::logo().0);
        app.switch("keys".into());
        let commands = String::from_utf8(pipe.read()).unwrap();
        assert!(commands.contains("bind TOP KEY_C\n"));
        assert_eq!(pipe.read(), lcd::Bitmap::logo().0);
        assert!(!app.analog && !daemon::analog_active());
        app.set_stick(Some(StickMode::Analog));
        assert!(!String::from_utf8(pipe.read()).unwrap().contains("bind TOP"));
        assert!(app.dirty() && app.analog);
        app.do_revert();
        assert!(String::from_utf8(pipe.read())
            .unwrap()
            .contains("bind TOP KEY_C"));
        assert!(!app.dirty() && !app.analog);
        app.set_stick(Some(StickMode::Analog));
        let _commands = pipe.read();
        app.do_save();
        assert_eq!(
            crate::load("keys").unwrap().0.stick,
            Some(StickMode::Analog)
        );
        // A missing daemon must not switch the active file or the editor's selection.
        drop(pipe);
        // Add an ordinary key to force a send even with no daemon baseline.
        let mut target = keys;
        target.binds.insert("G1".into(), "KEY_A".into());
        crate::save("failed", &target).unwrap();
        let before = app.name.clone();
        app.switch("failed".into());
        assert!(app.status_err);
        assert_eq!(app.name, before);
        assert_eq!(crate::active_name(), before);
    }
    #[test]
    fn external_profile_switch_replaces_editor_animation() {
        let _sandbox = crate::test_support::Sandbox::new("editor-switch");
        let pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("old", &[11, 12]);
        crate::test_support::picture("new", &[21, 22]);
        crate::set_active("old").unwrap();
        let mut app = App::new("old".into()).unwrap();
        pipe.expect_frames(&[11, 12], 3);
        crate::set_active("new").unwrap();
        app.follow_active();
        assert_eq!(app.name, "new");
        // Discard frames queued before the replacement's first frame.
        loop {
            let frame = pipe.read();
            if frame == vec![21; lcd::BYTES] {
                break;
            }
            assert!([11, 12].contains(&frame[0]), "unexpected frame");
            assert!(
                app.active_checked.elapsed() < Duration::from_secs(1),
                "old animation kept playing"
            );
        }
        pipe.expect_frames(&[21, 22], 4);
        drop(app);
    }

    #[test]
    fn editor_takes_over_playback_on_open() {
        let _sandbox = crate::test_support::Sandbox::new("editor-open");
        let pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("moving", &[31, 32]);
        let app = App::new("moving".into()).unwrap();
        assert!(lcd::editor_open());
        assert!(
            app.player.is_some(),
            "editor paused the watcher without taking over playback"
        );
        pipe.expect_frames(&[31, 32], 4);
        drop(app);
    }

    #[test]
    fn watcher_yields_first_frame_until_editor_closes() {
        let _sandbox = crate::test_support::Sandbox::new("editor-yield");
        let pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("moving", &[41, 42]);
        let lock = lcd::editor_lock().unwrap();
        let player = lcd::show(Some("moving"), true).unwrap();
        pipe.expect_quiet();
        drop(lock);
        pipe.expect_frames(&[41, 42], 4);
        drop(player);
    }

    #[test]
    fn manual_profile_switches_replace_animation_with_still_and_logo() {
        let _sandbox = crate::test_support::Sandbox::new("editor-manual");
        let pipe = crate::test_support::PanelPipe::new();
        crate::test_support::picture("moving", &[51, 52]);
        crate::test_support::picture("still", &[61]);
        crate::save("logo", &Profile::default()).unwrap();
        let mut app = App::new("moving".into()).unwrap();
        pipe.expect_frames(&[51, 52], 3);
        app.switch("still".into());
        assert!(!app.status_err, "{}", app.status);
        loop {
            let frame = pipe.read();
            if frame == vec![61; lcd::BYTES] {
                break;
            }
            // Profile binds are text reads; any intervening animation is the old one.
            if frame.len() == lcd::BYTES {
                assert!([51, 52].contains(&frame[0]));
            }
        }
        pipe.expect_quiet();
        app.switch("logo".into());
        assert!(!app.status_err, "{}", app.status);
        let logo = lcd::Bitmap::logo();
        loop {
            let frame = pipe.read();
            if frame == logo.0 {
                break;
            }
            assert_ne!(frame.len(), lcd::BYTES, "old animation restarted");
        }
        pipe.expect_quiet();
        drop(app);
    }

    #[test]
    fn every_captured_name_is_bindable() {
        for k in [
            Key::A,
            Key::F24,
            Key::ArrowUp,
            Key::Backtick,
            Key::IntlBackslash,
            Key::SuperLeft,
            Key::Num0,
        ] {
            let n = evdev_name(k).unwrap();
            assert!(KEYS.iter().any(|&(_, x)| x == n), "{n}");
        }
        assert!(crate::profile::CONTROLS.contains(&"G22"));
    }
}
