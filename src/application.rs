// SPDX-License-Identifier: GPL-3.0-or-later
//! Profile transitions retain the desired profile and the previous live bindings separately.
//! LCD ownership and committing the active profile belong to the caller.
use crate::{daemon, modes, profile::Profile};

#[derive(Clone, Copy)]
pub enum Routing {
    Profile,
    /// Login applies the routes before the watcher has chosen its LED sum.
    Routes,
    Sum(u8),
}

pub struct Plan<'a> {
    pub target: &'a Profile,
    pub previous: Option<&'a Profile>,
    pub baseline: Option<&'a Profile>,
    pub routing: Routing,
}

impl Plan<'_> {
    /// Pure command construction; analog ownership is observed after the service transition.
    pub fn commands(&self, analog: bool) -> String {
        let mut commands = self
            .target
            .commands_from(self.previous, self.baseline, analog);
        if let Routing::Sum(bits) = self.routing {
            commands.push_str(&format!("mod {}\n", modes::leds_for(bits, self.target)));
        }
        if !matches!(self.routing, Routing::Profile) {
            commands.push_str(&modes::binds());
        }
        commands
    }

    /// Service/mapping rollback stays in apply_stick. FIFO sends are not transactions:
    /// only a successful return permits the caller to commit its active profile.
    pub fn execute(&self) -> Result<bool, String> {
        let analog = daemon::apply_stick(self.target.stick, self.target.gamepad)?;
        daemon::send(&self.commands(analog))?;
        Ok(analog)
    }

    /// Saved state may have changed before a CLI apply. The fixed device inventory
    /// supplies clears without pretending the newly saved profile is the old live state.
    pub fn complete_commands(&self, analog: bool) -> String {
        let mut commands = String::new();
        for &control in crate::profile::CONTROLS {
            if !self.target.binds.contains_key(control) && !(analog && control == "TOP") {
                commands.push_str(&format!("bind {control} KEY_RESERVED\n"));
            }
        }
        commands.push_str(&self.commands(analog));
        commands
    }

    pub fn execute_complete(&self) -> Result<bool, String> {
        let analog = daemon::apply_stick(self.target.stick, self.target.gamepad)?;
        daemon::send(&self.complete_commands(analog))?;
        Ok(analog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_edits_clear_prior_live_controls_and_preserve_analog_and_m_routes() {
        let target = Profile::default();
        let plan = Plan {
            target: &target,
            previous: None,
            baseline: None,
            routing: Routing::Routes,
        };
        let commands = plan.complete_commands(true);
        assert!(commands.contains("bind G1 KEY_RESERVED\n"));
        assert!(!commands.contains("bind TOP "));
        assert!(commands.ends_with(&modes::binds()));
        assert!(plan
            .complete_commands(false)
            .contains("bind TOP KEY_RESERVED\n"));
    }

    #[test]
    fn transition_preserves_unsaved_unbinds_baseline_and_adapter_ownership() {
        let baseline = Profile::parse("bind G1 KEY_A\nbind TOP KEY_C\n").0;
        let edited = Profile::parse("bind G2 KEY_B\nbind TOP KEY_D\nbind CUSTOM KEY_Q\n").0;
        let saved = Profile::parse("mod 2\nbind G3 KEY_E\nbind M1 KEY_F\n").0;
        let plan = Plan {
            target: &saved,
            previous: Some(&edited),
            baseline: Some(&baseline),
            routing: Routing::Sum(5),
        };
        let commands = plan.commands(true);
        assert!(commands.contains("bind G1 KEY_RESERVED\n"));
        assert!(commands.contains("bind G2 KEY_RESERVED\n"));
        assert!(commands.contains("bind CUSTOM KEY_RESERVED\n"));
        assert!(commands.contains("bind G3 KEY_E\n"));
        assert!(!commands.contains("bind TOP"));
        assert!(commands.ends_with(&format!("mod 5\n{}", modes::binds())));
        assert_eq!(saved.binds.len(), 2, "planning mutated the saved profile");
        assert!(plan.commands(false).contains("bind TOP KEY_RESERVED\n"));
    }

    #[test]
    fn startup_routes_preserve_profile_leds_until_watcher_selects_sum() {
        let profile = Profile::parse("mod 3\nbind M1 KEY_A\n").0;
        let mut plan = Plan {
            target: &profile,
            previous: None,
            baseline: None,
            routing: Routing::Routes,
        };
        let commands = plan.commands(false);
        assert_eq!(commands.matches("mod ").count(), 1);
        assert!(commands.starts_with("mod 3\n"));
        assert!(commands.ends_with(&modes::binds()));
        plan.routing = Routing::Sum(0);
        assert!(plan
            .commands(false)
            .ends_with(&format!("mod 3\n{}", modes::binds())));
    }

    #[test]
    fn cli_startup_executes_stick_preference_and_routes_without_changing_saved_profile() {
        let _sandbox = crate::test_support::Sandbox::new("apply-startup");
        let pipe = crate::test_support::PanelPipe::new();
        let profile = Profile::parse("# stick keys\nmod 3\nbind G1 KEY_A\nbind TOP KEY_B\n").0;
        crate::save("startup", &profile).unwrap();
        crate::set_active("startup").unwrap();
        let modes = modes::Modes {
            on: true,
            ..Default::default()
        };
        modes.save().unwrap();
        crate::apply().unwrap();
        let commands = String::from_utf8(pipe.read()).unwrap();
        assert!(!daemon::analog_active());
        assert!(commands.contains("bind TOP KEY_B\n"));
        assert!(commands.ends_with(&modes::binds()));
        assert_eq!(crate::load("startup").unwrap().0, profile);
        assert_eq!(crate::active_name(), "startup");
    }
}
