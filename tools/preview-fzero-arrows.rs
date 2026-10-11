// SPDX-License-Identifier: GPL-3.0-or-later
// Standalone, offline design sampler. Does not touch the watcher or game.
// rustc tools/preview-fzero-arrows.rs -o /tmp/g13-arrow-samples
// g13-arrow-samples <render_samples directory> <output directory>
use std::{env, fmt::Write as _, fs, path::Path};

const W: usize = 160;
const H: usize = 43;

fn put(bm: &mut [u8], x: i32, y: i32, on: bool) {
    if !(0..W as i32).contains(&x) || !(0..H as i32).contains(&y) {
        return;
    }
    let byte = &mut bm[x as usize + y as usize / 8 * W];
    let mask = 1 << (y % 8);
    if on {
        *byte |= mask;
    } else {
        *byte &= !mask;
    }
}

fn lit(bm: &[u8], x: usize, y: usize) -> bool {
    bm[x + y / 8 * W] & (1 << (y % 8)) != 0
}

fn head(style: usize) -> Vec<Vec<u8>> {
    if style == 0 {
        return [
            "           ###           ",
            "          #####          ",
            "         #######         ",
            "        ####o####        ",
            "       ####ooo####       ",
            "      ####     ####      ",
            "     ####       ####     ",
            "    ##o#         #o##    ",
            "   ####           ####   ",
            "  ####             ####  ",
            " #####             ##### ",
            "#####               #####",
            " ###                 ### ",
        ]
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect();
    }
    let mut rows = vec![vec![b' '; 25]; 13];
    let swept = [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 12, 12, 11];
    for (y, row) in rows.iter_mut().enumerate() {
        let outer = if style == 3 || style == 5 {
            swept[y]
        } else {
            (y + 1).min(12)
        };
        let thickness = if style == 2 { 5 } else { 4 };
        let inner = outer.saturating_sub(thickness);
        for (x, pixel) in row.iter_mut().enumerate() {
            let d = x.abs_diff(12);
            if d <= outer && (d > inner || y < 3) {
                *pixel = b'#';
                if style == 2 && y >= 3 && d == outer - 2 {
                    *pixel = b'o'; // Two parallel rails, one-pixel channel.
                }
                if style == 4 && (x == 12 || (y == 7 && d == outer - 1)) {
                    *pixel = b'o'; // Split nose and shoulder etching.
                }
                if style == 5 && y == 7 {
                    *pixel = b'o'; // Separate outer wing panels from the nose.
                }
            }
        }
        if y == 12 {
            // Beveled bottom caps keep the wings inside the disc.
            for (x, pixel) in row.iter_mut().enumerate() {
                if x.abs_diff(12) >= outer {
                    *pixel = b' ';
                }
            }
        }
    }
    rows
}

fn overlay(bm: &mut [u8], rows: &[Vec<u8>]) {
    for top in [9, 20] {
        for (y, row) in rows.iter().enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                if *pixel == b'#' {
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            put(bm, 9 + x as i32 + dx, top + y as i32 + dy, false);
                        }
                    }
                }
            }
        }
    }
    for top in [9, 20] {
        for (y, row) in rows.iter().enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                if *pixel == b'#' || *pixel == b'o' {
                    put(bm, 9 + x as i32, top + y as i32, *pixel == b'#');
                }
            }
        }
    }
}

fn svg_pixels(svg: &mut String, bm: &[u8], ox: usize, oy: usize, scale: usize, width: usize) {
    writeln!(svg, "<g fill='#ffffff' shape-rendering='crispEdges'>").unwrap();
    for y in 0..H {
        for x in 0..width {
            if lit(bm, x, y) {
                writeln!(
                    svg,
                    "<rect x='{}' y='{}' width='{scale}' height='{scale}'/>",
                    ox + x * scale,
                    oy + y * scale
                )
                .unwrap();
            }
        }
    }
    svg.push_str("</g>\n");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = env::args().collect();
    if args.get(1).is_some_and(|a| a == "--version") {
        println!("g13-arrow-samples 1.0.0");
        return Ok(());
    }
    if args.len() != 3 {
        return Err("usage: g13-arrow-samples INPUT_DIR OUTPUT_DIR".into());
    }
    let input = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let full = fs::read(input.join("full.lpbm"))?;
    let boost = fs::read(input.join("boost.lpbm"))?;
    if full.len() != 960 || boost.len() != 960 {
        return Err("expected 960-byte LCD exports".into());
    }
    fs::create_dir_all(output)?;
    let titles = [
        "A / Current etched",
        "B / Clean bold",
        "C / Hollow rails",
        "D / Swept wings",
        "E / Split tips",
        "F / Segmented wings",
    ];
    let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1200' height='1072' viewBox='0 0 1200 1072'>\n<rect width='1200' height='1072' fill='#11151b'/>\n<g fill='#e4ecf5' font-family='DejaVu Sans,sans-serif'><text x='32' y='44' font-size='28'>F-Zero GX / boost arrow options</text><text x='32' y='76' font-size='16'>Same 43px circle, same spacing. Enlarged pixels + full LCD at 2x + native 160x43.</text></g>\n");
    for (style, title) in titles.iter().enumerate() {
        let mut bm = boost.clone();
        for y in 0..H {
            for x in 0..43 {
                put(&mut bm, x as i32, y as i32, lit(&full, x, y));
            }
        }
        let rows = head(style);
        overlay(&mut bm, &rows);
        if style == 0 && bm != boost {
            return Err("current option differs from deployed export".into());
        }
        let letter = (b'A' + style as u8) as char;
        fs::write(output.join(format!("{letter}.lpbm")), &bm)?;
        let mut pbm = b"P4\n160 43\n".to_vec();
        for y in 0..H {
            for byte in 0..W / 8 {
                let mut value = 0;
                for bit in 0..8 {
                    if !lit(&bm, byte * 8 + bit, y) {
                        value |= 1 << (7 - bit);
                    }
                }
                pbm.push(value);
            }
        }
        fs::write(output.join(format!("{letter}.pbm")), pbm)?;
        let mut sprite = String::new();
        for row in rows {
            sprite.push_str(std::str::from_utf8(&row)?);
            sprite.push('\n');
        }
        fs::write(output.join(format!("{letter}.sprite.txt")), sprite)?;
        let x = (style % 3) * 400;
        let y = 108 + (style / 3) * 476;
        writeln!(svg, "<rect x='{}' y='{y}' width='376' height='460' rx='8' fill='#1b222c' stroke='#354253'/><text x='{}' y='{}' fill='#e4ecf5' font-family='DejaVu Sans,sans-serif' font-size='19'>{title}</text>", x+12, x+30, y+31).unwrap();
        writeln!(svg, "<rect x='{}' y='{}' width='258' height='258' fill='black'/><rect x='{}' y='{}' width='320' height='86' fill='black'/><rect x='{}' y='{}' width='160' height='43' fill='black'/>", x+71,y+46,x+40,y+312,x+120,y+406).unwrap();
        svg_pixels(&mut svg, &bm, x + 71, y + 46, 6, 43);
        svg_pixels(&mut svg, &bm, x + 40, y + 312, 2, 160);
        // Native image sits at the bottom of each card.
        svg_pixels(&mut svg, &bm, x + 120, y + 406, 1, 160);
    }
    svg.push_str("</svg>\n");
    fs::write(output.join("selection.svg"), svg)?;
    println!("Six offline samples saved to {}", output.display());
    Ok(())
}
