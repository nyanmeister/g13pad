// SPDX-License-Identifier: GPL-3.0-or-later
//! The LCD picture window: choose the slice of a picture that goes on the panel, like a
//! screenshot selector over the source, with the panel's result beside it, and tune the
//! level and dithering. Every change renders at once; the frame is kept and sent when the
//! pointer rests (at most a few times a second while dragging).
use crate::lcd::{self, Bitmap, Crop, Options, Source};
use egui::{
    Color32, ColorImage, CursorIcon, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, TextureId,
    TextureOptions, Vec2,
};
use std::time::{Duration, Instant};

/// A slice is never narrower than this many source pixels.
const MIN_W: f32 = 8.0;
const HANDLE: f32 = 9.0;
const SEND_GAP: Duration = Duration::from_millis(150);

enum Drag {
    /// Offset of the pointer from the slice's corner, in source pixels.
    Move(Vec2),
    /// The corner that stays put, in source pixels; the slice spans from it to the pointer.
    Corner(Pos2),
}

pub struct Adjust {
    src: Source,
    /// The history entry being written.
    pub name: String,
    opts: Options,
    start: Options,
    /// Keep the panel's shape while resizing (off: the slice stretches).
    lock: bool,
    bm: Bitmap,
    tex: TextureHandle,
    view: TextureHandle,
    /// Rendered but not yet kept and sent.
    pending: bool,
    sent: Instant,
    drag: Option<Drag>,
    /// What the view showed last frame, in source pixels (picture and slice together).
    bounds: Rect,
    /// Whether the first change has checked which profiles share the entry.
    forked: bool,
    pub open: bool,
}

impl Adjust {
    pub fn new(ctx: &egui::Context, src: Source, name: String, opts: Options) -> Adjust {
        let bm = src.render(&opts);
        let tex = ctx.load_texture(
            "lcd:adjust",
            ColorImage::from_gray([lcd::W, lcd::H], &bm.gray()),
            TextureOptions::NEAREST,
        );
        let source_view = src.view(opts.background);
        let (vw, vh) = source_view.dimensions();
        let view = ctx.load_texture(
            "lcd:adjust-source",
            ColorImage::from_gray([vw as usize, vh as usize], source_view.as_raw()),
            TextureOptions::LINEAR,
        );
        Adjust {
            src,
            name,
            opts,
            start: opts,
            lock: opts.crop.is_none_or(|c| c.panel_shaped()),
            bm,
            tex,
            view,
            pending: false,
            sent: Instant::now(),
            drag: None,
            bounds: Rect::ZERO,
            forked: false,
            open: true,
        }
    }
    /// The panel as it looks right now, for the board.
    pub fn texture(&self) -> TextureId {
        self.tex.id()
    }
    fn crop(&self) -> Crop {
        self.opts
            .crop
            .unwrap_or_else(|| Crop::fit(self.src.w, self.src.h))
    }
    /// Keeps the slice on the picture and no smaller than `MIN_W`.
    fn set_crop(&mut self, mut c: Crop) {
        if c.w < MIN_W {
            let k = MIN_W / c.w;
            c.w = MIN_W;
            c.h *= k;
        }
        if c.h < MIN_W / Crop::ASPECT {
            let k = MIN_W / Crop::ASPECT / c.h;
            c.h = MIN_W / Crop::ASPECT;
            c.w *= k;
        }
        let (sw, sh) = (self.src.w as f32, self.src.h as f32);
        c.x = c.x.clamp(1.0 - c.w, sw - 1.0);
        c.y = c.y.clamp(1.0 - c.h, sh - 1.0);
        self.opts.crop = Some(c);
    }
    /// A slice from `anchor` to `p`, panel-shaped when locked.
    fn span(&self, anchor: Pos2, p: Pos2) -> Crop {
        let (mut w, mut h) = ((p.x - anchor.x).abs(), (p.y - anchor.y).abs());
        if self.lock {
            if w / h.max(0.001) >= Crop::ASPECT {
                h = w / Crop::ASPECT;
            } else {
                w = h * Crop::ASPECT;
            }
        }
        Crop {
            x: if p.x >= anchor.x {
                anchor.x
            } else {
                anchor.x - w
            },
            y: if p.y >= anchor.y {
                anchor.y
            } else {
                anchor.y - h
            },
            w,
            h,
        }
    }
    fn rerender(&mut self) {
        self.bm = self.src.render(&self.opts);
        self.tex.set(
            ColorImage::from_gray([lcd::W, lcd::H], &self.bm.gray()),
            TextureOptions::NEAREST,
        );
        self.pending = true;
    }

    /// Draws the window. Returns the entry's name when the picture was kept this frame (the
    /// caller shows and sends it), or the error that stopped that.
    pub fn show(&mut self, ctx: &egui::Context, profile: &str) -> Result<Option<String>, String> {
        let before = self.opts;
        let mut open = self.open;
        egui::Window::new("LCD picture")
            .collapsible(false)
            .resizable(true)
            .default_size([780.0, 640.0])
            .open(&mut open)
            .show(ctx, |ui| self.contents(ui));
        self.open &= open;
        if self.opts != before {
            if self.opts.background != before.background {
                let view = self.src.view(self.opts.background);
                self.view.set(
                    ColorImage::from_gray(
                        [view.width() as usize, view.height() as usize],
                        view.as_raw(),
                    ),
                    TextureOptions::LINEAR,
                );
            }
            if !self.forked {
                // The first change: a picture other profiles show gets its own entry.
                if lcd::users(&self.name).iter().any(|p| p != profile) {
                    self.name = lcd::fork_name(&self.name);
                }
                self.forked = true;
            }
            self.rerender();
        }
        if self.pending {
            let resting = !ctx.input(|i| i.pointer.any_down());
            // An animation renders every frame when kept: only once the pointer rests.
            let mid_drag = self.sent.elapsed() >= SEND_GAP && !self.src.animated();
            if !self.open || resting || mid_drag {
                self.pending = false;
                self.sent = Instant::now();
                lcd::keep(
                    &self.name,
                    &self.src.path,
                    &self.src.render_all(&self.opts),
                    &self.opts,
                )?;
                return Ok(Some(self.name.clone()));
            }
            ctx.request_repaint_after(SEND_GAP);
        }
        Ok(None)
    }

    fn contents(&mut self, ui: &mut egui::Ui) {
        let (sw, sh) = (self.src.w, self.src.h);
        ui.horizontal(|ui| {
            ui.label(RichText::new(&self.name).strong());
            let size = if self.src.animated() {
                format!(
                    "{sw}×{sh}, {} frames, {:.1} s",
                    self.src.frame_count(),
                    self.src.duration().as_secs_f32()
                )
            } else {
                format!("{sw}×{sh}")
            };
            ui.label(RichText::new(size).weak());
            ui.separator();
            if ui
                .button("Fit")
                .on_hover_text("The whole picture, letterboxed")
                .clicked()
            {
                self.lock = true;
                self.opts.crop = Some(Crop::fit(sw, sh));
            }
            if ui
                .button("Fill")
                .on_hover_text("As much as fills the panel, centred")
                .clicked()
            {
                self.lock = true;
                self.opts.crop = Some(Crop::fill(sw, sh));
            }
            if ui
                .button("Whole")
                .on_hover_text("The whole picture, stretched")
                .clicked()
            {
                self.lock = false;
                self.opts.crop = Some(Crop::whole(sw, sh));
            }
            ui.checkbox(&mut self.lock, "Panel shape")
                .on_hover_text("Keep the slice 160:43 while resizing; off, it stretches");
            ui.separator();
            if ui
                .button("Reset")
                .on_hover_text("As it was when this window opened")
                .clicked()
            {
                self.opts = self.start;
                self.lock = self.start.crop.is_none_or(|c| c.panel_shaped());
            }
            if ui.button("Done").clicked() {
                self.open = false;
            }
        });
        ui.label(
            RichText::new(
                "Drag the slice to move it, its corners to resize, or draw a new one outside it. \
                 Wheel scrolls, Ctrl+wheel zooms at the pointer.",
            )
            .weak(),
        );
        let avail = ui.available_size();
        let view_h = (avail.y - 205.0).max(120.0);
        self.selector(ui, Vec2::new(avail.x, view_h));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("On the panel").weak());
                ui.image((self.tex.id(), Vec2::new(480.0, 129.0)));
            });
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.add_space(18.0);
                ui.add(
                    egui::Slider::new(&mut self.opts.level, 0..=255)
                        .text("Level")
                        .step_by(1.0),
                )
                .on_hover_text(
                    "Grey below this sets a bright pixel on the LCD; higher lights more pixels",
                );
                let mut pct = (self.opts.dither * 100.0).round();
                ui.add(
                    egui::Slider::new(&mut pct, 0.0..=100.0)
                        .text("Dither")
                        .suffix("%")
                        .fixed_decimals(0),
                )
                .on_hover_text(
                    "How much error Floyd–Steinberg carries on; 0 is a flat cut at the level",
                );
                self.opts.dither = pct / 100.0;
                ui.checkbox(&mut self.opts.invert, "Invert");
                ui.add(egui::Slider::new(&mut self.opts.background, 0..=255).text("Background"))
                    .on_hover_text("Grey outside the picture and behind transparency, before Level, Dither and Invert");
            });
        });
    }

    /// The source with the slice over it: drag, corners, a new slice, wheel and zoom.
    fn selector(&mut self, ui: &mut egui::Ui, size: Vec2) {
        let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let rect = resp.rect;
        painter.rect_filled(rect, 0.0, Color32::from_gray(self.opts.background));
        let (sw, sh) = (self.src.w as f32, self.src.h as f32);
        // The view holds the picture and the slice together (a slice can reach outside the
        // picture), at a scale frozen while dragging so the picture does not slide under
        // the pointer.
        let crop = self.crop();
        let bounds = if self.drag.is_some() {
            self.bounds
        } else {
            Rect::from_min_max(
                Pos2::new(crop.x.min(0.0), crop.y.min(0.0)),
                Pos2::new((crop.x + crop.w).max(sw), (crop.y + crop.h).max(sh)),
            )
        };
        self.bounds = bounds;
        let v =
            ((rect.width() - 4.0) / bounds.width()).min((rect.height() - 4.0) / bounds.height());
        let shown = Rect::from_center_size(rect.center(), bounds.size() * v);
        let img = Rect::from_min_size(
            shown.min - bounds.min.to_vec2() * v,
            Vec2::new(sw * v, sh * v),
        );
        let to_screen = |p: Pos2| img.min + p.to_vec2() * v;
        let to_src = |s: Pos2| ((s - img.min) / v).to_pos2();
        painter.image(
            self.view.id(),
            img,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        let cs = Rect::from_min_size(
            to_screen(Pos2::new(crop.x, crop.y)),
            Vec2::new(crop.w, crop.h) * v,
        );
        let corners = [
            cs.left_top(),
            cs.right_top(),
            cs.right_bottom(),
            cs.left_bottom(),
        ];
        let near_corner = |p: Pos2| {
            corners
                .iter()
                .position(|c| (*c - p).abs().max_elem() <= HANDLE)
        };
        // Interaction.
        if let Some(p) = resp.hover_pos() {
            let icon = match near_corner(p) {
                Some(0) | Some(2) => CursorIcon::ResizeNwSe,
                Some(_) => CursorIcon::ResizeNeSw,
                None if cs.contains(p) => CursorIcon::Move,
                None => CursorIcon::Crosshair,
            };
            ui.output_mut(|o| o.cursor_icon = icon);
        }
        if resp.drag_started() {
            // The drag is reported once the pointer has moved past egui's threshold: judge
            // the zone by where the button went down, not where the pointer is now.
            let origin = ui.input(|i| i.pointer.press_origin());
            if let Some(p) = origin.or_else(|| resp.interact_pointer_pos()) {
                self.drag = Some(match near_corner(p) {
                    Some(i) => Drag::Corner(to_src(corners[(i + 2) % 4])),
                    None if cs.contains(p) => Drag::Move(to_src(p) - Pos2::new(crop.x, crop.y)),
                    None => Drag::Corner(to_src(p)),
                });
            }
        }
        if resp.dragged() {
            if let (Some(d), Some(p)) = (&self.drag, resp.interact_pointer_pos()) {
                let p = to_src(p);
                let c = match d {
                    Drag::Move(off) => Crop {
                        x: p.x - off.x,
                        y: p.y - off.y,
                        ..crop
                    },
                    Drag::Corner(a) => self.span(*a, p),
                };
                self.set_crop(c);
            }
        }
        if resp.drag_stopped() {
            self.drag = None;
        }
        if resp.hovered() {
            let (scroll, zoom, at) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if scroll != Vec2::ZERO {
                // A wheel notch (40 points to egui) moves the slice a quarter of its own
                // size, whatever the view scale.
                self.set_crop(Crop {
                    x: crop.x - scroll.x / 40.0 * crop.w / 4.0,
                    y: crop.y - scroll.y / 40.0 * crop.h / 4.0,
                    ..crop
                });
            } else if zoom != 1.0 {
                let p = at
                    .map(to_src)
                    .unwrap_or(Pos2::new(crop.x + crop.w / 2.0, crop.y + crop.h / 2.0));
                self.set_crop(Crop {
                    x: p.x - (p.x - crop.x) / zoom,
                    y: p.y - (p.y - crop.y) / zoom,
                    w: crop.w / zoom,
                    h: crop.h / zoom,
                });
            }
        }
        // Draw the slice as it is after this frame's input.
        let crop = self.crop();
        let cs = Rect::from_min_size(
            to_screen(Pos2::new(crop.x, crop.y)),
            Vec2::new(crop.w, crop.h) * v,
        );
        let shade = Color32::from_black_alpha(110);
        let clip = painter.with_clip_rect(rect);
        for r in [
            Rect::from_min_max(rect.min, Pos2::new(rect.max.x, cs.min.y)),
            Rect::from_min_max(Pos2::new(rect.min.x, cs.max.y), rect.max),
            Rect::from_min_max(
                Pos2::new(rect.min.x, cs.min.y),
                Pos2::new(cs.min.x, cs.max.y),
            ),
            Rect::from_min_max(
                Pos2::new(cs.max.x, cs.min.y),
                Pos2::new(rect.max.x, cs.max.y),
            ),
        ] {
            if r.is_positive() {
                clip.rect_filled(r, 0.0, shade);
            }
        }
        clip.rect_stroke(
            cs,
            0.0,
            Stroke::new(1.0, Color32::from_gray(20)),
            egui::StrokeKind::Outside,
        );
        clip.rect_stroke(
            cs,
            0.0,
            Stroke::new(1.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
        for c in [
            cs.left_top(),
            cs.right_top(),
            cs.right_bottom(),
            cs.left_bottom(),
        ] {
            clip.rect_filled(
                Rect::from_center_size(c, Vec2::splat(HANDLE)),
                1.0,
                Color32::WHITE,
            );
            clip.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(HANDLE)),
                1.0,
                Stroke::new(1.0, Color32::from_gray(20)),
                egui::StrokeKind::Outside,
            );
        }
    }
}
