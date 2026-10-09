// SPDX-License-Identifier: GPL-3.0-or-later
//! The G13 diagram, traced from the product photo shipped with g13-git
//! (/usr/share/doc/g13-git/g13.png, 598x896): every control's centre, size and tilt was read
//! off that image; nominal coordinates are photo pixels minus (50, 80). Drawn with egui's
//! painter and hit-tested for selection. The G-keys fan out: wider pitch and larger keys toward
//! the bottom row, the outer keys tilted and a few pixels higher than the centre ones.
use crate::keys::action_label;
use crate::profile::Profile;
use egui::{
    epaint::TextShape, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke,
    TextureId, Ui, Vec2,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Key,
    /// Stick zone, live only in KEYS mode.
    Zone,
    /// The stick click: L3 while the analog adapter runs.
    Top,
    /// Drawn for orientation, not bindable (the backlight button).
    Deco,
}

pub struct Spot {
    pub name: &'static str,
    pub centre: Pos2,
    pub size: Vec2,
    /// Tilt in radians, positive = clockwise on screen.
    pub angle: f32,
    pub kind: Kind,
    /// One line for the tooltip.
    pub about: &'static str,
}

/// The photo outline reaches left/above the control origin; `OFF` shifts everything drawn.
pub(crate) const OFF: Vec2 = Vec2::new(32.0, 5.0);
pub(crate) const W: f32 = 575.0;
pub(crate) const H: f32 = 816.0;
pub(crate) const LCD: Rect = Rect::from_min_max(Pos2::new(145.0, 15.0), Pos2::new(355.0, 87.0));
pub(crate) const STICK_CENTRE: Pos2 = Pos2::new(462.0, 478.0);
const STICK_SQUARE: Rect = Rect::from_min_max(Pos2::new(426.0, 442.0), Pos2::new(498.0, 514.0));
/// Coordinates follow the 598×896 upstream product photograph, minus (50, 80).
/// Retain the full palm rest and the thumbstick's asymmetric outer edge.
pub(crate) const OUTLINE: [[f32; 2]; 51] = [
    [117., 0.],
    [173., -3.],
    [277., -3.],
    [383., 0.],
    [409., 10.],
    [439., 31.],
    [464., 63.],
    [484., 110.],
    [508., 191.],
    [524., 243.],
    [529., 264.],
    [527., 280.],
    [515., 304.],
    [457., 374.],
    [484., 402.],
    [504., 436.],
    [516., 477.],
    [519., 511.],
    [514., 540.],
    [501., 557.],
    [439., 588.],
    [454., 621.],
    [474., 662.],
    [477., 695.],
    [472., 723.],
    [458., 747.],
    [393., 770.],
    [328., 790.],
    [264., 799.],
    [199., 788.],
    [121., 766.],
    [42., 742.],
    [19., 724.],
    [7., 693.],
    [5., 672.],
    [12., 650.],
    [107., 528.],
    [120., 493.],
    [117., 460.],
    [-20., 280.],
    [-29., 262.],
    [-27., 245.],
    [-17., 224.],
    [4., 178.],
    [20., 123.],
    [31., 85.],
    [42., 57.],
    [58., 38.],
    [78., 21.],
    [97., 9.],
    [107., 4.],
];

/// Ear clipping preserves the concave waist and stick lobe. egui's polygon fill accepts
/// convex polygons only; feeding it the whole device silently fills across the waist.
fn triangles(points: &[[f32; 2]]) -> Vec<[u32; 3]> {
    let cross = |a: usize, b: usize, c: usize| {
        (points[b][0] - points[a][0]) * (points[c][1] - points[a][1])
            - (points[b][1] - points[a][1]) * (points[c][0] - points[a][0])
    };
    let mut remaining: Vec<usize> = (0..points.len()).collect();
    let mut result = Vec::with_capacity(points.len() - 2);
    while remaining.len() > 3 {
        let ear = (0..remaining.len())
            .find(|&i| {
                let a = remaining[(i + remaining.len() - 1) % remaining.len()];
                let b = remaining[i];
                let c = remaining[(i + 1) % remaining.len()];
                cross(a, b, c) > 0.0
                    && remaining.iter().all(|&p| {
                        p == a
                            || p == b
                            || p == c
                            || cross(a, b, p) < 0.0
                            || cross(b, c, p) < 0.0
                            || cross(c, a, p) < 0.0
                    })
            })
            .expect("device outline must be a simple clockwise polygon");
        let a = remaining[(ear + remaining.len() - 1) % remaining.len()];
        let b = remaining[ear];
        let c = remaining[(ear + 1) % remaining.len()];
        result.push([a as u32, b as u32, c as u32]);
        remaining.remove(ear);
    }
    result.push([
        remaining[0] as u32,
        remaining[1] as u32,
        remaining[2] as u32,
    ]);
    result
}

fn spot(
    name: &'static str,
    cx: f32,
    cy: f32,
    w: f32,
    h: f32,
    kind: Kind,
    about: &'static str,
) -> Spot {
    Spot {
        name,
        centre: Pos2::new(cx, cy),
        size: Vec2::new(w, h),
        angle: 0.0,
        kind,
        about,
    }
}

pub fn spots() -> Vec<Spot> {
    // (name, centre x, row) per G-key; rows carry y, key size, arc lift and tilt.
    const G: [(&str, f32, usize); 22] = [
        ("G1", 62.0, 0),
        ("G2", 125.0, 0),
        ("G3", 187.0, 0),
        ("G4", 249.0, 0),
        ("G5", 309.0, 0),
        ("G6", 370.0, 0),
        ("G7", 435.0, 0),
        ("G8", 62.0, 1),
        ("G9", 122.0, 1),
        ("G10", 185.0, 1),
        ("G11", 247.0, 1),
        ("G12", 310.0, 1),
        ("G13", 372.0, 1),
        ("G14", 435.0, 1),
        ("G15", 112.0, 2),
        ("G16", 180.0, 2),
        ("G17", 249.0, 2),
        ("G18", 315.0, 2),
        ("G19", 382.0, 2),
        ("G20", 170.0, 3),
        ("G21", 246.0, 3),
        ("G22", 325.0, 3),
    ];
    const ROWS: [(f32, f32, f32, f32); 4] = [
        // y, width, height, arc lift at the outer keys
        (220.0, 48.0, 38.0, 6.0),
        (275.0, 50.0, 38.0, 8.0),
        (340.0, 55.0, 40.0, 3.0),
        (397.0, 62.0, 42.0, 0.0),
    ];
    let mut v = vec![];
    for (name, cx, row) in G {
        let (y, w, h, lift) = ROWS[row];
        let t = (cx - 249.0) / 187.0; // -1 at G1, +1 at G7
        let mut s = spot(name, cx, y - lift * t * t, w, h, Kind::Key, "G-key");
        s.angle = -3.5f32.to_radians() * t; // outer ends raised
        v.push(s);
    }
    v.push(spot(
        "BD",
        105.0,
        123.0,
        24.0,
        24.0,
        Kind::Key,
        "Round button left of the LCD keys",
    ));
    for (i, n) in ["L1", "L2", "L3", "L4"].iter().enumerate() {
        v.push(spot(
            n,
            177.0 + i as f32 * 49.5,
            123.0,
            44.0,
            13.0,
            Kind::Key,
            "LCD soft key",
        ));
    }
    v.push(spot(
        "LIGHT",
        395.0,
        123.0,
        24.0,
        24.0,
        Kind::Deco,
        "Backlight button (firmware; not bindable)",
    ));
    for (i, n) in ["M1", "M2", "M3", "MR"].iter().enumerate() {
        v.push(spot(
            n,
            137.0 + i as f32 * 75.0,
            157.0,
            66.0,
            13.0,
            Kind::Key,
            "Mode key (has an LED)",
        ));
    }
    v.push(spot(
        "LEFT",
        400.0,
        480.0,
        30.0,
        94.0,
        Kind::Key,
        "Tall button beside the stick",
    ));
    v.push(spot(
        "DOWN",
        467.0,
        548.0,
        80.0,
        34.0,
        Kind::Key,
        "Wide button below the stick",
    ));
    // These thumb buttons follow the sloping sides of the stick well in the photograph.
    v.iter_mut().find(|s| s.name == "LEFT").unwrap().angle = 10_f32.to_radians();
    v.iter_mut().find(|s| s.name == "DOWN").unwrap().angle = -28_f32.to_radians();
    let s = STICK_SQUARE;
    let (x, y, w, h) = (s.min.x, s.min.y, s.width(), s.height());
    let zone = |name, rect: Rect, about| Spot {
        name,
        centre: rect.center(),
        size: rect.size(),
        angle: 0.0,
        kind: Kind::Zone,
        about,
    };
    let r = |x, y, w, h| Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
    v.push(zone(
        "STICK_LEFT",
        r(x, y, w * 0.2, h),
        "Stick zone x 0–0.2 (KEYS mode only)",
    ));
    v.push(zone(
        "STICK_RIGHT",
        r(x + w * 0.8, y, w * 0.2, h),
        "Stick zone x 0.8–1 (KEYS mode only)",
    ));
    let mid = |y0: f32, y1: f32| r(x + w * 0.2, y + h * y0, w * 0.6, h * (y1 - y0));
    v.push(zone(
        "STICK_PAGEUP",
        mid(0.0, 0.1),
        "Stick zone y 0–0.1, full width (KEYS mode only)",
    ));
    v.push(zone(
        "STICK_UP",
        mid(0.1, 0.3),
        "Stick zone y 0.1–0.3, full width (KEYS mode only)",
    ));
    v.push(zone(
        "STICK_DOWN",
        mid(0.7, 0.9),
        "Stick zone y 0.7–0.9, full width (KEYS mode only)",
    ));
    v.push(zone(
        "STICK_PAGEDOWN",
        mid(0.9, 1.0),
        "Stick zone y 0.9–1, full width (KEYS mode only)",
    ));
    v.push(Spot {
        name: "TOP",
        centre: STICK_CENTRE,
        size: Vec2::splat(36.0),
        angle: 0.0,
        kind: Kind::Top,
        about: "Stick click",
    });
    v
}

pub struct Board {
    pub spots: Vec<Spot>,
    pub layout: std::collections::HashMap<String, String>,
    triangles: Vec<[u32; 3]>,
}

fn rot(v: Vec2, a: f32) -> Vec2 {
    let (s, c) = a.sin_cos();
    Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

impl Board {
    pub fn new() -> Self {
        Board {
            spots: spots(),
            layout: Default::default(),
            triangles: triangles(&OUTLINE),
        }
    }

    /// Draws the board into the available space, returns the clicked control if any ("LCD"
    /// for the panel) and whether it was a double click. `lcd` is the current LCD image as a texture, if there is one;
    /// `modes_on` shows the M-keys as profile selectors.
    pub fn show(
        &self,
        ui: &mut Ui,
        profile: &Profile,
        selected: Option<&str>,
        analog: bool,
        lcd: Option<TextureId>,
        modes_on: bool,
    ) -> Option<(&'static str, bool)> {
        let avail = ui.available_size();
        // Leave room for the instructions, stick controls and LCD-error checkbox below.
        let scale = (avail.x / W).min((avail.y - 78.0) / H).clamp(0.5, 1.6);
        let (resp, painter) = ui.allocate_painter(Vec2::new(W * scale, H * scale), Sense::click());
        let origin = resp.rect.min;
        let map = |p: Pos2| origin + (p.to_vec2() + OFF) * scale;
        let [br, bg, bb] = profile.rgb.unwrap_or([31, 0, 127]);
        let back = Color32::from_rgb(br, bg, bb);
        let tint = |a: u8| Color32::from_rgba_unmultiplied(back.r(), back.g(), back.b(), a);
        // body
        let pts = |o: &[[f32; 2]]| {
            o.iter()
                .map(|[x, y]| map(Pos2::new(*x, *y)))
                .collect::<Vec<Pos2>>()
        };
        let mesh = egui::Mesh {
            vertices: pts(&OUTLINE)
                .into_iter()
                .map(|pos| egui::epaint::Vertex {
                    pos,
                    uv: egui::epaint::WHITE_UV,
                    color: Color32::from_gray(30),
                })
                .collect(),
            indices: self.triangles.iter().flatten().copied().collect(),
            ..Default::default()
        };
        painter.add(mesh);
        painter.add(Shape::closed_line(
            pts(&OUTLINE[..]),
            Stroke::new(1.5, Color32::from_gray(70)),
        ));
        painter.text(
            map(Pos2::new(250.0, 647.0)),
            Align2::CENTER_CENTER,
            "G13",
            FontId::proportional(15.0 * scale),
            Color32::from_gray(58),
        );
        // LCD
        let lcd_rect = Rect::from_min_max(map(LCD.min), map(LCD.max));
        let hover = resp.hover_pos();
        let lcd_hover = hover.is_some_and(|p| lcd_rect.contains(p));
        painter.rect_filled(
            lcd_rect,
            CornerRadius::same(3),
            Color32::from_rgb(20, 24, 20),
        );
        match lcd {
            Some(id) => {
                // The panel's true aspect inside the bezel.
                let img = Rect::from_center_size(
                    lcd_rect.center(),
                    Vec2::new(lcd_rect.width(), lcd_rect.width() * 43.0 / 160.0),
                );
                painter.image(
                    id,
                    img,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    lcd_rect.center(),
                    Align2::CENTER_CENTER,
                    "LCD 160×43",
                    FontId::proportional(12.0 * scale),
                    Color32::from_gray(140),
                );
            }
        }
        painter.rect_filled(lcd_rect, CornerRadius::same(3), tint(70));
        let lcd_stroke = if selected == Some("LCD") {
            Stroke::new(2.5 * scale, Color32::from_rgb(255, 200, 60))
        } else {
            Stroke::new(1.0, Color32::from_gray(if lcd_hover { 140 } else { 90 }))
        };
        painter.rect_stroke(
            lcd_rect,
            CornerRadius::same(3),
            lcd_stroke,
            egui::StrokeKind::Outside,
        );
        let mut clicked = None;
        if lcd_hover {
            resp.clone()
                .on_hover_text("LCD 160×43: click to choose its picture");
            if resp.clicked() {
                clicked = Some(("LCD", false));
            }
        }
        // stick well and cap
        let well =
            Rect::from_min_max(map(STICK_SQUARE.min), map(STICK_SQUARE.max)).expand(4.0 * scale);
        painter.rect_filled(well, CornerRadius::same(8), Color32::from_gray(36));
        for s in &self.spots {
            let c = map(s.centre);
            let half = s.size * scale / 2.0;
            let locked = analog && s.kind == Kind::Top;
            let inert = analog && s.kind == Kind::Zone;
            let is_sel = selected == Some(s.name);
            let is_hover = hover.is_some_and(|p| {
                let q = rot(p - c, -s.angle);
                match s.kind {
                    Kind::Top => q.length() <= half.x,
                    _ => q.x.abs() <= half.x && q.y.abs() <= half.y,
                }
            });
            let action = profile.binds.get(s.name).map(String::as_str);
            let face = match s.kind {
                _ if inert => Color32::from_gray(48),
                Kind::Deco => Color32::from_gray(44),
                Kind::Key => Color32::from_gray(if is_hover { 96 } else { 78 }),
                _ => Color32::from_gray(if is_hover { 80 } else { 64 }),
            };
            let stroke = if is_sel {
                Stroke::new(2.5 * scale, Color32::from_rgb(255, 200, 60))
            } else {
                Stroke::new(
                    1.0,
                    Color32::from_gray(if inert || s.kind == Kind::Deco {
                        70
                    } else {
                        115
                    }),
                )
            };
            let mode_key = modes_on && crate::modes::KEYS.contains(&s.name);
            let bound = (action.is_some() || mode_key) && !inert && s.kind != Kind::Deco;
            let fill_tint = tint(if locked { 40 } else { 90 });
            let round = s.kind == Kind::Top || (s.kind != Kind::Zone && s.size.x == s.size.y);
            if round {
                painter.circle(c, half.x, face, stroke);
                if bound {
                    painter.circle_filled(c, half.x, fill_tint);
                }
            } else {
                let pts: Vec<Pos2> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .iter()
                    .map(|(sx, sy)| c + rot(Vec2::new(half.x * sx, half.y * sy), s.angle))
                    .collect();
                painter.add(Shape::convex_polygon(pts.clone(), face, stroke));
                if bound {
                    painter.add(Shape::convex_polygon(pts, fill_tint, Stroke::NONE));
                }
            }
            let fg = if inert || s.kind == Kind::Deco {
                Color32::from_gray(120)
            } else {
                Color32::from_gray(235)
            };
            let shown = if locked {
                profile
                    .gamepad
                    .unwrap_or_default()
                    .click
                    .label()
                    .to_string()
            } else if mode_key {
                match s.name {
                    "M1" => "+1",
                    "M2" => "+2",
                    "M3" => "+4",
                    _ => "clear",
                }
                .to_string()
            } else if s.kind == Kind::Deco {
                String::new()
            } else {
                action
                    .map(|a| action_label(a, &self.layout))
                    .unwrap_or_else(|| "—".into())
            };
            let name = match s.name.strip_prefix("STICK_") {
                Some("PAGEUP") => "PGUP",
                Some("PAGEDOWN") => "PGDN",
                Some(z) => z,
                None => s.name,
            };
            let big = s.kind == Kind::Key && s.size.y >= 30.0;
            if big {
                // name top-left, binding bottom-centre, both in the key's tilted frame
                let nf = FontId::proportional(11.0 * scale);
                let galley = painter.layout_no_wrap(name.to_string(), nf, fg);
                let pos = c + rot(
                    Vec2::new(-half.x + 4.0 * scale, -half.y + 2.0 * scale),
                    s.angle,
                );
                painter.add(TextShape::new(pos, galley, fg).with_angle(s.angle));
                let bf = FontId::proportional(11.0 * scale);
                let colour = if action.is_some() {
                    Color32::WHITE
                } else {
                    Color32::from_gray(150)
                };
                let galley = painter.layout_no_wrap(clip(&shown, s.size.x / 6.2), bf, colour);
                let size = galley.size();
                let pos = c + rot(
                    Vec2::new(-size.x / 2.0, half.y - size.y - 3.0 * scale),
                    s.angle,
                );
                painter.add(TextShape::new(pos, galley, colour).with_angle(s.angle));
            } else {
                let text = match s.kind {
                    Kind::Deco => String::new(),
                    Kind::Top => format!("{name}: {shown}"),
                    _ if s.size.y < 16.0 || s.kind == Kind::Zone => format!("{name}: {shown}"),
                    _ => format!("{name}\n{shown}"),
                };
                let font = FontId::proportional(9.0 * scale);
                if s.size.y > s.size.x * 1.5 {
                    // narrow and tall: rotated text, read bottom to top
                    let galley = painter.layout_no_wrap(clip(&text, s.size.y / 5.2), font, fg);
                    let pos = c + Vec2::new(-galley.size().y / 2.0, galley.size().x / 2.0);
                    painter.add(
                        TextShape::new(pos, galley, fg).with_angle(-std::f32::consts::FRAC_PI_2),
                    );
                } else {
                    let max = if text.contains('\n') {
                        s.size.x / 4.6
                    } else {
                        s.size.x / 5.0
                    };
                    let text = text
                        .split('\n')
                        .map(|l| clip(l, max))
                        .collect::<Vec<_>>()
                        .join("\n");
                    painter.text(c, Align2::CENTER_CENTER, text, font, fg);
                }
            }
            if is_hover {
                let extra = if locked {
                    " — bound to the stick click by g13-analog.service"
                } else if inert {
                    " — inactive while the analog adapter runs"
                } else {
                    ""
                };
                let full = action.map(|a| format!("\n{a}")).unwrap_or_default();
                resp.clone()
                    .on_hover_text(format!("{}: {}{}{}", s.name, s.about, extra, full));
                if resp.clicked() && !locked && s.kind != Kind::Deco {
                    clicked = Some((s.name, resp.double_clicked()));
                }
            }
        }
        clicked
    }
}

fn clip(text: &str, max_chars: f32) -> String {
    let max = max_chars.max(3.0) as usize;
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}
