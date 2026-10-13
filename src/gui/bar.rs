// SPDX-License-Identifier: GPL-3.0-or-later
//! The editor's top bar: profile choice, views, backlight, LEDs, Save and Revert.
use super::{App, View};
use crate::{daemon, profile_names};
use egui::RichText;
use std::time::{Duration, Instant};

impl App {
    pub(super) fn top_bar(&mut self, ui: &mut egui::Ui) {
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
            if ui
                .button("New (copy)")
                .on_hover_text("A new profile with this one's settings, under the name typed here")
                .clicked()
            {
                self.create();
            }
            if ui
                .button("Delete")
                .on_hover_text("Deletes this profile's file and switches to another")
                .clicked()
            {
                self.delete();
            }
            ui.separator();
            for (view, label, hover) in [
                (View::Board, "Board", "The pad: keys, stick and screen"),
                (
                    View::Windows,
                    "Window rules",
                    "Which profile each program gets when its window has focus, and the M-key switching",
                ),
                (
                    View::Obs,
                    "OBS",
                    "Overlay windows for OBS: the pad with its keys lit as pressed, or the LCD alone",
                ),
            ] {
                if ui
                    .selectable_label(self.view == view, label)
                    .on_hover_text(hover)
                    .clicked()
                {
                    self.show_view(view);
                }
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
            ui.label("Lights")
                .on_hover_text("The four small LEDs above the M-keys, as this profile sets them");
            if self.modes.on {
                ui.label(RichText::new("(while no sum is lit)").weak())
                    .on_hover_text(
                    "M-key switching is on: a lit sum takes the LEDs over; MR gives them back to the profile",
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
            if ui
                .add_enabled(dirty, egui::Button::new("Revert"))
                .on_hover_text("Back to the saved profile, on the pad too")
                .clicked()
            {
                self.do_revert();
            }
            if ui
                .button("Re-send")
                .on_hover_text(
                    "Send the whole profile to the pad again (after the driver restarted)",
                )
                .clicked()
            {
                self.send_all("profile re-sent to the driver");
            }
        });
    }
}
