// SPDX-License-Identifier: GPL-3.0-or-later
//! The editor window: the board on the left, the selected control's binding on the right.
//! Every edit goes to the daemon at once (so the key can be tried), Save writes the file.
use crate::adjust::Adjust;
use crate::board::Board;
use crate::focus::{self, Rules};
use crate::gamepad::Mapping;
use crate::keys::{self, action_label};
use crate::lcd;
use crate::modes::{self, Modes};
use crate::profile::{chord, chord_action, Profile, StickMode, UNBOUND};
use crate::{daemon, load, profile_names, save, set_active};
use egui::{Color32, ColorImage, Key, RichText, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

mod bar;
mod binding;
mod mic;
mod screen;
mod stick;
mod switching;
#[cfg(test)]
mod tests;

use binding::{evdev_name, is_modifier};
use mic::MicInputs;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Keys,
    Controller,
    Audio,
    Music,
    /// A program or script started on press (`run.rs`).
    Run,
    Raw,
}

/// What the left side of the window shows. The board is the editor; the rest were floating
/// windows stacked over it until 2026-10-12, and are one thing at a time now, with Escape
/// or the bar's Board button going back.
#[derive(Clone, Copy, PartialEq, Default)]
pub(super) enum View {
    #[default]
    Board,
    /// Window rules and M-key profile switching, served by the watcher.
    Windows,
    /// The OBS overlay windows and their options.
    Obs,
    /// The LCD picture being adjusted (`App::adjust`).
    Picture,
    /// Text for the LCD.
    Text,
    /// The built-in animations.
    Animations,
}

impl View {
    pub(super) fn title(self) -> &'static str {
        match self {
            View::Board => "Board",
            View::Windows => "Which profile for which window",
            View::Obs => "OBS overlay",
            View::Picture => "LCD picture",
            View::Text => "LCD text",
            View::Animations => "Built-in animations",
        }
    }
}

struct App {
    board: Board,
    name: String,
    profile: Profile,
    saved: Profile,
    microphones: MicInputs,
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
    view: View,
    /// The LCD picture being adjusted, while the picture view is up.
    adjust: Option<Adjust>,
    /// The built-in scenes (`art`) with live previews, rendered when the animations view
    /// first opens; each keeps the frame its texture shows.
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
    /// The program line being edited for the selected key (`# run KEY ...`).
    run_text: String,
    /// When that program runs (the radio row under the line).
    run_when: crate::run::When,
    /// A file dialog for a program, like `picker`.
    run_picker: Option<mpsc::Receiver<Result<PathBuf, String>>>,
    /// M-keys as profile selectors (`~/.config/g13map/modes`, served by `g13map watch`).
    modes: Modes,
    /// Profiles by focused window (`~/.config/g13map/focus`, same watcher), and the windows
    /// listed in that view.
    rules: Rules,
    wins: Vec<focus::Win>,
    wins_at: Option<Instant>,
    wins_err: String,
    /// When the active profile was last compared with the file (the watcher switches too).
    active_checked: Instant,
    /// When the adapter service was last asked whether it runs (a fork per ask).
    analog_checked: Instant,
    gamepad_edit: Mapping,
    /// The OBS overlay windows and their options.
    obs: crate::obs::Panel,
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
            microphones: MicInputs::default(),
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
            view: View::Board,
            adjust: None,
            anim_previews: vec![],
            anim_at: Instant::now(),
            lcd_text: String::new(),
            text_opts: crate::text_options::Options::default(),
            text_fonts: vec![],
            text_preview: None,
            text_preview_dirty: false,
            text_error: String::new(),
            picker: None,
            run_text: String::new(),
            run_when: crate::run::When::default(),
            run_picker: None,
            modes: Modes::load(),
            rules: Rules::load(),
            wins: vec![],
            wins_at: None,
            wins_err: String::new(),
            active_checked: Instant::now(),
            analog_checked: Instant::now(),
            gamepad_edit: Mapping::default(),
            obs: crate::obs::Panel::default(),
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
        let run = key.and_then(|k| self.profile.runs.get(k));
        self.run_text = run.map(|r| r.program.line()).unwrap_or_default();
        self.run_when = run.map(|r| r.when).unwrap_or_default();
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
            None if crate::gamepad::button_label(&action).is_some() => self.mode = Mode::Controller,
            None if crate::audio::Action::from_binding(&action).is_some() => {
                self.mode = Mode::Audio
            }
            None if crate::music::Action::from_binding(&action).is_some() => {
                self.mode = Mode::Music
            }
            None if crate::run::key_of_binding(&action).is_some() => self.mode = Mode::Run,
            None => self.mode = Mode::Raw,
        }
    }
    /// Sets (or clears) the selected control's action, and sends it to the daemon.
    fn set_action(&mut self, action: Option<String>) {
        let Some(key) = self.selected else { return };
        let controller = action
            .as_deref()
            .is_some_and(|a| crate::gamepad::button_label(a).is_some());
        let audio = action
            .as_deref()
            .is_some_and(|a| crate::audio::Action::from_binding(a).is_some());
        let music = action
            .as_deref()
            .is_some_and(|a| crate::music::Action::from_binding(a).is_some());
        let run = action
            .as_deref()
            .is_some_and(|a| crate::run::key_of_binding(a).is_some());
        if !run {
            // Any other action (or none) leaves no `# run` line behind.
            self.profile.runs.remove(key);
        }
        if audio || music || run {
            if let Err(e) = Self::watch_unit(true) {
                self.report(Err(e), "");
                return;
            }
        }
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
        let r = if controller {
            self.profile.stick = Some(StickMode::Analog);
            self.send_profile(&self.profile.clone())
        } else {
            daemon::send(&line)
        };
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
                self.gamepad_edit = self.profile.gamepad.unwrap_or_default();
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
        // The modifiers on the event itself: a synthetic Ctrl+S (xdotool) can release Ctrl
        // in the same frame, when the frame's modifier state already says no Ctrl.
        let save = ctx.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, egui::Event::Key { key: Key::S, pressed: true, modifiers, .. } if modifiers.ctrl)
            })
        });
        if save {
            self.do_save();
        }
        // Away from the board, Escape goes back to it and nothing else.
        if self.view != View::Board {
            if ctx.input(|i| i.key_pressed(Key::Escape)) && !ctx.egui_wants_keyboard_input() {
                self.show_view(View::Board);
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
        if let Some(rx) = &self.run_picker {
            match rx.try_recv() {
                Ok(Ok(p)) => {
                    self.run_picker = None;
                    self.run_text = crate::run::tilde(&p);
                }
                Ok(Err(e)) => {
                    self.run_picker = None;
                    self.report(Err(e), "");
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.run_picker = None,
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
        // The picture view was left (Escape, the bar): keep what it still had pending.
        if self.view != View::Picture {
            self.finish_picture(ui);
        }
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(4.0);
            match self.view {
                View::Board => self.board_view(ui),
                view => {
                    ui.horizontal(|ui| {
                        if ui
                            .button("← Board")
                            .on_hover_text("Back to the pad (Escape)")
                            .clicked()
                        {
                            self.show_view(View::Board);
                        }
                        ui.heading(view.title());
                    });
                    ui.add_space(6.0);
                    match view {
                        View::Board => {}
                        View::Windows => self.windows_view(ui),
                        View::Obs => self.obs.contents(ui),
                        View::Picture => self.picture_view(ui),
                        View::Text => self.text_view(ui),
                        View::Animations => self.animations_view(ui),
                    }
                }
            }
        });
    }
}

impl App {
    /// Switches the left side to `view`; leaving the picture view keeps what it changed.
    pub(super) fn show_view(&mut self, view: View) {
        if self.view == View::Picture && view != View::Picture {
            if let Some(a) = &mut self.adjust {
                a.open = false;
            }
        }
        self.view = view;
    }
    /// The pad: click a control to edit it on the right; below it, the stick and a few
    /// profile-wide settings.
    fn board_view(&mut self, ui: &mut egui::Ui) {
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
        ui.label(
            RichText::new(
                "Click a key, the stick or the screen to change it. Changes take effect at \
                 once; Save keeps them.",
            )
            .weak(),
        );
        self.stick_row(ui);
        let mut enabled = crate::overlay::enabled();
        if ui
            .checkbox(&mut enabled, "Show errors on the screen for five seconds")
            .on_hover_text("A problem with a picture or a live display is shown on the LCD, then the normal picture comes back")
            .changed()
        {
            let animated = self
                .profile
                .lcd
                .as_deref()
                .is_some_and(|n| lcd::frame_count(n) > 1);
            let r = crate::overlay::set_enabled(enabled).and_then(|_| {
                Self::watch_unit(enabled || self.modes.on || self.rules.on || animated)
            });
            self.report(r, "LCD error preference saved");
        }
    }
}
