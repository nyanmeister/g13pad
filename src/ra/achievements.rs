// SPDX-License-Identifier: GPL-3.0-or-later
//! Achievements responsibilities for RetroAchievements.
use super::*;

#[derive(Debug, PartialEq)]
pub(super) enum Event {
    Identity(u32, String),
    Loaded(u32, bool),
    Mode(bool),
    Summary(u32, u32),
    Total(u32),
    Assets(u32, u32, u32, bool),
    Encore(bool),
    Unlock(u32, String),
    Awarded(u32),
    Unload,
}
pub(super) fn number(s: &str) -> Option<u32> {
    s.parse().ok()
}
pub(super) fn label(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect::<String>()
        .trim()
        .to_string()
}
pub(super) fn event(line: &str) -> Option<Event> {
    let s = line.split_once("[RCHEEVOS] ")?.1.trim();
    message(s)
}
pub(super) fn dolphin_message(line: &str) -> Option<&str> {
    line.split_once(" I[RetroAchievements]: ")
        .map(|(_, s)| s.trim())
}
pub(super) fn dolphin_event(line: &str) -> Option<Event> {
    let s = dolphin_message(line)?;
    if s == "Achievement Manager Initialized" || s == "Game closed." {
        return Some(Event::Unload);
    }
    match s {
        "Encore mode enabled" => return Some(Event::Encore(true)),
        "Encore mode disabled" => return Some(Event::Encore(false)),
        _ => {}
    }
    if let Some(s) = s.strip_prefix("Set ") {
        let (set, counts) = s.split_once(": ")?;
        let (counts, suffix) = counts.split_once(" achievements active")?;
        let (active, total) = counts.split_once('/')?;
        let (active, total) = (number(active)?, number(total)?);
        if active > total {
            return None;
        }
        let supported = if suffix.starts_with(", ") {
            true
        } else {
            let count = suffix.strip_prefix(" (")?.split_once(" unsupported), ")?.0;
            number(count)? == 0
        };
        return Some(Event::Assets(number(set)?, active, total, supported));
    }
    message(s)
}
pub(super) fn message(s: &str) -> Option<Event> {
    if let Some(s) = s.strip_prefix("Identified game: ") {
        let (id, tail) = s.split_once(' ')?;
        let title = tail.strip_prefix('"')?.rsplit_once('"')?.0;
        return Some(Event::Identity(number(id)?, label(title)));
    }
    if let Some(s) = s.strip_prefix("Game ") {
        let (id, mode) = s.split_once(" loaded, hardcore ")?;
        let mode = match mode.split(',').next()? {
            "enabled" => true,
            "disabled" => false,
            _ => return None,
        };
        return Some(Event::Loaded(number(id)?, mode));
    }
    match s {
        "Hardcore enabled" | "Hardcore enabled, waiting for reset" => {
            return Some(Event::Mode(true));
        }
        "Hardcore disabled" => return Some(Event::Mode(false)),
        _ => {}
    }
    if let Some(s) = s.strip_prefix("You have ") {
        let (earned, total) = s.split_once(" of ")?;
        let total = number(total.split_once(" achievements unlocked")?.0)?;
        let earned = number(earned)?;
        return (earned <= total).then_some(Event::Summary(earned, total));
    }
    if let Some(s) = s.strip_prefix("All ") {
        return Some(Event::Total(number(
            s.split_once(" achievements activated for this session")?.0,
        )?));
    }
    if let Some(s) = s.strip_prefix("Awarding achievement ") {
        let (id, title) = s.split_once(": ")?;
        return Some(Event::Unlock(number(id)?, label(title)));
    }
    if let Some(s) = s.strip_prefix("Achievement ") {
        let (id, rest) = s.split_once(' ')?;
        if rest.starts_with("awarded, new score: ") || rest.starts_with("awarded after ") {
            return Some(Event::Awarded(number(id)?));
        }
    }
    s.strip_prefix("Unloading game ")
        .and_then(number)
        .map(|_| Event::Unload)
}

#[derive(Default)]
pub(super) struct Model {
    pub(super) generation: u64,
    /// Memory epoch advances even when a reload announces the same RA game ID.
    pub(super) memory_epoch: u64,
    pub(super) game: Option<u32>,
    pub(super) title: String,
    pub(super) hardcore: Option<bool>,
    pub(super) earned: Option<u32>,
    pub(super) total: Option<u32>,
    pub(super) gained: u32,
    pub(super) confirmed: HashSet<u32>,
    pub(super) requested: HashSet<u32>,
    pub(super) queue: VecDeque<String>,
    pub(super) encore: Option<bool>,
    pub(super) sets: HashMap<u32, (Option<u32>, u32)>,
    pub(super) sets_overflow: bool,
}
impl Model {
    pub(super) fn reset(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            memory_epoch: self.memory_epoch.wrapping_add(1),
            ..Self::default()
        };
    }
    pub(super) fn ingest(&mut self, e: Event, announce: bool) {
        if matches!(e, Event::Identity(..) | Event::Loaded(..)) {
            self.memory_epoch = self.memory_epoch.wrapping_add(1);
        }
        match e {
            Event::Identity(id, title) => {
                if self.game != Some(id) {
                    let encore = self.encore;
                    self.reset();
                    self.encore = encore;
                }
                self.game = Some(id);
                self.title = title;
            }
            Event::Loaded(id, mode) => {
                if self.game != Some(id) {
                    self.reset();
                    self.game = Some(id);
                }
                self.hardcore = Some(mode);
            }
            Event::Mode(mode) if self.game.is_some() => {
                if self.hardcore.is_some_and(|old| old != mode) {
                    // The previous mode's summary cannot establish this mode's count.
                    self.earned = None;
                    self.sets.clear();
                    self.requested.clear();
                    self.confirmed.clear();
                }
                self.hardcore = Some(mode);
            }
            Event::Summary(earned, total) if self.game.is_some() => {
                self.earned = Some(earned);
                self.total = Some(total);
            }
            Event::Total(total) if self.game.is_some() => {
                self.total = Some(total);
                self.earned = None;
            }
            Event::Encore(enabled) => {
                self.encore = Some(enabled);
                if enabled {
                    self.earned = None;
                }
            }
            Event::Assets(id, active, total, supported)
                if self.game.is_some() && self.hardcore.is_some() =>
            {
                // Set IDs are not game IDs. Sum all loaded core sets, and do not
                // count unsupported or Encore-active achievements as earned.
                if self.sets_overflow || (self.sets.len() >= 256 && !self.sets.contains_key(&id)) {
                    self.sets_overflow = true;
                    self.earned = None;
                    self.total = None;
                    return;
                }
                let earned = (supported && self.encore == Some(false)).then_some(total - active);
                self.sets.insert(id, (earned, total));
                self.total = self
                    .sets
                    .values()
                    .try_fold(0u32, |sum, (_, t)| sum.checked_add(*t));
                self.earned = self
                    .sets
                    .values()
                    .try_fold(0u32, |sum, (n, _)| sum.checked_add((*n)?));
            }
            Event::Unlock(id, title) if self.game.is_some() => {
                self.requested.insert(id);
                if !announce {
                    return;
                }
                if self.queue.len() < MAX_QUEUE {
                    self.queue.push_back(title);
                } else {
                    eprintln!("RetroAchievements notification queue full");
                }
            }
            Event::Awarded(id)
                if self.game.is_some()
                    && self.requested.contains(&id)
                    && self.confirmed.insert(id) =>
            {
                if let Some(n) = self.earned.as_mut() {
                    *n = n.saturating_add(1).min(self.total.unwrap_or(u32::MAX));
                }
                // Reconstruct session progress on reactivation; only cards need
                // live-event gating to avoid replaying old notifications.
                self.gained = self.gained.saturating_add(1);
            }
            Event::Unload => self.reset(),
            _ => {}
        }
    }
}
