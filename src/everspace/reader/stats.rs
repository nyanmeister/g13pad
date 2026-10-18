// SPDX-License-Identifier: GPL-3.0-or-later
//! Predictions are conditional on current consumption/no further damage.
use super::*;
#[derive(Clone, Copy)]
pub(super) struct EnergySample {
    pawn: u64,
    time: f64,
    value: f64,
    maximum: f64,
    rate: f64,
}
fn seconds(value: f64) -> Option<u16> {
    (value.is_finite() && (0.0..=9999.0).contains(&value)).then(|| value.ceil() as u16)
}
impl Reader {
    // GetResourceAmount selects the persistent player's resources only for
    // configured persistent classes outside a hardcore run; otherwise run inventory.
    pub(super) fn repair_budget(
        &self,
        game: u64,
        health: u64,
        hull: f64,
        maximum: f64,
    ) -> io::Result<Option<u32>> {
        let class = self.ptr(health + 528)?;
        let run = self.ptr(game + 240)?;
        if class == 0 || run == 0 {
            return Ok(None);
        }
        let mut resources = run + 144;
        if self.read::<1>(run + 398)?[0] == 0 {
            let (classes, count) = self.array(game + 1056, 256)?;
            for i in 0..count {
                if self.ptr(classes + i * 8)? == class {
                    let player = self.ptr(game + 248)?;
                    if player == 0 {
                        return Ok(None);
                    }
                    resources = player + 368;
                    break;
                }
            }
        }
        let (entries, count) = self.array(resources, 512)?;
        let mut bots = 0;
        for i in 0..count {
            if self.ptr(entries + i * 16)? == class {
                bots = self.integer(entries + i * 16 + 8)?;
                break;
            }
        }
        if !(0..=1_000_000).contains(&bots) {
            return Err(invalid("invalid nanobot amount"));
        }
        let per_bot = self.float(health + 524)?;
        if !(0.0..=1_000_000_000.0).contains(&per_bot) {
            return Err(invalid("invalid nanobot repair capacity"));
        }
        // Conservative whole percent of hull recoverable now, capped at full hull.
        Ok(Some(
            ((bots as f64 * per_bot).min((maximum - hull).max(0.0)) / maximum * 100.0)
                .floor()
                .clamp(0.0, 100.0) as u32,
        ))
    }
    fn array(&self, at: u64, limit: i32) -> io::Result<(u64, u64)> {
        let data = self.ptr(at)?;
        let count = self.integer(at + 8)?;
        let capacity = self.integer(at + 12)?;
        if !(0..=limit).contains(&count)
            || count > capacity
            || capacity > limit
            || (count != 0 && data == 0)
        {
            return Err(invalid("invalid resource array"));
        }
        Ok((data, count as u64))
    }
    pub(super) fn shield_delay(&self, component: u64) -> io::Result<Option<u16>> {
        // Both timers decrease on the same game ticks, including during shutdown.
        // A depleted shield's longer shutdown is part of the wait before charging.
        let wait = self
            .float(component + 784)?
            .max(self.float(component + 788)?)
            .max(0.0);
        Ok(seconds(wait))
    }
    pub(super) fn energy_time(
        &self,
        world: u64,
        pawn: u64,
        component: u64,
        current: f64,
        maximum: f64,
    ) -> io::Result<Option<i16>> {
        let time = self.float(world + 2304)?;
        let old = self.energy_sample.get();
        let mut rate = 0.0;
        if let Some(old) = old.filter(|s| s.pawn == pawn && s.maximum == maximum) {
            let dt = time - old.time;
            if dt == 0.0 {
                rate = old.rate;
            } else if (0.05..=2.0).contains(&dt) {
                let instant = (current - old.value) / dt;
                // Reset immediately on a sign change; otherwise smooth two samples.
                rate = if instant.signum() == old.rate.signum() {
                    (instant + old.rate) / 2.0
                } else {
                    instant
                };
            }
        }
        self.energy_sample.set(Some(EnergySample {
            pawn,
            time,
            value: current,
            maximum,
            rate,
        }));
        if current >= maximum || self.read::<1>(component + 600)?[0] != 0 {
            return Ok(None);
        }
        if rate < -0.01 {
            return Ok(seconds(current.max(0.0) / -rate)
                .filter(|s| *s > 0)
                .map(|s| -(s as i16)));
        }
        let delay = self.float(component + 632)?.max(0.0);
        let output = if rate > 0.01 && delay == 0.0 {
            rate
        } else {
            if self.read::<1>(component + 536)?[0] != 0 {
                return Ok(None);
            }
            // Conditional estimate at current base output while waiting. Once
            // gaining energy, the observed net rate includes movement bonuses.
            self.float(component + 516)?
        };
        Ok((output > 0.0)
            .then(|| seconds(delay + (maximum - current).max(0.0) / output))
            .flatten()
            .filter(|s| *s > 0)
            .map(|s| s as i16))
    }
}
