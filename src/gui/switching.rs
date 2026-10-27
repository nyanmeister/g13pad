// SPDX-License-Identifier: GPL-3.0-or-later
//! Profile switching done by the watcher: M-keys as profile selectors, and rules by focused window.
use super::App;
use crate::focus;
use crate::modes;
use crate::profile::UNBOUND;
use crate::{daemon, load, profile_names};
use egui::{Color32, RichText};
use std::time::{Duration, Instant};

/// The controller mapping the settings section edits, for a freshly loaded profile.
fn p_gamepad(p: &crate::profile::Profile) -> crate::gamepad::Mapping {
    p.gamepad.unwrap_or_default()
}

impl App {
    /// Enables or disables the user unit that runs `g13map watch`. `G13MAP_UNIT=0` skips it
    /// (offscreen tests must not touch the real session).
    pub(super) fn watch_unit(on: bool) -> Result<(), String> {
        if std::env::var("G13MAP_UNIT").is_ok_and(|v| v == "0") {
            return Ok(());
        }
        let verb = if on
            || crate::overlay::enabled()
            || crate::load(&crate::active_name()).is_ok_and(|(p, _)| p.needs_watcher())
        {
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
            .checkbox(&mut on, "M-keys switch profiles (M-Sum mode)")
            .on_hover_text("M1, M2 and M3 toggle and add up (M1+M3 = 5), MR clears; each sum picks a profile, over any window rule. The lights show the lit sum, or the profile's own pattern while none is lit. Done by g13map-watch.service.")
            .changed()
        {
            self.modes.on = on;
            let r = self
                .modes
                .save()
                .and_then(|_| Self::watch_unit(on || self.rules.on || self.profile.needs_watcher()))
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
                    "the M-keys switch profiles now (g13map-watch.service enabled)"
                } else {
                    "the M-keys are ordinary keys again"
                },
            );
        }
    }
    pub(super) fn modes_editor(&mut self, ui: &mut egui::Ui) -> bool {
        self.modes_tick(ui);
        if !self.modes.on {
            return false;
        }
        ui.label(
            RichText::new("Lit M-keys add up to a sum; each sum picks a profile. A sum with no profile uses sum 0.")
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
    pub(super) fn follow_active(&mut self) {
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
                self.gamepad_edit = p_gamepad(&self.profile);
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

impl App {
    /// The window rules view: the open windows by class, each with the profile it gets.
    pub(super) fn windows_view(&mut self, ui: &mut egui::Ui) {
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
        ui.ctx().request_repaint_after(Duration::from_secs(2));
        let mut on = self.rules.on;
        if ui
            .checkbox(&mut on, "Switch profiles by the focused window")
            .on_hover_text("Done by g13map-watch.service, the same watcher as the M-keys")
            .changed()
        {
            self.rules.on = on;
            let r = self.rules.save().and_then(|_| {
                Self::watch_unit(on || self.modes.on || self.profile.needs_watcher())
            });
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
        let mut show = self.panel_name;
        if ui
            .checkbox(&mut show, "Show the profile name in the panel applet")
            .on_hover_text(
                "Beside the device mark in the LXQt or XFCE panel; off leaves the mark alone. \
                 The applet picks the change up at its next refresh, within half a minute.",
            )
            .changed()
        {
            self.panel_name = show;
            let r = crate::panel::Settings { show_name: show }.save();
            self.report(
                r,
                if show {
                    "the panel applet shows the profile name"
                } else {
                    "the panel applet shows the mark alone"
                },
            );
        }
        ui.label(
            RichText::new(
                "Each program (by its window class) can have a profile: when one of its windows \
                 has focus the pad switches to that profile, and a window with no rule gets \
                 the default. Lit M-keys override this and the lights show which; MR gives \
                 the window its profile back. Focusing this editor changes nothing.",
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
