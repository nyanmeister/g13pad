// SPDX-License-Identifier: GPL-3.0-or-later
//! `g13map glass`: matching the glass by eye (asked 2026-10-08). A course of backlight
//! colours: the three LEDs alone, all three together, then every colour the profiles and
//! the health meter use. Each step sets the pad to that colour; the picture here is the
//! LCD as the model shows it, so the eye compares the glass with the screen and drags
//! the colour until they agree. A match goes into the table, the fit is solved from the
//! matches as they come (so the later steps start from a prediction, and confirming one
//! is a check of the model), and Save writes `~/.config/g13map/glass`, which the overlay
//! window, the LCD window and the OBS source pick up at once. The course wraps, for a
//! second round. Leaving puts the backlight back as it was found.
use crate::glass::{Glass, Rgb};
use crate::obs::{self, State};
use crate::{daemon, lcd, meter, profile::Profile};
use egui::{Color32, Pos2, Sense, Vec2};
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

/// Screen pixels per LCD pixel.
const PX: f32 = 4.0;

/// A SIGTERM (the editor's Stop, a session end) closes the window the ordinary way, so
/// the backlight is put back; only a kill -9 skips that.
static TERM: AtomicBool = AtomicBool::new(false);

extern "C" fn on_term(_: i32) {
    TERM.store(true, Ordering::Relaxed);
}

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}
const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;

struct Step {
    led: Rgb,
    about: String,
}

/// The course: fixed colours first, then the ones in use, each once.
fn course() -> Vec<Step> {
    let mut steps: Vec<Step> = [
        ([255, 0, 0], "the red LED alone"),
        ([0, 255, 0], "the green LED alone"),
        ([0, 0, 255], "the blue LED alone"),
        ([255, 255, 255], "all three LEDs at full"),
    ]
    .into_iter()
    .map(|(led, about)| Step {
        led,
        about: about.to_string(),
    })
    .collect();
    let mut add = |led: Rgb, about: String| {
        if led != [0, 0, 0] && !steps.iter().any(|s| s.led == led) {
            steps.push(Step { led, about });
        }
    };
    let mut names: Vec<String> = fs::read_dir(crate::profiles_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let p = e.path();
                    (p.extension().is_some_and(|x| x == "bind"))
                        .then(|| p.file_stem()?.to_str().map(str::to_string))
                        .flatten()
                })
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in &names {
        let text = fs::read_to_string(crate::profile_path(name)).unwrap_or_default();
        if let Some(rgb) = Profile::parse(&text).0.rgb {
            add(rgb, format!("profile {name}"));
        }
    }
    for band in meter::Tuning::load().bands {
        add(band.rgb, format!("meter band {}", band.name));
    }
    for name in &names {
        for band in meter::Tuning::load_for(name).bands {
            add(band.rgb, format!("meter band {} ({name})", band.name));
        }
    }
    steps
}

struct Calibrate {
    state: Arc<Mutex<State>>,
    course: Vec<Step>,
    at: usize,
    glass: Glass,
    /// The file's text as last loaded or saved: anything else is unsaved.
    on_disk: String,
    lcd_texture: egui::TextureHandle,
    /// The backlight as found, put back on leaving.
    restore: Option<Rgb>,
    status: String,
}

impl Calibrate {
    fn set_pad(&mut self, led: Rgb) {
        self.status = match daemon::send(&format!("rgb {} {} {}\n", led[0], led[1], led[2])) {
            Ok(()) => String::new(),
            Err(e) => format!("backlight not set: {e}"),
        };
    }

    fn step(&mut self, by: isize) {
        let n = self.course.len() as isize;
        self.at = ((self.at as isize + by).rem_euclid(n)) as usize;
        let led = self.course[self.at].led;
        self.set_pad(led);
    }

    fn set_match(&mut self, led: Rgb, shown: Rgb) {
        match self.glass.table.iter_mut().find(|(from, _)| *from == led) {
            Some(entry) => entry.1 = shown,
            None => self.glass.table.push((led, shown)),
        }
        self.glass.refit();
    }

    fn drop_match(&mut self, led: Rgb) {
        self.glass.table.retain(|(from, _)| *from != led);
        self.glass.refit();
    }

    fn save(&mut self) {
        self.status = match self.glass.save() {
            Ok(()) => {
                self.on_disk = self.glass.to_text();
                format!("saved {}", Glass::path().display())
            }
            Err(e) => e,
        };
    }
}

impl eframe::App for Calibrate {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(led) = self.restore {
            let _ = daemon::send(&format!("rgb {} {} {}\n", led[0], led[1], led[2]));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if TERM.load(Ordering::Relaxed) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(500));
        let state = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        let frame = state.lcd.clone().unwrap_or_else(lcd::Bitmap::blank);
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [lcd::W, lcd::H],
            &self.glass.render(&frame, state.backlight),
        );
        self.lcd_texture.set(image, egui::TextureOptions::NEAREST);
        let led = self.course[self.at].led;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Each step of the course sets the pad's backlight. Below is the LCD as the screen \
                     will draw it: drag the colour until it looks like the glass, then step on. The \
                     first three steps fix the fit; after them every step starts from its prediction, \
                     and confirming one is a check. Save writes the table for the overlay, the LCD \
                     window and the OBS source.",
                )
                .weak(),
            );
            ui.add_space(6.0);
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(lcd::W as f32, lcd::H as f32) * PX,
                Sense::hover(),
            );
            let full = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.painter()
                .image(self.lcd_texture.id(), rect, full, Color32::WHITE);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("◀").on_hover_text("the step before").clicked() {
                    self.step(-1);
                }
                if ui.button("▶").on_hover_text("the next step; the course wraps").clicked() {
                    self.step(1);
                }
                let step = &self.course[self.at];
                ui.strong(format!(
                    "{} of {}: LED {} {} {}, {}",
                    self.at + 1,
                    self.course.len(),
                    step.led[0],
                    step.led[1],
                    step.led[2],
                    step.about
                ));
            });
            if state.backlight != led {
                ui.horizontal(|ui| {
                    let [r, g, b] = state.backlight;
                    ui.colored_label(
                        Color32::LIGHT_RED,
                        format!("The pad shows {r} {g} {b} instead (a profile switch or the meter set it)."),
                    );
                    if ui.button("Set the pad").clicked() {
                        self.set_pad(led);
                    }
                });
            }
            ui.add_space(6.0);
            let matched = self.glass.matched(led);
            let predicted = self.glass.predicted(led);
            let shown = matched.or(predicted).unwrap_or(led);
            let mut colour = Color32::from_rgb(shown[0], shown[1], shown[2]);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(220.0);
                    if egui::color_picker::color_picker_color32(
                        ui,
                        &mut colour,
                        egui::color_picker::Alpha::Opaque,
                    ) {
                        self.set_match(led, [colour.r(), colour.g(), colour.b()]);
                    }
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(format!(
                        "On the screen: {} {} {}",
                        colour.r(),
                        colour.g(),
                        colour.b()
                    ));
                    match (matched, predicted) {
                        (Some(_), _) => {
                            ui.horizontal(|ui| {
                                ui.label("Matched by eye.");
                                if ui.button("Drop the match").clicked() {
                                    self.drop_match(led);
                                }
                            });
                        }
                        (None, Some(_)) => {
                            ui.horizontal(|ui| {
                                ui.label("Predicted from the fit; drag if the glass disagrees, or");
                                if ui.button("Confirm").clicked() {
                                    self.set_match(led, shown);
                                }
                            });
                        }
                        (None, None) => {
                            ui.label("Not translated yet: shown as it is. The three LEDs alone fix the fit.");
                        }
                    }
                    ui.add_space(8.0);
                    ui.add(
                        egui::Slider::new(&mut self.glass.lit, 0.0..=1.0)
                            .text("lit pixels toward white"),
                    );
                    ui.add_space(8.0);
                    let matches = self.glass.table.len();
                    ui.label(match (self.glass.fit, self.glass.worst_miss()) {
                        (Some(_), Some((miss, at))) => format!(
                            "Fit from {matches} matches; it misses one by at most {miss} (LED {} {} {}).",
                            at[0], at[1], at[2]
                        ),
                        (Some(_), None) => format!("Fit from {matches} matches."),
                        (None, _) => format!(
                            "No fit yet ({matches} matches; red, green and blue alone are needed)."
                        ),
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            self.save();
                        }
                        if self.glass.to_text() != self.on_disk {
                            ui.label(egui::RichText::new("unsaved").weak());
                        }
                    });
                    if !self.status.is_empty() {
                        ui.label(&self.status);
                    }
                });
            });
        });
    }
}

pub fn run() -> Result<String, String> {
    let course = course();
    let found = fs::read_to_string(obs::state_path("keys"))
        .ok()
        .map(|t| State::parse(&t).backlight);
    // Without a file the eye starts from nothing: the built-in GIF pairs are not matches.
    let on_disk = fs::read_to_string(Glass::path()).unwrap_or_default();
    let glass = if on_disk.is_empty() {
        Glass {
            table: vec![],
            ..Glass::default()
        }
    } else {
        Glass::parse(&on_disk)
    };
    let state = Arc::new(Mutex::new(State::default()));
    let size = [lcd::W as f32 * PX + 16.0, 470.0];
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(size)
            .with_min_inner_size(size)
            .with_app_id("g13map-glass")
            .with_title("G13 glass"),
        ..Default::default()
    };
    let watched = state.clone();
    // SAFETY: the handler only stores a flag.
    unsafe {
        let handler = on_term as extern "C" fn(i32) as usize;
        signal(SIGTERM, handler);
        signal(SIGINT, handler);
    }
    eframe::run_native(
        "g13map-glass",
        opts,
        Box::new(move |cc| {
            let blank = egui::ColorImage::filled([lcd::W, lcd::H], Color32::TRANSPARENT);
            let lcd_texture =
                cc.egui_ctx
                    .load_texture("g13-glass-lcd", blank, egui::TextureOptions::NEAREST);
            obs::watch(watched, cc.egui_ctx.clone());
            let mut app = Calibrate {
                state,
                course,
                at: 0,
                glass,
                on_disk,
                lcd_texture,
                restore: found,
                status: String::new(),
            };
            app.set_pad(app.course[0].led);
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())?;
    Ok(String::new())
}
