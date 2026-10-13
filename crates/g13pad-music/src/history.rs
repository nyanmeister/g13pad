// SPDX-License-Identifier: GPL-3.0-or-later
use crate::client::{Client, Song, Status};
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, time::Duration};
type Result<T> = std::result::Result<T, String>;
pub struct Store {
    pub db: Connection,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let db = Connection::open(path).map_err(|e| e.to_string())?;
        Self::initialize(db)
    }
    fn initialize(db: Connection) -> Result<Self> {
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if version > 1 {
            return Err("history database was written by a newer version".into());
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS tracks (
                id INTEGER PRIMARY KEY, uri TEXT NOT NULL UNIQUE, title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL,
                playcount INTEGER NOT NULL DEFAULT 0, lastplayed INTEGER NOT NULL DEFAULT 0, listened_ms INTEGER NOT NULL DEFAULT 0,
                favourite INTEGER NOT NULL DEFAULT 0, bookmark REAL, rating INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS sessions (
                id INTEGER PRIMARY KEY, track_id INTEGER NOT NULL REFERENCES tracks(id), queue_id INTEGER NOT NULL, daemon_epoch INTEGER NOT NULL,
                started INTEGER NOT NULL, observed INTEGER NOT NULL, elapsed REAL NOT NULL, heard_ms INTEGER NOT NULL DEFAULT 0,
                counted INTEGER NOT NULL DEFAULT 0, ended INTEGER, end_reason TEXT);
            CREATE INDEX IF NOT EXISTS sessions_track ON sessions(track_id,started);
            CREATE UNIQUE INDEX IF NOT EXISTS one_open_session ON sessions((1)) WHERE ended IS NULL;
            CREATE TABLE IF NOT EXISTS outbox (uri TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL, attempted INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(uri,name));
            PRAGMA user_version=1;").map_err(|e|e.to_string())?;
        Ok(Self { db })
    }
    pub fn ensure_track(&self, s: &Song, baseline: Option<(u64, u64, u64, u32)>) -> Result<()> {
        let b = baseline.unwrap_or_default();
        self.db.execute("INSERT INTO tracks(uri,title,artist,album,playcount,lastplayed,listened_ms,rating) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
            ON CONFLICT(uri) DO UPDATE SET title=excluded.title,artist=excluded.artist,album=excluded.album",params![s.uri,s.title,s.artist,s.album,b.0.min(i64::MAX as u64) as i64,b.1.min(i64::MAX as u64) as i64,b.2.min(i64::MAX as u64) as i64,b.3.min(10)]).map_err(|e|e.to_string())?;
        Ok(())
    }
    pub fn has_track(&self, uri: &str) -> Result<bool> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tracks WHERE uri=?1)",
                [uri],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
    }
    pub fn summary(&self) -> Result<String> {
        self.db.query_row("SELECT count(*),coalesce(sum(playcount),0),coalesce(sum(listened_ms),0), (SELECT count(*) FROM sessions), (SELECT count(*) FROM outbox) FROM tracks",[],|r|Ok(format!("{} observed tracks; {} plays; {:.1} listening hours; {} sessions; {} pending stickers",r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)? as f64/3_600_000.,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?))).map_err(|e|e.to_string())
    }
    pub fn project(&self, client: &mut Client) -> Result<()> {
        // Absolute totals make retries safe even if MPD accepted a write before a crash.
        let rows = self
            .db
            .prepare("SELECT uri,name,value FROM outbox ORDER BY attempted,uri,name LIMIT 12")
            .map_err(|e| e.to_string())?
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| e.to_string())?;
        let mut failure = None;
        for (uri, name, value) in rows {
            if let Err(e) = client.set_sticker(&uri, &name, &value) {
                if !client.recoverable() {
                    return Err(e);
                }
                self.db
                    .execute(
                        "UPDATE outbox SET attempted=unixepoch() WHERE uri=?1 AND name=?2",
                        params![uri, name],
                    )
                    .map_err(|e| e.to_string())?;
                failure.get_or_insert(e);
                continue;
            }
            self.db
                .execute(
                    "DELETE FROM outbox WHERE uri=?1 AND name=?2 AND value=?3",
                    params![uri, name, value],
                )
                .map_err(|e| e.to_string())?;
        }
        failure.map_or(Ok(()), Err)
    }
}

#[derive(Clone)]
struct Session {
    id: i64,
    uri: String,
    queue_id: u32,
    epoch: i64,
    heard: f64,
    saved: f64,
    counted: bool,
    elapsed: f64,
    duration: f64,
}
#[derive(Clone)]
struct Observation {
    status: Status,
    uri: String,
    at: f64,
    duration: f64,
}
pub struct Tracker {
    session: Option<Session>,
    previous: Option<Observation>,
    last_flush: f64,
    pub fraction: f64,
    pub seconds: f64,
}
impl Tracker {
    pub fn new(store: &Store, fraction: f64, seconds: f64) -> Result<Self> {
        let session=store.db.query_row("SELECT s.id,t.uri,s.queue_id,s.daemon_epoch,s.heard_ms,s.counted,s.elapsed FROM sessions s JOIN tracks t ON t.id=s.track_id WHERE ended IS NULL",[],|r|{let heard=r.get::<_,i64>(4)? as f64/1000.;Ok(Session{id:r.get(0)?,uri:r.get(1)?,queue_id:r.get(2)?,epoch:r.get(3)?,heard,saved:heard,counted:r.get::<_,bool>(5)?,elapsed:r.get(6)?,duration:0.})}).optional().map_err(|e|e.to_string())?;
        Ok(Self {
            session,
            previous: None,
            last_flush: 0.,
            fraction,
            seconds,
        })
    }
    fn close(&mut self, store: &Store, wall: i64, reason: &str) -> Result<()> {
        if let Some(s) = self.session.take() {
            store
                .db
                .execute(
                    "UPDATE sessions SET ended=?1,end_reason=?2 WHERE id=?3",
                    params![wall, reason, s.id],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn observe(
        &mut self,
        store: &Store,
        status: Status,
        song: Option<&Song>,
        at: f64,
        wall: i64,
        epoch: i64,
    ) -> Result<()> {
        let song = song.filter(|s| status.id == Some(s.id));
        let new_loop = match (&self.previous, song) {
            (Some(p), Some(s)) => {
                p.status.playing
                    && status.playing
                    && p.uri == s.uri
                    && p.status.id == status.id
                    && p.duration > 0.
                    && p.status.elapsed > p.duration - 2.5
                    && status.elapsed < 2.5
                    && status.elapsed + 0.2 < p.status.elapsed
            }
            _ => false,
        };
        let same = self.session.as_ref().is_some_and(|s| {
            song.is_some_and(|t| t.uri == s.uri && t.id == s.queue_id)
                && (s.epoch - epoch).abs() <= 2
                && (self.previous.is_some() || status.elapsed + 2.5 >= s.elapsed)
                && !new_loop
                && !status.stopped
        });
        if !same && self.session.is_some() {
            self.flush(store, at, wall)?;
            self.close(
                store,
                wall,
                if new_loop {
                    "repeat"
                } else if status.stopped {
                    "stop"
                } else {
                    "track-change-or-restart"
                },
            )?;
        }
        if self.session.is_none() && status.playing {
            if let Some(s) = song {
                store.db.execute("INSERT INTO sessions(track_id,queue_id,daemon_epoch,started,observed,elapsed) SELECT id,?2,?3,?4,?4,?5 FROM tracks WHERE uri=?1",params![s.uri,s.id,epoch,wall,status.elapsed]).map_err(|e|e.to_string())?;
                self.session = Some(Session {
                    id: store.db.last_insert_rowid(),
                    uri: s.uri.clone(),
                    queue_id: s.id,
                    epoch,
                    heard: 0.,
                    saved: 0.,
                    counted: false,
                    elapsed: status.elapsed,
                    duration: s.duration,
                });
                self.last_flush = at;
            }
        }
        if let (Some(session), Some(prev), Some(song)) = (&mut self.session, &self.previous, song) {
            let dt = at - prev.at;
            let progress = status.elapsed - prev.status.elapsed;
            // A seek is a discontinuity, not listening. Paused and unobserved gaps add nothing.
            if !new_loop
                && prev.status.playing
                && prev.uri == song.uri
                && prev.status.id == status.id
                && dt > 0.
                && dt <= 2.5
                && progress >= 0.
                && (progress - dt).abs() <= 0.35
            {
                session.heard += dt.min(progress);
            }
        }
        if let (Some(session), Some(song)) = (&mut self.session, song) {
            session.elapsed = status.elapsed;
            session.duration = song.duration;
        }
        let threshold = song.map_or(self.seconds, |s| {
            if s.duration > 0. {
                (s.duration * self.fraction).min(self.seconds)
            } else {
                self.seconds
            }
        });
        let newly_counted = self
            .session
            .as_ref()
            .is_some_and(|s| !s.counted && s.heard >= threshold);
        if newly_counted || at - self.last_flush >= 10. {
            self.flush(store, at, wall)?;
        }
        self.previous = song.map(|s| Observation {
            status,
            uri: s.uri.clone(),
            at,
            duration: s.duration,
        });
        Ok(())
    }
    pub fn gap(&mut self, store: &Store, at: f64, wall: i64) -> Result<()> {
        self.flush(store, at, wall)?;
        self.previous = None;
        Ok(())
    }
    pub fn flush(&mut self, store: &Store, at: f64, wall: i64) -> Result<()> {
        if let Some(s) = &mut self.session {
            let threshold = if s.duration > 0. {
                (s.duration * self.fraction).min(self.seconds)
            } else {
                self.seconds
            };
            let count = !s.counted && s.heard >= threshold;
            let delta = ((s.heard - s.saved) * 1000.).round().max(0.) as i64;
            let tx = store
                .db
                .unchecked_transaction()
                .map_err(|e| e.to_string())?;
            tx.execute("UPDATE tracks SET listened_ms=listened_ms+?1,playcount=playcount+?2,lastplayed=CASE WHEN ?2=1 THEN ?3 ELSE lastplayed END WHERE uri=?4",params![delta,i64::from(count),wall,s.uri]).map_err(|e|e.to_string())?;
            tx.execute(
                "UPDATE sessions SET heard_ms=?1,counted=?2,observed=?3,elapsed=?4 WHERE id=?5",
                params![
                    (s.heard * 1000.).round() as i64,
                    s.counted || count,
                    wall,
                    self.previous.as_ref().map_or(0., |p| p.status.elapsed),
                    s.id
                ],
            )
            .map_err(|e| e.to_string())?;
            if delta > 0 || count {
                tx.execute("INSERT INTO outbox(uri,name,value) SELECT uri,'listeningtime',CAST(listened_ms/1000 AS TEXT) FROM tracks WHERE uri=?1 ON CONFLICT(uri,name) DO UPDATE SET value=excluded.value",[&s.uri]).map_err(|e|e.to_string())?;
                if count {
                    for key in ["playcount", "lastplayed"] {
                        tx.execute(&format!("INSERT INTO outbox(uri,name,value) SELECT uri,'{key}',CAST({key} AS TEXT) FROM tracks WHERE uri=?1 ON CONFLICT(uri,name) DO UPDATE SET value=excluded.value"),[&s.uri]).map_err(|e|e.to_string())?;
                    }
                }
            }
            tx.commit().map_err(|e| e.to_string())?;
            s.saved = s.heard;
            s.counted |= count;
        }
        self.last_flush = at;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Store, Song, Status, Tracker) {
        let store = Store::initialize(Connection::open_in_memory().unwrap()).unwrap();
        let song = Song {
            uri: "test.opus".into(),
            id: 7,
            duration: 10.,
            ..Default::default()
        };
        store.ensure_track(&song, None).unwrap();
        let t = Tracker::new(&store, 0.5, 240.).unwrap();
        (
            store,
            song,
            Status {
                playing: true,
                id: Some(7),
                ..Default::default()
            },
            t,
        )
    }
    fn totals(store: &Store) -> (i64, i64) {
        store
            .db
            .query_row("SELECT playcount,listened_ms FROM tracks", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap()
    }
    #[test]
    fn pause_seek_disconnect_and_count_once() {
        let (store, song, mut status, mut t) = fixture();
        for i in 0..=3 {
            status.elapsed = i as f64;
            t.observe(&store, status.clone(), Some(&song), i as f64, 100 + i, 90)
                .unwrap();
        }
        status.playing = false;
        t.observe(&store, status.clone(), Some(&song), 4., 104, 90)
            .unwrap();
        t.observe(&store, status.clone(), Some(&song), 6., 106, 90)
            .unwrap();
        status.playing = true;
        status.elapsed = 8.;
        t.observe(&store, status.clone(), Some(&song), 7., 107, 90)
            .unwrap();
        status.elapsed = 9.;
        t.observe(&store, status.clone(), Some(&song), 8., 108, 90)
            .unwrap();
        t.gap(&store, 9., 109).unwrap();
        status.elapsed = 9.5;
        t.observe(&store, status.clone(), Some(&song), 20., 120, 90)
            .unwrap();
        t.flush(&store, 20., 120).unwrap();
        assert_eq!(totals(&store), (0, 4000));
        status.elapsed = 10.5;
        t.observe(&store, status.clone(), Some(&song), 21., 121, 90)
            .unwrap();
        assert_eq!(totals(&store), (1, 5000));
        t.flush(&store, 22., 122).unwrap();
        assert_eq!(totals(&store), (1, 5000));
        let mut restarted = Tracker::new(&store, 0.5, 240.).unwrap();
        restarted
            .observe(&store, status.clone(), Some(&song), 0., 123, 90)
            .unwrap();
        status.elapsed = 11.5;
        restarted
            .observe(&store, status, Some(&song), 1., 124, 90)
            .unwrap();
        restarted.flush(&store, 2., 125).unwrap();
        assert_eq!(totals(&store), (1, 6000));
    }
    #[test]
    fn repeated_queue_item_gets_another_session() {
        let (store, song, mut s, mut t) = fixture();
        for i in 0..=9 {
            s.elapsed = i as f64;
            t.observe(&store, s.clone(), Some(&song), i as f64, 100 + i, 90)
                .unwrap();
        }
        s.elapsed = 0.;
        t.observe(&store, s.clone(), Some(&song), 10., 110, 90)
            .unwrap();
        for i in 1..=5 {
            s.elapsed = i as f64;
            t.observe(&store, s.clone(), Some(&song), (10 + i) as f64, 110 + i, 90)
                .unwrap();
        }
        assert_eq!(totals(&store).0, 2);
        assert_eq!(
            store
                .db
                .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
    #[test]
    fn baseline_and_outbox_are_not_additive_retries() {
        let (store, song, mut s, mut t) = fixture();
        store
            .db
            .execute("UPDATE tracks SET playcount=12", [])
            .unwrap();
        for i in 0..=7 {
            s.elapsed = i as f64;
            t.observe(&store, s.clone(), Some(&song), i as f64, 100 + i, 90)
                .unwrap();
        }
        t.flush(&store, 8., 108).unwrap();
        assert_eq!(totals(&store), (13, 7000));
        let count: String = store
            .db
            .query_row("SELECT value FROM outbox WHERE name='playcount'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(count, "13");
    }
}
