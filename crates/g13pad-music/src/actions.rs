// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    client::{Client, Song, Status},
    history::Store,
};
use g13pad_music::Action;
use rand::{seq::SliceRandom, Rng};
use rusqlite::params;
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, HashMap, VecDeque},
};
type Result<T> = std::result::Result<T, String>;
fn song(client: &mut Client, status: &Status) -> Result<Song> {
    client
        .current()?
        .filter(|s| status.id == Some(s.id))
        .ok_or("no stable current track".into())
}
pub fn ensure(store: &Store, client: &mut Client, s: &Song) -> Result<()> {
    if store.has_track(&s.uri)? {
        return Ok(());
    }
    let mut fields = [0u64; 4];
    for (i, key) in ["playcount", "lastplayed", "listeningtime", "rating"]
        .iter()
        .enumerate()
    {
        match client.sticker(&s.uri, key) {
            Ok(v) => fields[i] = v.and_then(|v| v.parse().ok()).unwrap_or(0),
            Err(_) if client.recoverable() => break,
            Err(e) => return Err(e),
        }
    }
    store.ensure_track(
        s,
        Some((
            fields[0],
            fields[1],
            fields[2].saturating_mul(1000),
            fields[3].min(10) as u32,
        )),
    )
}
pub fn execute(store: &Store, client: &mut Client, action: Action, wall: i64) -> Result<String> {
    let status = client.status()?;
    match action {
        Action::Toggle => {
            if status.stopped {
                client.command("play", &[])?;
            } else {
                client.command("pause", &[if status.playing { "1" } else { "0" }])?;
            }
        }
        Action::Stop => {
            client.command("stop", &[])?;
        }
        Action::Next => {
            client.command("next", &[])?;
        }
        Action::Previous => {
            client.command("previous", &[])?;
        }
        Action::Random => {
            client.command("random", &[if status.random { "0" } else { "1" }])?;
        }
        Action::NextAlbum | Action::PreviousAlbum => {
            let queue = client.queue()?;
            let pos = queue
                .iter()
                .position(|s| Some(s.id) == status.id)
                .ok_or("no current album")?;
            let current = &queue[pos].album_key;
            let target = if action == Action::NextAlbum {
                queue.iter().skip(pos + 1).find(|s| &s.album_key != current)
            } else {
                queue[..pos]
                    .iter()
                    .rposition(|s| &s.album_key != current)
                    .map(|p| {
                        let key = &queue[p].album_key;
                        let start = (0..=p)
                            .rev()
                            .take_while(|&i| &queue[i].album_key == key)
                            .last()
                            .unwrap_or(p);
                        &queue[start]
                    })
            }
            .ok_or("no other album in that direction")?;
            if client.status()?.queue_version != status.queue_version {
                return Err("queue changed; retry album navigation".into());
            }
            client.command("playid", &[&target.id.to_string()])?;
        }
        Action::RateUp | Action::RateDown => {
            let s = song(client, &status)?;
            ensure(store, client, &s)?;
            let rating = client
                .sticker(&s.uri, "rating")?
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(0)
                .min(10);
            let rating = if action == Action::RateUp {
                (rating + 2).min(10)
            } else {
                rating.saturating_sub(2)
            };
            client.set_sticker(&s.uri, "rating", &rating.to_string())?;
            store
                .db
                .execute(
                    "UPDATE tracks SET rating=?1 WHERE uri=?2",
                    params![rating, s.uri],
                )
                .map_err(|e| e.to_string())?;
            return Ok(format!("rating {}/5 stars", rating as f64 / 2.));
        }
        Action::Favourite | Action::Bookmark => {
            let s = song(client, &status)?;
            ensure(store, client, &s)?;
            let tx = store
                .db
                .unchecked_transaction()
                .map_err(|e| e.to_string())?;
            let (key, v) = if action == Action::Favourite {
                tx.execute(
                    "UPDATE tracks SET favourite=1-favourite WHERE uri=?1",
                    [&s.uri],
                )
                .map_err(|e| e.to_string())?;
                (
                    "favourite",
                    tx.query_row("SELECT favourite FROM tracks WHERE uri=?1", [&s.uri], |r| {
                        r.get::<_, i64>(0)
                    })
                    .map_err(|e| e.to_string())?
                    .to_string(),
                )
            } else {
                tx.execute(
                    "UPDATE tracks SET bookmark=?1 WHERE uri=?2",
                    params![status.elapsed, s.uri],
                )
                .map_err(|e| e.to_string())?;
                ("bookmark", format!("{:.3}", status.elapsed))
            };
            tx.execute("INSERT INTO outbox(uri,name,value) VALUES(?1,?2,?3) ON CONFLICT(uri,name) DO UPDATE SET value=excluded.value",params![s.uri,key,v]).map_err(|e|e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(format!("{key}: {v}"));
        }
        Action::Resume => {
            let s = song(client, &status)?;
            let position: Option<f64> = store
                .db
                .query_row("SELECT bookmark FROM tracks WHERE uri=?1", [&s.uri], |r| {
                    r.get(0)
                })
                .map_err(|_| "no bookmark for this track")?;
            let position = position.ok_or("no bookmark for this track")?;
            if status.stopped {
                client.command("playid", &[&s.id.to_string()])?;
            }
            client.command("seekid", &[&s.id.to_string(), &format!("{position:.3}")])?;
            client.command("pause", &["0"])?;
        }
        Action::ShuffleAlbums | Action::SmartShuffle => {
            let queue = client.queue()?;
            if queue.len() < 2 {
                return Err("queue needs at least two tracks".into());
            }
            let mut ratings = HashMap::new();
            if action == Action::SmartShuffle {
                // Cantata's current stickers, including changes made since our last play.
                let mut uri = None;
                match client.command("sticker", &["find", "song", "", "rating"]) {
                    Ok(pairs) => {
                        for (k, v) in pairs {
                            if k == "file" {
                                uri = Some(v);
                            } else if k == "sticker" {
                                if let (Some(u), Some(r)) = (
                                    &uri,
                                    v.strip_prefix("rating=")
                                        .and_then(|v| v.parse::<u32>().ok()),
                                ) {
                                    ratings.insert(u.clone(), r.min(10));
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if !client.recoverable() {
                            return Err(e);
                        }
                        eprintln!("smart shuffle: MPD ratings unavailable; using neutral ratings");
                    }
                }
            }
            let order = album_order(
                &queue,
                status.id,
                action == Action::SmartShuffle,
                store,
                &ratings,
                wall,
            )?;
            let swaps = permutation_swaps(&queue.iter().map(|s| s.id).collect::<Vec<_>>(), &order)?;
            let now = client.status()?;
            if now.queue_version != status.queue_version || now.id != status.id {
                return Err("queue or current track changed; retry shuffle".into());
            }
            // Sequential playback is essential for album ordering. Only explicit shuffle actions change it.
            client.command("random", &["0"])?;
            client.reorder(&swaps)?;
            return Ok(format!(
                "{} queued tracks arranged by album; MPD random off",
                queue.len()
            ));
        }
    }
    Ok(action.label().into())
}
struct Album {
    songs: Vec<Song>,
    weight: f64,
    artist: String,
}
pub fn album_order(
    queue: &[Song],
    current: Option<u32>,
    smart: bool,
    store: &Store,
    ratings: &HashMap<String, u32>,
    wall: i64,
) -> Result<Vec<u32>> {
    let prefix = current
        .and_then(|id| queue.iter().position(|s| s.id == id))
        .map_or(0, |i| i + 1);
    let current_key = prefix.checked_sub(1).map(|i| &queue[i].album_key);
    let mut order = queue[..prefix].iter().map(|s| s.id).collect::<Vec<_>>();
    let mut active = Vec::new();
    let mut groups = BTreeMap::<String, Vec<Song>>::new();
    for s in &queue[prefix..] {
        if current_key == Some(&s.album_key) {
            active.push(s.clone());
        } else {
            groups
                .entry(s.album_key.clone())
                .or_default()
                .push(s.clone());
        }
    }
    let sort = |songs: &mut Vec<Song>| songs.sort_by_key(|s| (s.disc, s.track));
    sort(&mut active);
    order.extend(active.iter().map(|s| s.id));
    let mut query = store
        .db
        .prepare_cached("SELECT playcount,lastplayed,rating,favourite FROM tracks WHERE uri=?1")
        .map_err(|e| e.to_string())?;
    let mut albums = Vec::with_capacity(groups.len());
    for (_, mut songs) in groups {
        sort(&mut songs);
        let mut weights = 0.;
        for s in &songs {
            let row = query.query_row([&s.uri], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, u32>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            });
            let (plays, last, rating, fav) = match row {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => (0, 0, 0, false),
                Err(e) => return Err(e.to_string()),
            };
            let rating = ratings.get(&s.uri).copied().unwrap_or(rating);
            let preference = if rating == 0 {
                1.
            } else {
                0.4 + rating as f64 / 5.
            };
            let freshness = if last == 0 {
                1.
            } else {
                ((wall - last).max(0) as f64 / (7. * 86400.)).clamp(0.08, 1.)
            };
            weights +=
                preference * freshness / (1. + plays as f64).sqrt() * if fav { 1.5 } else { 1. };
        }
        albums.push(Album {
            weight: weights / songs.len() as f64,
            artist: songs[0].artist.clone(),
            songs,
        });
    }
    let mut rng = rand::rng();
    if smart {
        // Exponential weighted keys avoid O(album_count²) selection. A heap repair
        // spaces identical album artists whenever another album is still available.
        albums.sort_by_cached_key(|a| {
            let key = -rng.random::<f64>().max(f64::MIN_POSITIVE).ln() / a.weight.max(0.01);
            (key * 1_000_000.) as u64
        });
        let mut previous = prefix
            .checked_sub(1)
            .map(|i| queue[i].artist.as_str())
            .unwrap_or("")
            .to_owned();
        let mut by_artist = BTreeMap::<String, VecDeque<(usize, Album)>>::new();
        for (rank, album) in albums.drain(..).enumerate() {
            by_artist
                .entry(album.artist.clone())
                .or_default()
                .push_back((rank, album));
        }
        let mut heap = by_artist
            .iter()
            .map(|(artist, rows)| Reverse((rows[0].0, artist.clone())))
            .collect::<BinaryHeap<_>>();
        while let Some(Reverse((rank, artist))) = heap.pop() {
            let chosen = if !previous.is_empty() && artist == previous && !heap.is_empty() {
                let other = heap.pop().unwrap();
                heap.push(Reverse((rank, artist)));
                other.0
            } else {
                (rank, artist)
            };
            let rows = by_artist.get_mut(&chosen.1).unwrap();
            let (_, album) = rows.pop_front().unwrap();
            previous.clone_from(&album.artist);
            albums.push(album);
            if let Some((next, _)) = rows.front() {
                heap.push(Reverse((*next, chosen.1)));
            }
        }
    } else {
        albums.shuffle(&mut rng);
    }
    for album in albums {
        order.extend(album.songs.iter().map(|s| s.id));
    }
    Ok(order)
}
pub fn permutation_swaps(original: &[u32], wanted: &[u32]) -> Result<Vec<(u32, u32)>> {
    if original.len() != wanted.len() {
        return Err("invalid queue permutation".into());
    }
    let mut ids = original.to_vec();
    let mut positions = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, i))
        .collect::<HashMap<_, _>>();
    if positions.len() != original.len() {
        return Err("duplicate queue IDs".into());
    }
    let mut swaps = Vec::new();
    for (i, &id) in wanted.iter().enumerate() {
        let j = *positions.get(&id).ok_or("unknown queue ID")?;
        if i != j {
            let old = ids[i];
            swaps.push((old, id));
            ids.swap(i, j);
            positions.insert(id, i);
            positions.insert(old, j);
        }
    }
    if ids != wanted {
        return Err("duplicate target queue IDs".into());
    }
    Ok(swaps)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shuffle_keeps_prefix_album_discs_and_all_ids() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let queue = [
            (1, "a", 1, 1),
            (2, "b", 1, 2),
            (3, "a", 2, 1),
            (4, "c", 1, 2),
            (5, "b", 1, 1),
            (6, "c", 1, 1),
        ]
        .map(|(id, k, disc, track)| Song {
            id,
            album_key: k.into(),
            artist: k.into(),
            uri: id.to_string(),
            disc,
            track,
            ..Default::default()
        });
        for smart in [true, false] {
            let order = album_order(&queue, Some(1), smart, &store, &HashMap::new(), 100).unwrap();
            assert_eq!(&order[..2], &[1, 3]);
            assert!(order.windows(2).any(|w| w == [5, 2]));
            assert!(order.windows(2).any(|w| w == [6, 4]));
            let swaps =
                permutation_swaps(&queue.iter().map(|s| s.id).collect::<Vec<_>>(), &order).unwrap();
            let mut ids = queue.iter().map(|s| s.id).collect::<Vec<_>>();
            for (a, b) in swaps {
                let i = ids.iter().position(|&id| id == a).unwrap();
                let j = ids.iter().position(|&id| id == b).unwrap();
                ids.swap(i, j);
            }
            assert_eq!(ids, order);
        }
    }
    use std::path::Path;
}
