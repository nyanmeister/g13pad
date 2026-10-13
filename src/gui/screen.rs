// SPDX-License-Identifier: GPL-3.0-or-later
//! The LCD side of the editor: the pane (what the screen shows, the kept pictures), and the picture, text and animation views.
use super::{App, View};
use crate::adjust::Adjust;
use crate::lcd;
use crate::profile::Display;
use egui::{Color32, ColorImage, Key, RichText, Sense, TextureHandle, TextureOptions, Vec2};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

impl App {
    fn lcd_texture(ctx: &egui::Context, name: &str, bm: &lcd::Bitmap) -> TextureHandle {
        ctx.load_texture(
            format!("lcd:{name}"),
            ColorImage::from_gray([lcd::W, lcd::H], &bm.gray()),
            TextureOptions::NEAREST,
        )
    }
    /// Reloads the profile's image and the history list with their textures.
    pub(super) fn lcd_refresh(&mut self, ctx: &egui::Context) {
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
    pub(super) fn lcd_send_profile(&mut self) -> Result<(), String> {
        self.player = None; // stops and joins the old thread before the new first frame
        self.player = Some(lcd::show(self.profile.lcd.as_deref(), false)?);
        Ok(())
    }
    /// A live display went on, off or to another reader: the watcher shows it once the
    /// profile is saved and active, so the unit must run for it.
    fn set_display(&mut self, display: Display) {
        if display == self.profile.display {
            return;
        }
        self.profile.display = display;
        let mut msg = match display {
            Display::Picture => "LCD: the picture shows again (not saved yet)".to_string(),
            d => format!("LCD: {} (not saved yet)", d.label()),
        };
        let mut r = Ok(());
        if display != Display::Picture && !self.modes.on && !self.rules.on {
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
    /// (letterboxed, dithered) kept under its name; shows it and opens the picture view.
    pub(super) fn lcd_pick(&mut self, ctx: &egui::Context, path: PathBuf) {
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
                self.show_view(View::Picture);
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    /// Opens the picture view on the profile's picture, from the copy kept beside it.
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
                self.show_view(View::Picture);
            }
            Err(e) => self.report(Err(e), ""),
        }
    }
    /// The picture view: the slice, level and dithering of the profile's picture. A kept
    /// frame is shown and sent from here; Done (or leaving the view) ends it.
    pub(super) fn picture_view(&mut self, ui: &mut egui::Ui) {
        let Some(mut a) = self.adjust.take() else {
            self.view = View::Board;
            return;
        };
        match a.show(ui, &self.name) {
            Ok(Some(name)) => self.lcd_use(Some(name)),
            Ok(None) => {}
            Err(e) => self.report(Err(e), ""),
        }
        if a.open {
            self.adjust = Some(a);
        } else {
            self.lcd_dirty = true;
            self.view = View::Board;
        }
    }
    /// The picture view was left by other means (Escape, the bar): a frame still pending
    /// is kept, and the board's LCD goes back to the kept picture.
    pub(super) fn finish_picture(&mut self, ui: &mut egui::Ui) {
        let Some(mut a) = self.adjust.take() else {
            return;
        };
        a.open = false;
        match a.show(ui, &self.name) {
            Ok(Some(name)) => self.lcd_use(Some(name)),
            Ok(None) => {}
            Err(e) => self.report(Err(e), ""),
        }
        self.lcd_dirty = true;
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
    /// The pane for the screen: one choice of what it shows, then the picture (kept
    /// whatever shows) with the ways to change it and the pictures kept before.
    pub(super) fn lcd_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Screen");
        ui.label(RichText::new("The 160×43 black-and-white LCD above the keys.").weak());
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Shows:");
            let current = self.profile.display;
            let mut picked = current;
            egui::ComboBox::from_id_salt("lcd display")
                .height(480.0)
                .selected_text(current.label())
                .width(250.0)
                .show_ui(ui, |ui| {
                    let mut group = "";
                    for display in Display::ALL {
                        let g = group_of(display);
                        if g != group {
                            group = g;
                            ui.add_space(4.0);
                            ui.label(RichText::new(g).weak().small());
                        }
                        ui.selectable_value(&mut picked, display, display.label())
                            .on_hover_text(display.about());
                    }
                });
            self.set_display(picked);
        });
        ui.label(RichText::new(self.profile.display.about()).weak());
        if self.profile.display == Display::Audio {
            self.microphone_editor(ui);
        }
        ui.add_space(8.0);
        ui.label(
            RichText::new(if self.profile.display == Display::Picture {
                "Picture"
            } else {
                "Picture, kept for when the screen shows it again"
            })
            .strong(),
        );
        match &self.lcd_tex {
            Some(t) => {
                ui.image((t.id(), Vec2::new(320.0, 86.0)));
                let name = self.profile.lcd.as_deref().unwrap_or("");
                let caption = match &self.lcd_anim {
                    Some(a) => {
                        let total: Duration = a.frames.iter().map(|(d, _)| *d).sum();
                        format!(
                            "{name} · {} frames, {:.1} s",
                            a.frames.len(),
                            total.as_secs_f32()
                        )
                    }
                    None => name.to_string(),
                };
                ui.add(egui::Label::new(RichText::new(caption).weak()).truncate());
            }
            None => {
                let text = match &self.profile.lcd {
                    Some(n) => format!("picture '{n}' is missing from the kept pictures"),
                    None => "the driver's logo (no picture of its own)".to_string(),
                };
                ui.label(RichText::new(text).italics());
                ui.add_space(68.0);
            }
        }
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.picker.is_none(), egui::Button::new("Browse…"))
                .on_hover_text("Choose an image file: any common format, animated GIFs too")
                .clicked()
            {
                self.lcd_browse();
            }
            if ui
                .button("Text…")
                .on_hover_text("Write text for the screen; long text scrolls")
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
                self.text_opts = self
                    .profile
                    .lcd
                    .as_deref()
                    .map(crate::text_options::Options::load)
                    .unwrap_or_default();
                self.text_fonts = crate::text_options::fonts();
                self.text_preview_dirty = true;
                self.text_error.clear();
                self.show_view(View::Text);
            }
            if ui
                .button("Animations…")
                .on_hover_text("Built-in looping pixel art")
                .clicked()
            {
                self.show_view(View::Animations);
            }
            let has_source = self
                .profile
                .lcd
                .as_deref()
                .is_some_and(|n| lcd::source(n).is_some());
            if ui
                .add_enabled(has_source, egui::Button::new("Adjust…"))
                .on_hover_text("Crop, scale, brightness and dithering of this picture")
                .clicked()
            {
                self.lcd_adjust(ui.ctx());
            }
            if ui
                .button("Logo")
                .on_hover_text("No picture of its own: the driver's logo")
                .clicked()
            {
                self.lcd_use(None);
            }
        });
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.lcd_path)
                    .hint_text("or type a path, or drop a file on the window")
                    .desired_width(250.0),
            );
            let submit = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if (ui.button("Load").clicked() || submit) && !self.lcd_path.trim().is_empty() {
                self.lcd_pick(ui.ctx(), PathBuf::from(self.lcd_path.trim()));
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("Kept pictures, newest first. Click one to show it.").weak());
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
    pub(super) fn animations_view(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.anim_previews.is_empty() {
            for s in crate::art::SCENES {
                let a = s.animation();
                let tex = Self::lcd_texture(&ctx, &format!("scene:{}", s.name), a.first());
                self.anim_previews.push((a, tex, 0));
            }
            self.anim_at = Instant::now();
        }
        let elapsed = self.anim_at.elapsed();
        let mut pick = None;
        ui.label(
            RichText::new(
                "Looping pixel art drawn in code. Click one to keep it as a picture and show it.",
            )
            .weak(),
        );
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
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
            });
        ctx.request_repaint_after(Duration::from_millis(30));
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
                Ok(()) => {
                    self.lcd_use(Some(name.to_string()));
                    self.show_view(View::Board);
                }
                Err(e) => self.report(Err(e), ""),
            }
        }
    }
    /// Text for the screen, with a live preview; "Use this text" keeps it as a picture.
    pub(super) fn text_view(&mut self, ui: &mut egui::Ui) {
        use crate::text_options::Align;
        let ctx = ui.ctx().clone();
        if self.text_preview_dirty {
            self.text_preview_dirty = false;
            match crate::marquee::render_with(&self.lcd_text, &self.text_opts) {
                Ok(animation) => {
                    let texture = Self::lcd_texture(&ctx, "text-preview", animation.first());
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
        let mut create = false;
        let before = (self.lcd_text.clone(), self.text_opts.clone());
        ui.label(
            RichText::new("Up to 128 characters; Enter starts a new line. Tall text is scaled to fit the screen's 43 rows.")
                .weak(),
        );
        ui.add(
            egui::TextEdit::multiline(&mut self.lcd_text)
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .char_limit(128),
        );
        let font_label = self
            .text_opts
            .font
            .as_ref()
            .and_then(|path| {
                self.text_fonts
                    .iter()
                    .find(|(p, _)| p == path)
                    .map(|(_, label)| label.clone())
            })
            .unwrap_or_else(|| {
                self.text_opts
                    .font
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Roboto Mono (default)".into())
            });
        ui.horizontal(|ui| {
            ui.label("Font");
            egui::ComboBox::from_id_salt("text-font")
                .width(420.0)
                .selected_text(font_label)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.text_opts.font, None, "Roboto Mono (default)");
                    for (path, label) in &self.text_fonts {
                        ui.selectable_value(&mut self.text_opts.font, Some(path.clone()), label);
                    }
                });
        });
        ui.add(
            egui::Slider::new(&mut self.text_opts.size, 6.0..=72.0)
                .text("Size")
                .suffix(" pt"),
        );
        ui.add(
            egui::Slider::new(&mut self.text_opts.speed, 0..=120)
                .text("Scroll")
                .suffix(" px/s"),
        )
        .on_hover_text(
            "Zero shows a still slice; otherwise text wider than the screen scrolls sideways",
        );
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.text_opts.wrap, "Wrap long lines");
            ui.checkbox(&mut self.text_opts.invert, "Light on dark");
            ui.label("Align:");
            ui.selectable_value(&mut self.text_opts.align, Align::Left, "left");
            ui.selectable_value(&mut self.text_opts.align, Align::Center, "centre");
            ui.selectable_value(&mut self.text_opts.align, Align::Right, "right");
        });
        ui.separator();
        ui.label(RichText::new("On the screen").weak());
        if let Some((texture, animation, _)) = &self.text_preview {
            ui.image((texture.id(), Vec2::new(480.0, 129.0)));
            ui.label(RichText::new(format!("{} frame(s)", animation.frames.len())).weak());
        } else if self.lcd_text.trim().is_empty() {
            ui.label(RichText::new("Type something above to see it here.").weak());
        } else {
            ui.colored_label(Color32::from_rgb(255, 120, 100), &self.text_error);
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .button("Use this text")
                .on_hover_text(
                    "Keeps it as a picture and shows it; Save keeps the choice with the profile",
                )
                .clicked()
            {
                create = true;
            }
        });
        if before != (self.lcd_text.clone(), self.text_opts.clone()) {
            self.text_preview_dirty = true;
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if create {
            match crate::marquee::keep_with(&self.lcd_text, None, &self.text_opts) {
                Ok(name) => {
                    self.lcd_use(Some(name));
                    self.show_view(View::Board);
                }
                Err(e) => self.report(Err(e), ""),
            }
        }
    }
}

/// The heading a display is listed under in the pane's list.
fn group_of(display: Display) -> &'static str {
    match display {
        Display::Picture => "",
        Display::Health(_) => "Games",
        Display::Mpv | Display::Mpd => "Media",
        Display::Audio | Display::Pressure => "This computer",
    }
}
