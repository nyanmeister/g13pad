// SPDX-License-Identifier: GPL-3.0-or-later
//! Display responsibilities for RetroAchievements.
use super::*;

pub(super) fn lcd_label(s: &str) -> String {
    s.to_uppercase()
        .chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}
pub(super) fn rows(s: &str) -> (String, String) {
    let s = lcd_label(s);
    if s.len() <= 26 {
        return (s, String::new());
    }
    let target = if s.len() <= 52 { 26 } else { s.len() / 2 };
    let split = s[..target]
        .rfind(' ')
        .filter(|n| *n > target / 2)
        .unwrap_or(target);
    (s[..split].trim().into(), s[split..].trim().into())
}
pub(super) fn scroll_offset(width: usize, elapsed_ms: u128, budget_ms: u128) -> i32 {
    let end = width.saturating_sub(160) as u128;
    (end * elapsed_ms.saturating_sub(1000).min(budget_ms) / budget_ms.max(1)) as i32
}
pub(super) fn bar(bm: &mut Bitmap, model: &Model, y: i32) {
    progress_bar(bm, model, y, 5);
}
pub(super) fn progress_bar(bm: &mut Bitmap, model: &Model, y: i32, height: i32) {
    bm.line(0, y, 159, y);
    bm.line(0, y + height, 159, y + height);
    bm.line(0, y, 0, y + height);
    bm.line(159, y, 159, y + height);
    if let (Some(n), Some(total)) = (model.earned, model.total) {
        if total != 0 {
            for x in 1..=((u64::from(n) * 158 / u64::from(total)) as i32).min(158) {
                bm.line(x, y + 1, x, y + height - 1);
            }
        }
    }
}
pub(super) struct Card {
    pub(super) title: String,
    pub(super) started: Instant,
    pub(super) duration: Duration,
}
#[derive(Default)]
pub(super) struct Display {
    pub(super) generation: u64,
    pub(super) card: Option<Card>,
    pub(super) game: Option<u32>,
    pub(super) started: Option<Instant>,
}
impl Display {
    #[cfg(test)]
    pub(super) fn frame(&mut self, model: &mut Model, now: Instant) -> Bitmap {
        self.frame_for(model, now, Source::RetroArch)
    }
    pub(super) fn frame_for(&mut self, model: &mut Model, now: Instant, source: Source) -> Bitmap {
        self.frame_adapter(model, now, source, None)
    }
    #[cfg(test)]
    pub(super) fn frame_with(
        &mut self,
        model: &mut Model,
        now: Instant,
        source: Source,
        specific: Option<&fzero::Snapshot>,
    ) -> Bitmap {
        self.frame_telemetry(model, now, source, specific, None)
    }
    #[cfg(test)]
    pub(super) fn frame_telemetry(
        &mut self,
        model: &mut Model,
        now: Instant,
        source: Source,
        specific: Option<&fzero::Snapshot>,
        battle_network: Option<&bn2::Snapshot>,
    ) -> Bitmap {
        self.frame_adapter(
            model,
            now,
            source,
            specific
                .map(adapters::View::FZeroGX)
                .or_else(|| battle_network.map(adapters::View::BattleNetwork2)),
        )
    }
    pub(super) fn frame_interval(&self) -> Duration {
        if self.card.as_ref().is_some_and(|c| {
            let (a, b) = rows(&c.title);
            a.len().max(b.len()) > 26
        }) {
            crate::display_clock::MIN_FRAME_INTERVAL
        } else {
            crate::display_clock::TELEMETRY_INTERVAL
        }
    }
    pub(super) fn frame_adapter(
        &mut self,
        model: &mut Model,
        now: Instant,
        source: Source,
        specific: Option<adapters::View<'_>>,
    ) -> Bitmap {
        if self.game != model.game || self.generation != model.generation {
            self.card = None;
            self.game = model.game;
            self.generation = model.generation;
            self.started = Some(now);
        }
        if self
            .card
            .as_ref()
            .is_some_and(|c| now.saturating_duration_since(c.started) >= c.duration)
        {
            self.card = None;
        }
        if self.card.is_none() {
            if let Some(title) = model.queue.pop_front() {
                let (a, b) = rows(&title);
                let seconds = if a.len().max(b.len()) > 26 { 10 } else { 8 };
                self.card = Some(Card {
                    title,
                    started: now,
                    duration: Duration::from_secs(seconds),
                });
            }
        }
        let mut bm = Bitmap::blank();
        if let Some(card) = &self.card {
            bm.sprite(
                2,
                1,
                &[
                    "  ##########  ",
                    "### ###### ###",
                    "# # ###### # #",
                    "# # ###### # #",
                    " ## ###### ## ",
                    "    ######    ",
                    "     ####     ",
                    "      ##      ",
                    "      ##      ",
                    "    ######    ",
                    "   ########   ",
                ],
            );
            text(&mut bm, 25, 2, "UNLOCKED", 1);
            let (a, b) = rows(&card.title);
            let width = a.len().max(b.len()) * 6;
            let offset = scroll_offset(
                width,
                now.saturating_duration_since(card.started).as_millis(),
                8000,
            );
            text(&mut bm, -offset, 15, &a, 1);
            text(&mut bm, -offset, 24, &b, 1);
            bar(&mut bm, model, 36);
            return bm;
        }
        if let Some(snapshot) = specific.filter(|s| s.matches(source, model.game)) {
            return snapshot.frame(model);
        }
        if model.game.is_none() {
            text(&mut bm, 5, 9, "RETROACHIEVEMENTS", 1);
            text(
                &mut bm,
                5,
                25,
                &format!("WAITING FOR {}", source.name().to_uppercase()),
                1,
            );
            return bm;
        }
        let title = if model.title.is_empty() {
            format!("RA GAME {}", model.game.unwrap())
        } else {
            lcd_label(&model.title)
        };
        let width = title.len() * 6;
        let span = width.saturating_sub(160) as u128;
        let duration = 2000 + span * 50;
        let elapsed = now
            .saturating_duration_since(*self.started.get_or_insert(now))
            .as_millis()
            % duration;
        let offset = scroll_offset(width, elapsed, span * 50);
        text(&mut bm, -offset, 1, &title, 1);
        let count = match (model.earned, model.total) {
            (Some(n), Some(t)) if t > 0 => {
                format!("{n}/{t} {}%", u64::from(n) * 100 / u64::from(t))
            }
            (_, Some(t)) => format!("--/{t}"),
            _ => "--/--".into(),
        };
        text(
            &mut bm,
            0,
            12,
            &count,
            if count.len() <= 13 { 2 } else { 1 },
        );
        bar(&mut bm, model, 28);
        text(
            &mut bm,
            0,
            36,
            match model.hardcore {
                Some(true) => "HARDCORE",
                Some(false) => "SOFTCORE",
                None => "MODE ?",
            },
            1,
        );
        text(&mut bm, 72, 36, &format!("SESSION +{}", model.gained), 1);
        bm
    }
}

pub(crate) fn preview(source: Source) -> Bitmap {
    let (game, title, earned, total, hardcore) = match source {
        Source::RetroArch => (773, "Mega Man Battle Network 2", 4, 80, false),
        Source::Dolphin => (15543, "Skies of Arcadia Legends", 38, 180, true),
    };
    Display::default().frame_for(
        &mut Model {
            game: Some(game),
            title: title.into(),
            earned: Some(earned),
            total: Some(total),
            hardcore: Some(hardcore),
            ..Model::default()
        },
        Instant::now(),
        source,
    )
}
