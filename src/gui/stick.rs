// SPDX-License-Identifier: GPL-3.0-or-later
//! The stick: keys or a game controller per profile, and the controller mapping.
use super::App;
use crate::gamepad::{Click, Mapping, Stick};
use crate::profile::StickMode;
use egui::RichText;

impl App {
    pub(super) fn set_gamepad(&mut self, mapping: Option<Mapping>) {
        let mut p = self.profile.clone();
        p.gamepad = mapping;
        match self.send_profile(&p) {
            Ok(()) => {
                self.profile = p;
                self.gamepad_edit = self.profile.gamepad.unwrap_or_default();
                self.report(Ok(()), "controller settings applied; Save keeps them");
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    /// What the stick is in this profile: the choice, and a word on the state now.
    fn stick_choice(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Stick:");
            let mut mode = self.profile.stick;
            let name = |m: Option<StickMode>| match m {
                None => "as it is now",
                Some(StickMode::Analog) => "a game controller",
                Some(StickMode::Keys) => "four keys",
            };
            egui::ComboBox::from_id_salt("profile-stick")
                .selected_text(name(mode))
                .show_ui(ui, |ui| {
                    for m in [None, Some(StickMode::Analog), Some(StickMode::Keys)] {
                        ui.selectable_value(&mut mode, m, name(m));
                    }
                })
                .response
                .on_hover_text(
                    "A game controller: games see an Xbox-style stick, through g13-analog.service. \
                     Four keys: pushing it presses the keys bound to its directions.",
                );
            if mode != self.profile.stick {
                self.set_stick(mode);
            }
        });
        let mapping = self.profile.gamepad.unwrap_or_default();
        ui.label(
            RichText::new(if self.analog {
                format!(
                    "A controller now; pressing it sends {}.",
                    mapping.click.label()
                )
            } else {
                "Keys now; its four directions and pressing it (TOP) take bindings.".to_string()
            })
            .weak(),
        );
    }
    /// Under the board: the stick's mode, in one line.
    pub(super) fn stick_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.stick_choice(ui);
        });
    }
    /// The pane for the stick itself (TOP on the board): its mode, and while it is a game
    /// controller, the controller settings. Returns true when it took the pane; otherwise
    /// pressing the stick is a key and the caller shows the key editor.
    pub(super) fn stick_pane(&mut self, ui: &mut egui::Ui) -> bool {
        ui.heading("Stick");
        self.stick_choice(ui);
        if !self.analog {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Pressing the stick (TOP) does:").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.clear_button(ui);
                });
            });
            return false;
        }
        ui.add_space(8.0);
        ui.label(RichText::new("Controller settings").strong());
        ui.horizontal(|ui| {
            ui.label("Acts as the");
            egui::ComboBox::from_id_salt("controller-stick")
                .selected_text(if self.gamepad_edit.stick == Stick::Left {
                    "left stick"
                } else {
                    "right stick"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.gamepad_edit.stick, Stick::Left, "left stick");
                    ui.selectable_value(&mut self.gamepad_edit.stick, Stick::Right, "right stick");
                });
        });
        ui.horizontal(|ui| {
            ui.label("Pressing it sends");
            egui::ComboBox::from_id_salt("controller-click")
                .selected_text(self.gamepad_edit.click.label())
                .show_ui(ui, |ui| {
                    for click in Click::ALL {
                        ui.selectable_value(&mut self.gamepad_edit.click, click, click.label());
                    }
                });
        });
        ui.checkbox(&mut self.gamepad_edit.swap, "Swap the axes");
        ui.checkbox(&mut self.gamepad_edit.invert_x, "Flip left and right");
        ui.checkbox(&mut self.gamepad_edit.invert_y, "Flip up and down");
        ui.horizontal(|ui| {
            let changed = Some(self.gamepad_edit) != self.profile.gamepad
                && !(self.profile.gamepad.is_none() && self.gamepad_edit == Mapping::default());
            if ui
                .add_enabled(changed, egui::Button::new("Apply"))
                .on_hover_text("Send these to the controller service now; Save keeps them")
                .clicked()
            {
                self.set_gamepad(Some(self.gamepad_edit));
            }
            if ui.button("Use defaults").clicked() {
                self.set_gamepad(None);
            }
        });
        true
    }
    pub(super) fn set_stick(&mut self, mode: Option<StickMode>) {
        let mut p = self.profile.clone();
        p.stick = mode;
        match self.send_profile(&p) {
            Ok(()) => {
                self.profile = p;
                self.report(Ok(()), "stick mode applied; Save keeps it");
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
}
