// SPDX-License-Identifier: GPL-3.0-or-later
//! The right-hand pane for a key or thumb button: what it does, and the ways to change it (a key or chord, a controller button, a sound or music action, raw driver text).
use super::{App, Mode};
use crate::keys::{self, action_label, KEYS};
use crate::modes;
use crate::profile::MODIFIERS;
use egui::{Color32, Key, RichText};
use std::sync::mpsc;

impl App {
    pub(super) fn editor(&mut self, ui: &mut egui::Ui) {
        let Some(key) = self.selected else {
            ui.label("Click a key, the stick or the screen on the board.");
            return;
        };
        if key == "LCD" {
            self.lcd_editor(ui);
            return;
        }
        if key == "TOP" {
            if self.stick_pane(ui) {
                return;
            }
        } else {
            ui.horizontal(|ui| {
                ui.heading(key);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.clear_button(ui);
                });
            });
        }
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
                ui.label(RichText::new("does nothing yet").italics());
                ui.label(RichText::new(" ").small());
            }
        }
        if self.analog && key.starts_with("STICK_") {
            ui.colored_label(
                Color32::from_rgb(230, 170, 60),
                "The stick is a game controller in this profile, so its direction keys are not \
                 used. Set the stick to four keys (click the stick, or below the board) to use \
                 them.",
            );
        }
        ui.add_space(6.0);
        // Wrapped: a row wider than the pane squeezes the whole pane off the window.
        ui.horizontal_wrapped(|ui| {
            for (mode, label, hover) in [
                (
                    Mode::Keys,
                    "Key",
                    "A keyboard key, with Ctrl, Shift, Alt or Super held",
                ),
                (Mode::Controller, "Controller", "A game-controller button"),
                (
                    Mode::Audio,
                    "Sound",
                    "Mute or unmute the microphone, change volumes",
                ),
                (
                    Mode::Music,
                    "Music",
                    "Play, pause, skip and rate music (MPD)",
                ),
                (
                    Mode::Run,
                    "Program",
                    "Start a program or script (g13map-watch runs it)",
                ),
                (
                    Mode::Raw,
                    "Advanced",
                    "The driver's own action text: mouse buttons, release actions, commands",
                ),
            ] {
                ui.selectable_value(&mut self.mode, mode, label)
                    .on_hover_text(hover);
            }
        });
        ui.add_space(6.0);
        if self.mode != Mode::Keys {
            self.capture = false;
        }
        match self.mode {
            Mode::Keys => self.keys_editor(ui),
            Mode::Controller => self.controller_editor(ui),
            Mode::Audio => {
                self.microphone_editor(ui);
                ui.label(RichText::new("Mic actions use the microphone chosen above; volume actions use the default output.").weak());
                for action in crate::audio::Action::ALL {
                    let binding = action.binding();
                    let picked = self.profile.binds.get(key) == Some(&binding);
                    if ui.selectable_label(picked, action.label()).clicked() {
                        self.set_action(Some(binding));
                    }
                }
            }
            Mode::Music => {
                if key == "TOP" {
                    ui.label("Pressing the stick cannot take a music action; choose a G-key or a thumb button.");
                    return;
                }
                ui.label(
                    RichText::new("Controls the music daemon (MPD) through the g13music service.")
                        .weak(),
                );
                for action in crate::music::Action::ALL {
                    let binding = action.binding();
                    let picked = self.profile.binds.get(key) == Some(&binding);
                    if ui.selectable_label(picked, action.label()).clicked() {
                        self.set_action(Some(binding));
                    }
                }
            }
            Mode::Run => self.run_editor(ui, key),
            Mode::Raw => self.raw_editor(ui),
        }
    }
    /// The pane for a program on the key: the line as it will be saved, a file dialog, the
    /// scripts found in the two folders, and where the program resolves to before Set.
    fn run_editor(&mut self, ui: &mut egui::Ui, key: &'static str) {
        let [mine, shipped] = crate::run::dirs();
        ui.label(
            RichText::new(format!(
                "Starts a program or script when the key is pressed. A path (~ is home) or a \
                 name on PATH, then its arguments, split on spaces; a bare name is looked for \
                 in {} and {} first. It gets G13MAP_KEY and G13MAP_PROFILE in its environment \
                 and runs as a unit of your session, so it outlives the watcher. A script needs \
                 a #! line and the executable bit.",
                crate::run::tilde(&mine),
                shipped.display()
            ))
            .weak(),
        );
        ui.add_space(4.0);
        let mut submit = false;
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.run_text)
                    .hint_text("firefox · alacritty -e htop · ~/bin/lights.sh on")
                    .desired_width(240.0),
            );
            submit = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if ui.button("Browse…").clicked() {
                self.run_browse();
            }
        });
        let found = crate::run::listed();
        if !found.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Scripts:").weak());
                for name in found {
                    if ui.selectable_label(self.run_text == name, &name).clicked() {
                        self.run_text = name;
                    }
                }
            });
        }
        let check =
            crate::run::Program::parse(&self.run_text).and_then(|p| p.resolve().map(|at| (p, at)));
        match &check {
            Ok((_, at)) => {
                ui.label(RichText::new(at.display().to_string()).weak().small());
            }
            Err(e) if !self.run_text.trim().is_empty() => {
                ui.colored_label(Color32::from_rgb(230, 170, 60), e);
            }
            Err(_) => {}
        }
        let set = ui
            .add_enabled(check.is_ok(), egui::Button::new("Set"))
            .clicked();
        if let (true, Ok((program, _))) = (set || submit, check) {
            let binding = crate::run::binding(key);
            self.set_action(Some(binding.clone()));
            if self.profile.binds.get(key) == Some(&binding) {
                self.profile.runs.insert(key.to_string(), program.line());
                self.run_text = program.line();
            }
        }
    }
    /// The file dialog for a program, in a thread like the picture's; the path comes back
    /// through `run_picker` with the home directory as `~`.
    fn run_browse(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.run_picker = Some(rx);
        std::thread::spawn(move || {
            let out = std::process::Command::new("zenity")
                .env("GTK_USE_PORTAL", "1")
                .args(["--file-selection", "--title=Program or script to run"])
                .output();
            let r = match out {
                Ok(o) if o.status.success() => {
                    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if s.is_empty() {
                        Err("nothing chosen".to_string())
                    } else {
                        Ok(std::path::PathBuf::from(s))
                    }
                }
                Ok(o) if o.status.code() == Some(1) => Err("cancelled".to_string()),
                Ok(o) => Err(format!(
                    "zenity: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                )),
                Err(e) => Err(format!("zenity: {e}; type a path instead")),
            };
            let _ = tx.send(r);
        });
    }
    /// Clears the selected control's binding; on the heading row, right-aligned.
    pub(super) fn clear_button(&mut self, ui: &mut egui::Ui) {
        let bound = self
            .selected
            .is_some_and(|k| self.profile.binds.contains_key(k));
        if ui
            .add_enabled(bound, egui::Button::new("Clear"))
            .on_hover_text("Make this control do nothing")
            .clicked()
        {
            self.mods.clear();
            self.main = None;
            self.raw.clear();
            self.set_action(None);
        }
    }
    fn controller_editor(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "A button on the same virtual game controller as the stick. Choosing one makes \
                 the stick a game controller in this profile.",
            )
            .weak(),
        );
        if self.selected == Some("TOP") {
            ui.label("What pressing the stick sends is set by clicking the stick on the board.");
            return;
        }
        ui.label(RichText::new("Triggers are either fully pressed or released.").weak());
        let mut pick = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for &(action, label, _) in &crate::gamepad::BUTTONS {
                    let selected = self
                        .selected
                        .and_then(|k| self.profile.binds.get(k))
                        .is_some_and(|a| a == action);
                    if ui.selectable_label(selected, label).clicked() {
                        pick = Some(action);
                    }
                }
            });
        if let Some(action) = pick {
            self.raw = action.into();
            self.capture = false;
            self.set_action(Some(action.into()));
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
                .on_hover_text(
                    "Takes the next key you press, with Ctrl, Shift or Alt held if you hold them",
                )
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
        ui.label(
            RichText::new(
                "The driver's own action text. KEY_A+KEY_B is a chord; a second group \
                 (KEY_A KEY_B) is sent on release; MLEFT, MRIGHT, MMIDDLE, MSIDE and MEXTRA \
                 are mouse buttons; >text goes to the output pipe; !command to the driver.",
            )
            .weak(),
        );
        let r = ui.add(egui::TextEdit::singleline(&mut self.raw).desired_width(f32::INFINITY));
        let submit = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if ui.button("Set").clicked() || submit {
            let a = self.raw.trim().to_string();
            self.set_action(if a.is_empty() { None } else { Some(a) });
        }
    }
}

pub(super) fn is_modifier(k: Key) -> bool {
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
pub(super) fn evdev_name(k: Key) -> Option<&'static str> {
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
