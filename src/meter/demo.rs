// SPDX-License-Identifier: GPL-3.0-or-later
//! Demo responsibilities for Health Mode.
use super::*;

// ---- feeders ----

/// A scripted pass through every state, for a look at the meter without a game.
pub fn demo() -> Result<String, String> {
    let active = crate::active_name();
    // Health mode is the profile's own line since 0.2.3x; an old picture name still counts.
    let shows = crate::load(&active)
        .ok()
        .is_some_and(|(p, _)| p.health().is_some());
    if !shows {
        println!(
            "note: profile '{active}' has no health mode (g13map profile health '{active}' \
             feed); the panel will not follow this run"
        );
    }
    let steps: &[(&str, Option<State>, u64)] = &[
        ("connected, waiting", Some(State::Wait), 5000),
        (
            "full health, half shield",
            Some(State::Health {
                pct: 100,
                shield: 50,
                helmet: false,
                extra: Extra::NONE,
            }),
            3000,
        ),
        (
            "90",
            Some(State::Health {
                pct: 90,
                shield: 20,
                helmet: false,
                extra: Extra::NONE,
            }),
            1500,
        ),
        ("76, shield gone", Some(State::health(76)), 2000),
        ("75: yellow", Some(State::health(75)), 2000),
        ("60", Some(State::health(60)), 1500),
        ("50: orange", Some(State::health(50)), 2000),
        ("35", Some(State::health(35)), 1500),
        ("25: red, the alarm", Some(State::health(25)), 4000),
        ("10", Some(State::health(10)), 4000),
        (
            "0: flatline, lights out; then the search under red",
            Some(State::health(0)),
            8000,
        ),
        ("back to 100", Some(State::health(100)), 2000),
        (
            "125: overshield",
            Some(State::Health {
                pct: 125,
                shield: 100,
                helmet: false,
                extra: Extra::NONE,
            }),
            4000,
        ),
        (
            "150, armour 200",
            Some(State::Health {
                pct: 150,
                shield: 200,
                helmet: false,
                extra: Extra::NONE,
            }),
            2000,
        ),
        ("game gone", None, 0),
    ];
    for (what, state, ms) in steps {
        println!("{what}");
        write(*state, None)?;
        thread::sleep(Duration::from_millis(*ms));
    }
    Ok("demo done; the profile's picture and colour are back".into())
}
