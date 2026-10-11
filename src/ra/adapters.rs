// SPDX-License-Identifier: GPL-3.0-or-later
//! Static game dispatch. Transports validate bytes; displays consume typed snapshots.
use super::{bn2, core_memory, fzero, process_start, Bitmap, Model, Session, Source};
use std::{path::Path, time::SystemTime};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    BattleNetwork2,
    FZeroGX,
}
struct Descriptor {
    source: Source,
    game: u32,
    kind: Kind,
    memory: Option<&'static core_memory::Recipe>,
    identify_content: Option<fn(&Path) -> bool>,
}
const REGISTRY: &[Descriptor] = &[
    Descriptor {
        source: Source::RetroArch,
        game: bn2::GAME,
        kind: Kind::BattleNetwork2,
        memory: Some(&bn2::RECIPE),
        identify_content: Some(bn2::identify_content),
    },
    Descriptor {
        source: Source::Dolphin,
        game: fzero::GAME,
        kind: Kind::FZeroGX,
        memory: None,
        identify_content: None,
    },
];

pub(super) fn memory_recipe(game: Option<u32>) -> Option<&'static core_memory::Recipe> {
    REGISTRY
        .iter()
        .find(|a| a.source == Source::RetroArch && Some(a.game) == game)?
        .memory
}

/// Offline startup hint only; every RAM collection still validates the ROM layout.
pub(super) fn content_game(path: &Path) -> Option<u32> {
    REGISTRY
        .iter()
        .find(|a| a.source == Source::RetroArch && a.identify_content.is_some_and(|f| f(path)))
        .map(|a| a.game)
}

/// An epoch changes on emulator/log replacement and game identity/reset transitions.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Binding {
    pid: u32,
    start: String,
    source: Source,
    game: u32,
    epoch: u64,
}
impl Binding {
    fn new(session: &Session, source: Source, game: u32, epoch: u64) -> Self {
        Self {
            pid: session.pid,
            start: session.start.clone(),
            source,
            game,
            epoch,
        }
    }
    pub(super) fn matches(
        &self,
        session: &Session,
        source: Source,
        game: Option<u32>,
        epoch: u64,
    ) -> bool {
        self.pid == session.pid
            && self.start == session.start
            && self.source == source
            && Some(self.game) == game
            && self.epoch == epoch
    }
}

pub(super) enum Snapshot {
    BattleNetwork2(bn2::Snapshot),
    FZeroGX(fzero::Snapshot),
}
#[derive(Clone, Copy)]
pub(super) enum View<'a> {
    BattleNetwork2(&'a bn2::Snapshot),
    FZeroGX(&'a fzero::Snapshot),
}
impl View<'_> {
    pub(super) fn matches(self, source: Source, game: Option<u32>) -> bool {
        match self {
            Self::BattleNetwork2(_) => source == Source::RetroArch && game == Some(bn2::GAME),
            Self::FZeroGX(_) => source == Source::Dolphin && game == Some(fzero::GAME),
        }
    }
    pub(super) fn frame(self, model: &Model) -> Bitmap {
        match self {
            Self::BattleNetwork2(value) => value.frame(model),
            Self::FZeroGX(value) => value.frame(model),
        }
    }
}
pub(super) struct BoundSnapshot {
    pub(super) binding: Binding,
    snapshot: Snapshot,
}
impl BoundSnapshot {
    pub(super) fn view(&self) -> View<'_> {
        match &self.snapshot {
            Snapshot::BattleNetwork2(s) => View::BattleNetwork2(s),
            Snapshot::FZeroGX(s) => View::FZeroGX(s),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Fallback {
    NoAdapter,
    NoSession,
    Unavailable,
}
pub(super) enum Outcome {
    Ready(BoundSnapshot),
    Fallback(Fallback),
}
#[derive(Default)]
pub(super) struct Reader {
    fzero: fzero::Reader,
    boundary: Option<(u64, SystemTime)>,
}
impl Reader {
    pub(super) fn poll(
        &mut self,
        session: Option<&Session>,
        source: Source,
        game: Option<u32>,
        epoch: u64,
    ) -> Outcome {
        let Some(descriptor) = REGISTRY
            .iter()
            .find(|a| a.source == source && Some(a.game) == game)
        else {
            return Outcome::Fallback(Fallback::NoAdapter);
        };
        let Some(session) = session else {
            return Outcome::Fallback(Fallback::NoSession);
        };
        if process_start(session.pid).as_deref() != Some(&session.start) {
            return Outcome::Fallback(Fallback::Unavailable);
        }
        if self.boundary.is_none_or(|(previous, _)| previous != epoch) {
            self.boundary = Some((epoch, SystemTime::now()));
        }
        let data = match descriptor.kind {
            Kind::BattleNetwork2 => bn2::poll_since(session, self.boundary.map(|(_, at)| at))
                .map(Snapshot::BattleNetwork2),
            Kind::FZeroGX => self.fzero.poll(session, game).map(Snapshot::FZeroGX),
        };
        match data {
            Some(snapshot) => Outcome::Ready(BoundSnapshot {
                binding: Binding::new(session, source, descriptor.game, epoch),
                snapshot,
            }),
            None => Outcome::Fallback(Fallback::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bindings_reject_same_game_reload_pid_reuse_and_other_sources() {
        let mut session = Session {
            pid: 123,
            start: "10".into(),
            path: "/private/log".into(),
            since: None,
        };
        let b = Binding::new(&session, Source::RetroArch, bn2::GAME, 2);
        assert!(b.matches(&session, Source::RetroArch, Some(bn2::GAME), 2));
        assert!(!b.matches(&session, Source::RetroArch, Some(bn2::GAME), 3));
        assert!(!b.matches(&session, Source::Dolphin, Some(bn2::GAME), 2));
        assert!(!b.matches(&session, Source::RetroArch, Some(1), 2));
        session.start = "11".into();
        assert!(!b.matches(&session, Source::RetroArch, Some(bn2::GAME), 2));
    }
    #[test]
    fn unknown_games_and_missing_sessions_are_distinct() {
        let mut reader = Reader::default();
        assert!(matches!(
            reader.poll(None, Source::RetroArch, Some(1), 0),
            Outcome::Fallback(Fallback::NoAdapter)
        ));
        assert!(matches!(
            reader.poll(None, Source::RetroArch, Some(bn2::GAME), 0),
            Outcome::Fallback(Fallback::NoSession)
        ));
    }
    #[test]
    fn same_id_reload_advances_memory_epoch_without_replaying_trophies() {
        let mut model = Model::default();
        model.ingest(
            super::super::Event::Identity(bn2::GAME, "BN2".into()),
            false,
        );
        let before = model.memory_epoch;
        let generation = model.generation;
        model.queue.push_back("PENDING TROPHY".into());
        model.ingest(
            super::super::Event::Identity(bn2::GAME, "BN2".into()),
            false,
        );
        assert!(model.memory_epoch > before);
        assert_eq!(model.generation, generation);
        assert_eq!(
            model.queue.front().map(String::as_str),
            Some("PENDING TROPHY")
        );
    }
}
