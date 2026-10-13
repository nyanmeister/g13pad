// SPDX-License-Identifier: GPL-3.0-or-later
//! Microphone choice for the audio display and the mic bindings: the inputs are listed off the GUI thread.
use super::App;
use egui::Color32;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct MicInputs {
    inputs: crate::audio::Inputs,
    pending: Option<mpsc::Receiver<Result<crate::audio::Inputs, String>>>,
    checked: Option<Instant>,
    error: String,
}
impl MicInputs {
    fn poll(&mut self, refresh: bool) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(inputs) => {
                            self.inputs = inputs;
                            self.error.clear();
                        }
                        Err(error) => {
                            self.inputs = Default::default();
                            self.error = error;
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = "Microphone query stopped".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.pending.is_none()
            && (refresh
                || self
                    .checked
                    .is_none_or(|t| t.elapsed() >= Duration::from_secs(5)))
        {
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.checked = Some(Instant::now());
            std::thread::spawn(move || {
                let _ = tx.send(crate::audio::inputs());
            });
        }
    }
}

impl App {
    pub(super) fn microphone_editor(&mut self, ui: &mut egui::Ui) {
        self.microphones.poll(false);
        let combo_width = (ui.available_width() - 95.0).clamp(100.0, 225.0);
        let inputs = &self.microphones.inputs;
        let selected = self.profile.audio_mic.as_deref();
        let system_default = inputs
            .default
            .as_deref()
            .and_then(|name| inputs.devices.iter().find(|d| d.name == name));
        let label = match selected {
            None => system_default.map_or("System default".into(), |d| {
                format!("System default — {}", d.label())
            }),
            Some(name) => inputs
                .devices
                .iter()
                .find(|d| d.name == name)
                .map_or_else(|| format!("{name} (unavailable)"), |d| d.label()),
        };
        let mut picked = self.profile.audio_mic.clone();
        let mut refresh = false;
        ui.horizontal(|ui| {
            ui.label("Mic:");
            ui.scope(|ui| {
                ui.set_max_width(combo_width);
                egui::ComboBox::from_id_salt("audio microphone")
                    .width(combo_width)
                    .truncate()
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut picked, None, "System default");
                        for d in &inputs.devices {
                            ui.selectable_value(&mut picked, Some(d.name.clone()), d.label())
                                .on_hover_text(&d.name);
                        }
                    })
                    .response
                    .on_hover_text(
                        selected
                            .or(inputs.default.as_deref())
                            .unwrap_or("No microphone available"),
                    );
            });
            refresh = ui.button("Refresh").clicked();
        });
        if !self.microphones.error.is_empty() {
            ui.colored_label(Color32::YELLOW, &self.microphones.error);
        } else if selected.is_some_and(|name| !inputs.devices.iter().any(|d| d.name == name))
            && self.microphones.pending.is_none()
        {
            ui.label("Selected mic is unavailable. It stays selected and the pad shows red.");
        }
        if self.microphones.pending.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        if picked != self.profile.audio_mic {
            self.profile.audio_mic = picked;
            self.report(Ok(()), "Microphone selection changed (not saved yet)");
        }
        if refresh {
            self.microphones.poll(true);
        }
    }
}
