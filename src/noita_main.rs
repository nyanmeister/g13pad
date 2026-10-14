// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only Noita observer and launch wrapper: `g13map-noita %command%` in Steam.
//!
//! Noita turns achievements, run records and streaks off for any enabled mod, so this
//! is not a mod. It reads the running `noita.exe` (a 32-bit Windows build under Proton)
//! from the host with necauqua's engine reader, which knows the entity and component
//! layouts per game build, and writes the health feed ten times a second: health, the
//! held wand's mana and recharge, the death tally, and the wand's deck as a `deck` strip
//! with the next cast framed. The deck pointer lives in the game's Lua, so it is
//! reconstructed: the shot counter advances it by the draw rules in `gun_actions.lua`,
//! read from the game's own `data.wak`, and a reload resets it.
//!
//! As the game's parent, the feeder may read its memory under Yama's default ptrace
//! scope without any capability. `--observe` reads an already running game instead
//! (that needs CAP_SYS_PTRACE, e.g. through `caprun`).
use std::{
    collections::HashMap,
    env,
    ffi::OsString,
    fs, io,
    io::Write,
    path::{Path, PathBuf},
    process::{self, Command},
    thread,
    time::{Duration, Instant},
};

use noita_engine_reader::{
    discovery::{self, KnownBuild},
    memory::{exe_image::ExeImage, MemoryStorage, ProcessRef},
    types::components::{
        AbilityComponent, DamageModelComponent, ItemActionComponent, ItemComponent,
    },
    Noita, PlayerState,
};

/// Shown health is twenty-five times the stored value.
const HP_SCALE: f64 = 25.0;
const TTL: &str = "ttl 3";
/// The feed shows four icons; six is the parser's cap.
const DECK_SEND: usize = 6;

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

// ---- finding the game ----

/// PIDs whose kernel name is `noita.exe`, descendants of `root` first when `root` is set.
fn find_game(root: Option<u32>) -> Option<u32> {
    let mut found = Vec::new();
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "noita.exe") {
            found.push(pid);
        }
    }
    match root {
        Some(root) => found.into_iter().find(|&pid| descends_from(pid, root)),
        None => found.into_iter().next(),
    }
}

fn descends_from(mut pid: u32, root: u32) -> bool {
    for _ in 0..64 {
        if pid == root {
            return true;
        }
        let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
            return false;
        };
        let Some(ppid) = status
            .lines()
            .find_map(|l| l.strip_prefix("PPid:"))
            .and_then(|v| v.trim().parse::<u32>().ok())
        else {
            return false;
        };
        if ppid <= 1 {
            return false;
        }
        pid = ppid;
    }
    false
}

/// The game folder, from Steam's environment in the game process, else its cwd.
fn install_dir(pid: u32) -> Option<PathBuf> {
    if let Ok(env) = fs::read(format!("/proc/{pid}/environ")) {
        for var in env.split(|b| *b == 0) {
            if let Some(v) = var.strip_prefix(b"STEAM_COMPAT_INSTALL_PATH=") {
                let p = PathBuf::from(String::from_utf8_lossy(v).into_owned());
                if p.join("data/data.wak").is_file() {
                    return Some(p);
                }
            }
        }
    }
    let cwd = fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    cwd.join("data/data.wak").is_file().then_some(cwd)
}

// ---- the game's own data: spell sprites and draw rules out of data.wak ----

/// One spell as `gun_actions.lua` describes it: where its icon is, and how many more
/// cards it draws when cast (a modifier draws one, a multicast two or three, a trigger
/// draws its payload).
#[derive(Clone, Debug, Default)]
struct Action {
    sprite: String,
    draws: u32,
}

struct Wak {
    path: PathBuf,
    index: HashMap<String, (u64, u64)>,
    actions: HashMap<String, Action>,
    icons: HashMap<String, Option<[u8; 32]>>,
}

impl Wak {
    fn open(dir: &Path) -> io::Result<Self> {
        let path = dir.join("data/data.wak");
        let bytes = fs::read(&path)?;
        let word = |at: usize| -> io::Result<u32> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                .ok_or_else(|| invalid("data.wak truncated"))
        };
        let count = word(4)? as usize;
        if count > 1_000_000 {
            return Err(invalid("data.wak index count out of range"));
        }
        let mut index = HashMap::with_capacity(count);
        let mut at = 16;
        for _ in 0..count {
            let (off, size, len) = (word(at)?, word(at + 4)?, word(at + 8)? as usize);
            let name = bytes
                .get(at + 12..at + 12 + len)
                .ok_or_else(|| invalid("data.wak name truncated"))?;
            index.insert(
                String::from_utf8_lossy(name).into_owned(),
                (off as u64, size as u64),
            );
            at += 12 + len;
        }
        let mut wak = Wak {
            path,
            index,
            actions: HashMap::new(),
            icons: HashMap::new(),
        };
        let lua = wak.file("data/scripts/gun/gun_actions.lua")?;
        wak.actions = parse_actions(&String::from_utf8_lossy(&lua));
        if wak.actions.is_empty() {
            return Err(invalid("no actions parsed from gun_actions.lua"));
        }
        Ok(wak)
    }
    fn file(&self, name: &str) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        let &(off, size) = self
            .index
            .get(name)
            .ok_or_else(|| invalid(format!("{name} not in data.wak")))?;
        if size > 16 << 20 {
            return Err(invalid(format!("{name} too large")));
        }
        let f = fs::File::open(&self.path)?;
        let mut buf = vec![0; size as usize];
        f.read_exact_at(&mut buf, off)?;
        Ok(buf)
    }
    /// The spell's icon as the feed's 32 bytes, decoded once; `None` when it has none.
    fn icon(&mut self, action_id: &str) -> Option<[u8; 32]> {
        if let Some(cached) = self.icons.get(action_id) {
            return *cached;
        }
        let icon = self
            .actions
            .get(action_id)
            .filter(|a| !a.sprite.is_empty())
            .and_then(|a| self.file(&a.sprite).ok())
            .and_then(|png| image::load_from_memory(&png).ok())
            .map(|img| one_bit(&img.to_rgba8()));
        self.icons.insert(action_id.to_owned(), icon);
        icon
    }
    fn draws(&self, action_id: &str) -> u32 {
        self.actions.get(action_id).map_or(0, |a| a.draws)
    }
}

/// `id = "X"`, `sprite = "path"` and the draw calls between one `id` and the next.
fn parse_actions(lua: &str) -> HashMap<String, Action> {
    let mut out = HashMap::new();
    let mut id: Option<String> = None;
    let mut action = Action::default();
    let quoted = |line: &str, key: &str| -> Option<String> {
        let rest = line
            .trim_start()
            .strip_prefix(key)?
            .trim_start()
            .strip_prefix('=')?;
        let rest = rest.trim_start().strip_prefix('"')?;
        Some(rest.split('"').next()?.to_owned())
    };
    let number_after = |line: &str, call: &str| -> u32 {
        let mut total = 0;
        let mut rest = line;
        while let Some(at) = rest.find(call) {
            rest = &rest[at + call.len()..];
            // draw_actions( N, ... ) or add_projectile_trigger_x( "file", N )
            let args = rest.split(')').next().unwrap_or("");
            let n = if call.starts_with("draw_actions") {
                args.split(',').next()
            } else {
                args.split(',').nth(1)
            };
            total += n.and_then(|s| s.trim().parse::<u32>().ok()).unwrap_or(0);
        }
        total
    };
    for line in lua.lines() {
        let t = line.trim_start();
        if t.starts_with("--") {
            continue;
        }
        if let Some(new_id) = quoted(line, "id") {
            if let Some(prev) = id.take() {
                out.insert(prev, std::mem::take(&mut action));
            }
            id = Some(new_id);
            continue;
        }
        if id.is_none() {
            continue;
        }
        if let Some(sprite) = quoted(line, "sprite") {
            action.sprite = sprite;
        }
        action.draws += number_after(line, "draw_actions(");
        action.draws += number_after(line, "add_projectile_trigger_hit_world(");
        action.draws += number_after(line, "add_projectile_trigger_timer(");
        action.draws += number_after(line, "add_projectile_trigger_death(");
    }
    if let Some(prev) = id {
        out.insert(prev, action);
    }
    out
}

/// A 16x16 sprite on a one-bit glass: lit where it is opaque and bright, the same
/// threshold the mock the owner picked used (2026-10-13). Smaller art sits top-left.
fn one_bit(img: &image::RgbaImage) -> [u8; 32] {
    let mut out = [0u8; 32];
    for y in 0..16.min(img.height()) {
        let mut bits = 0u16;
        for x in 0..16.min(img.width()) {
            let [r, g, b, a] = img.get_pixel(x, y).0;
            let lum = 0.3 * r as f32 + 0.59 * g as f32 + 0.11 * b as f32;
            if a > 128 && lum > 70.0 {
                bits |= 0x8000 >> x;
            }
        }
        out[2 * y as usize..2 * y as usize + 2].copy_from_slice(&bits.to_be_bytes());
    }
    out
}

fn hex(icon: &[u8; 32]) -> String {
    icon.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- the deck: the game's Lua owns the pointer, so it is reconstructed ----

#[derive(Clone, Debug, PartialEq)]
struct Card {
    id: String,
    spent: bool,
}

/// What the next cast draws from `cards` starting at `at`, by the gun script's rule:
/// each card consumes a draw and adds its own draws; spent cards are passed over; the
/// deck ends where the cards do (then the wand recharges and the pointer returns).
fn hand(cards: &[Card], at: usize, per_cast: u32, draws_of: impl Fn(&str) -> u32) -> Vec<usize> {
    let mut hand = Vec::new();
    let mut draws = per_cast.max(1);
    let mut i = at;
    while draws > 0 && i < cards.len() {
        if cards[i].spent {
            i += 1;
            continue;
        }
        hand.push(i);
        draws = draws - 1 + draws_of(&cards[i].id);
        i += 1;
    }
    hand
}

#[derive(Default)]
struct DeckTracker {
    wand: u32,
    shots: i32,
    at: usize,
}

impl DeckTracker {
    /// Advance by the casts since the last tick; a new wand or a recharge restarts.
    fn observe(
        &mut self,
        wand: u32,
        shots: i32,
        recharging: bool,
        cards: &[Card],
        per_cast: u32,
        draws_of: &dyn Fn(&str) -> u32,
    ) {
        if wand != self.wand {
            self.wand = wand;
            self.shots = shots;
            self.at = 0;
        }
        if recharging || self.at >= cards.len() {
            self.at = 0;
        }
        let casts = (shots - self.shots).clamp(0, 16);
        for _ in 0..casts {
            let drawn = hand(cards, self.at, per_cast, draws_of);
            self.at = drawn.last().map_or(cards.len(), |&i| i + 1);
            if self.at >= cards.len() {
                self.at = 0;
                break;
            }
        }
        self.shots = shots;
    }
}

// ---- reading ----

struct Reader {
    noita: Noita,
    wak: Option<Wak>,
    tracker: DeckTracker,
    bad: u32,
    wand_errors: u32,
    /// `G13MAP_NOITA_DEBUG=1`: the wand's frame fields and the deck pointer on stderr.
    debug: bool,
}

impl Reader {
    fn open(pid: u32) -> io::Result<Self> {
        let proc = ProcessRef::connect(pid)?;
        let ts = proc.header().timestamp();
        let globals = match KnownBuild::from_timestamp(ts) {
            Some(build) => {
                eprintln!("g13map-noita: observing noita.exe PID {pid}, build {build:?}");
                build.map()
            }
            None => {
                // Asked 2026-10-13: attempt newer builds, never refuse them. The reader's
                // own discovery scans the executable image for the globals.
                eprintln!("g13map-noita: PID {pid} is an unknown build (PE timestamp {ts:#x}); discovering its layout");
                let image = ExeImage::read(&proc)?;
                discovery::run(&image)
            }
        };
        let wak = install_dir(pid).and_then(|dir| match Wak::open(&dir) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("g13map-noita: no spell icons: {e}");
                None
            }
        });
        Ok(Reader {
            noita: Noita::new(proc, globals),
            wak,
            tracker: DeckTracker::default(),
            bad: 0,
            wand_errors: 0,
            debug: env::var_os("G13MAP_NOITA_DEBUG").is_some(),
        })
    }

    fn line(&mut self) -> io::Result<String> {
        let deaths = self
            .noita
            .read_stats()
            .map(|s| s.global.death_count)
            .unwrap_or(0);
        let Some((player, state)) = self.noita.get_player()? else {
            return Ok(format!("wait {TTL}\n"));
        };
        let dmc = self
            .noita
            .component_store::<DamageModelComponent>()?
            .get(&player)?
            .ok_or_else(|| invalid("player without a damage model"))?;
        let (hp, max) = (dmc.hp.get() * HP_SCALE, dmc.max_hp.get() * HP_SCALE);
        if !(hp.is_finite() && max.is_finite()) || max <= 0.0 || max > 1.0e9 {
            return Err(invalid(format!("implausible health {hp}/{max}")));
        }
        if hp <= 0.0 {
            return Ok(format!("0 {TTL} deaths {deaths}\n"));
        }
        // The deck must be the last word (its icons run to the end of the line).
        let mut line = format!(
            "{}/{} {TTL} deaths {deaths}",
            hp.round() as u64,
            max.round() as u64
        );
        if matches!(state, PlayerState::Normal) {
            // A wand mid-swap or mid-edit can hand out a null pointer for a tick; the
            // health line stands on its own without the wand words.
            match self.wand(&player) {
                Ok(Some(w)) => line.push_str(&w),
                Ok(None) => {}
                Err(e) => {
                    self.wand_errors += 1;
                    if self.wand_errors == 1 || self.wand_errors.is_multiple_of(100) {
                        eprintln!("g13map-noita: wand: {e} ({} so far)", self.wand_errors);
                    }
                }
            }
        }
        line.push('\n');
        Ok(line)
    }

    /// The held wand's words: mana, recharge, and the deck from the next card on.
    fn wand(&mut self, player: &noita_engine_reader::types::Entity) -> io::Result<Option<String>> {
        let p = self.noita.proc().clone();
        let Some(inv) = self
            .noita
            .component_store::<Inventory2Component>()?
            .get(player)?
        else {
            return Ok(None);
        };
        let Some(quick) = player.first_child_by_name("inventory_quick", &p)? else {
            return Ok(None);
        };
        let abilities = self.noita.component_store::<AbilityComponent>()?;
        let items = self.noita.component_store::<ItemComponent>()?;
        let actions = self.noita.component_store::<ItemActionComponent>()?;
        let mut wand = None;
        for child in quick.children.read(&p)?.read_storage(&p)? {
            if child.id == inv.m_active_item {
                wand = Some(child);
                break;
            }
        }
        let Some(wand) = wand else {
            return Ok(None);
        };
        let Some(ab) = abilities.get(&wand)? else {
            return Ok(None); // a flask or a kick, not a wand
        };
        let (mana, mana_max) = (ab.mana, ab.mana_max);
        if !(mana.is_finite() && mana_max.is_finite()) || mana_max < 0.0 || mana_max > 1.0e7 {
            return Err(invalid("implausible mana"));
        }
        let frame = self
            .noita
            .read_game_global()
            .map(|g| g.frame_counter as i32)
            .unwrap_or(0);
        // Frames until the wand fires again: the later of the recharge's end and the cast
        // delay's end, against the game's frame counter. (`m_reload_frames_left` is the
        // recharge's length, not a countdown: it read a constant 28 through six minutes
        // of play on 2026-10-13.)
        let recharge = (ab.m_reload_next_frame_usable.max(ab.m_next_frame_usable) - frame).max(0);
        if self.debug {
            eprintln!(
                "g13map-noita: frame {frame} next_usable {} reload_next_usable {} reload_frames_left {} cast_delay_start {} shots {} wand {}",
                ab.m_next_frame_usable,
                ab.m_reload_next_frame_usable,
                ab.m_reload_frames_left,
                ab.m_cast_delay_start_frame,
                ab.stat_times_player_has_shot,
                wand.id
            );
        }
        let mut words = format!(
            " count MANA {}/{} count RLD {}",
            mana.max(0.0).round() as u32,
            mana_max.round() as u32,
            recharge.min(99_999)
        );
        // The spells in slot order, with whether each is spent.
        let mut cards: Vec<(i32, Card)> = Vec::new();
        for spell in wand.children.read(&p)?.read_storage(&p)? {
            let Some(action) = actions.get(&spell)? else {
                continue;
            };
            let Some(item) = items.get(&spell)? else {
                continue;
            };
            if cards.len() >= 64 {
                break;
            }
            let id = action.action_id.read(&p)?;
            cards.push((
                item.inventory_slot.x,
                Card {
                    id,
                    spent: item.uses_remaining == 0,
                },
            ));
        }
        cards.sort_by_key(|(slot, _)| *slot);
        let cards: Vec<Card> = cards.into_iter().map(|(_, c)| c).collect();
        let Some(wak) = self.wak.as_mut() else {
            return Ok(Some(words));
        };
        if cards.is_empty() {
            return Ok(Some(words));
        }
        let per_cast = ab.gun_config.actions_per_round.clamp(1, 32) as u32;
        let shuffle = ab.gun_config.shuffle_deck_when_empty.as_bool();
        let draws_of = |id: &str| wak.draws(id);
        // Recharging now: the recharge's end frame is still ahead. (`m_reload_frames_left`
        // is its length and is never zero, which once reset the pointer every tick.)
        let recharging = ab.m_reload_next_frame_usable > frame;
        self.tracker.observe(
            wand.id,
            ab.stat_times_player_has_shot,
            recharging,
            &cards,
            per_cast,
            &draws_of,
        );
        let at = self.tracker.at.min(cards.len() - 1);
        if self.debug {
            let ids: Vec<&str> = cards.iter().map(|c| c.id.as_str()).collect();
            eprintln!(
                "g13map-noita: deck at {at} per_cast {per_cast} shuffle {shuffle} cards {ids:?}"
            );
        }
        let next = if shuffle {
            0
        } else {
            hand(&cards, at, per_cast, draws_of).len()
        };
        // Slot order, as the inventory shows them; six at most, the window chosen so
        // the framed run is inside it; `at` is sent relative to that window.
        let start = (at + next)
            .saturating_sub(DECK_SEND)
            .min(cards.len().saturating_sub(DECK_SEND));
        let mut icons = Vec::new();
        let mut sent_at = None;
        for (i, card) in cards.iter().enumerate().skip(start).take(DECK_SEND) {
            if let Some(icon) = wak.icon(&card.id) {
                if i == at {
                    sent_at = Some(icons.len());
                }
                icons.push(hex(&icon));
            }
        }
        if !icons.is_empty() {
            let sent_at = sent_at.unwrap_or(0).min(icons.len() - 1);
            let next = next.min(icons.len() - sent_at);
            words.push_str(&format!(" deck {sent_at} {next} {}", icons.join(" ")));
        }
        Ok(Some(words))
    }
}

/// `Inventory2Component` as laid out in the Jan 25 2025 build: the documented members
/// then its privates (verified live 2026-10-13: slots 16x1, the active item a wand).
#[derive(zerocopy::FromBytes, zerocopy::IntoBytes, Debug)]
#[repr(C)]
struct Inventory2Component {
    quick_inventory_slots: i32,
    full_inventory_slots_x: i32,
    full_inventory_slots_y: i32,
    m_saved_active_item_index: u32,
    m_active_item: u32,
    m_actual_active_item: u32,
}

impl noita_engine_reader::types::components::ComponentName for Inventory2Component {
    const NAME: &str = "Inventory2Component";
}

// ---- the feed ----

fn feed_path() -> PathBuf {
    env::var_os("G13MAP_HEALTH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let state = env::var_os("XDG_STATE_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state")
                });
            state.join("g13map/health")
        })
}

fn publish(path: &Path, line: &str) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| invalid("feed has no parent"))?;
    fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!("noita-{}.tmp", process::id()));
    fs::write(&tmp, line)?;
    fs::rename(tmp, path)
}

fn run() -> io::Result<i32> {
    let args: Vec<OsString> = env::args_os().skip(1).collect();
    if args.first().is_some_and(|s| s == "--version") {
        println!(
            "g13map-noita {} (read-only; tested Jan 25 2025 build)",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(0);
    }
    if args.first().is_some_and(|s| s == "--help") || args.is_empty() {
        println!("g13map-noita GAME [ARGS...]\nSteam: g13map-noita %command%\nAlready running game: g13map-noita --observe [--once] (needs CAP_SYS_PTRACE)\nReads the running noita.exe's memory; no mod, no game writes, achievements stay on.");
        return Ok(0);
    }
    let observe = args[0] == "--observe";
    let once = observe && args.get(1).is_some_and(|s| s == "--once");
    let path = feed_path();
    let mut child = if observe {
        None
    } else {
        Some(Command::new(&args[0]).args(&args[1..]).spawn()?)
    };
    let root = child.as_ref().map(|_| process::id());
    let mut reader: Option<Reader> = None;
    let mut next_scan = Instant::now();
    let mut warned = false;
    let mut last_line = String::new();
    let mut last_write = Instant::now() - Duration::from_secs(2);
    loop {
        if let Some(c) = child.as_mut() {
            if let Some(status) = c.try_wait()? {
                return Ok(status.code().unwrap_or(1));
            }
        }
        if reader.is_none() && Instant::now() >= next_scan {
            next_scan = Instant::now() + Duration::from_secs(1);
            if let Some(pid) = find_game(root) {
                match Reader::open(pid) {
                    Ok(r) => reader = Some(r),
                    Err(e) => {
                        if !warned {
                            eprintln!("g13map-noita: {e}; game continues without telemetry");
                            warned = true;
                        }
                    }
                }
            } else if observe && once {
                eprintln!("g13map-noita: no noita.exe process");
                return Ok(2);
            }
        }
        let line = match reader.as_mut() {
            Some(r) => match r.line() {
                Ok(line) => {
                    r.bad = 0;
                    line
                }
                Err(e) => {
                    r.bad += 1;
                    if r.bad == 1 || r.bad % 50 == 0 {
                        eprintln!("g13map-noita: read: {e}");
                    }
                    if r.bad >= 10 && e.kind() == io::ErrorKind::NotFound
                        || fs::metadata(format!("/proc/{}", r.noita.proc().pid())).is_err()
                    {
                        eprintln!("g13map-noita: game process gone; waiting for another");
                        reader = None;
                    }
                    format!("wait {TTL}\n")
                }
            },
            None => format!("wait {TTL}\n"),
        };
        if line != last_line || last_write.elapsed() >= Duration::from_secs(1) {
            if let Err(e) = publish(&path, &line) {
                if !warned {
                    eprintln!("g13map-noita: feed: {e}");
                    warned = true;
                }
            }
            last_line = line.clone();
            last_write = Instant::now();
        }
        if observe {
            print!("{line}");
            io::stdout().flush()?;
            if once && reader.is_some() {
                return Ok(0);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn main() {
    match run() {
        Ok(code) => process::exit(code),
        Err(e) => {
            eprintln!("g13map-noita: {e}");
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(ids: &[&str]) -> Vec<Card> {
        ids.iter()
            .map(|id| Card {
                id: (*id).to_owned(),
                spent: false,
            })
            .collect()
    }
    /// The draw rules of the real spells used below, as gun_actions.lua states them.
    fn draws(id: &str) -> u32 {
        match id {
            "DAMAGE" | "SPEED" => 1,     // modifiers: draw_actions( 1, true )
            "LIGHT_BULLET_TRIGGER" => 1, // add_projectile_trigger_hit_world( ..., 1 )
            "BURST_2" => 2,              // draw_actions( 2, true )
            _ => 0,
        }
    }

    #[test]
    fn hand_follows_the_gun_script_rules() {
        // One per cast: a plain bolt is one card.
        let deck = cards(&["LIGHT_BULLET", "CHAINSAW", "BOMB"]);
        assert_eq!(hand(&deck, 0, 1, draws), vec![0]);
        assert_eq!(hand(&deck, 2, 1, draws), vec![2]);
        // A modifier pulls the next card with it: DAMAGE + CHAINSAW is one cast.
        let deck = cards(&["DAMAGE", "CHAINSAW", "BOMB"]);
        assert_eq!(hand(&deck, 0, 1, draws), vec![0, 1]);
        // A trigger draws its payload; a two-per-cast wand draws two such groups.
        let deck = cards(&[
            "LIGHT_BULLET_TRIGGER",
            "BOMB",
            "SPEED",
            "CHAINSAW",
            "LIGHT_BULLET",
        ]);
        assert_eq!(hand(&deck, 0, 1, draws), vec![0, 1]);
        assert_eq!(hand(&deck, 0, 2, draws), vec![0, 1, 2, 3]);
        // Spent cards are passed over; the deck ends where the cards do.
        let mut deck = cards(&["BOMB", "LIGHT_BULLET", "DAMAGE"]);
        deck[0].spent = true;
        assert_eq!(hand(&deck, 0, 1, draws), vec![1]);
        assert_eq!(hand(&deck, 2, 1, draws), vec![2]); // DAMAGE with nothing after it
        assert_eq!(hand(&deck, 3, 1, draws), Vec::<usize>::new());
    }

    #[test]
    fn tracker_advances_per_shot_and_wraps_on_recharge() {
        let deck = cards(&["LIGHT_BULLET", "DAMAGE", "CHAINSAW", "BOMB"]);
        let mut t = DeckTracker::default();
        let d: &dyn Fn(&str) -> u32 = &draws;
        t.observe(7, 10, false, &deck, 1, d);
        assert_eq!(t.at, 0, "a new wand starts at the top");
        t.observe(7, 11, false, &deck, 1, d);
        assert_eq!(t.at, 1, "one bolt cast");
        t.observe(7, 12, false, &deck, 1, d);
        assert_eq!(t.at, 3, "DAMAGE took CHAINSAW with it");
        t.observe(7, 13, false, &deck, 1, d);
        assert_eq!(t.at, 0, "the bomb was the last card: back to the top");
        t.observe(7, 14, false, &deck, 1, d);
        t.observe(7, 14, true, &deck, 1, d);
        assert_eq!(t.at, 0, "a recharge restarts the deck");
        t.observe(8, 14, false, &deck, 1, d);
        assert_eq!((t.at, t.wand), (0, 8), "a different wand starts over");
    }

    #[test]
    fn actions_parse_from_the_real_script_shape() {
        let lua = r#"
actions =
{
	{
		id          = "LIGHT_BULLET_TRIGGER",
		name 		= "$action_light_bullet_trigger",
		sprite 		= "data/ui_gfx/gun_actions/light_bullet_trigger.png",
		sprite_unidentified = "data/ui_gfx/gun_actions/light_bullet_trigger_unidentified.png",
		action 		= function()
			c.fire_rate_wait = c.fire_rate_wait + 3
			add_projectile_trigger_hit_world("data/entities/projectiles/deck/light_bullet.xml", 1)
		end,
	},
	{
		id          = "DAMAGE",
		sprite 		= "data/ui_gfx/gun_actions/damage.png",
		-- draw_actions( 9, true ) in a comment does not count
		action 		= function()
			c.damage_projectile_add = c.damage_projectile_add + 0.4
			draw_actions( 1, true )
		end,
	},
	{
		id          = "BURST_2",
		sprite 		= "data/ui_gfx/gun_actions/burst_2.png",
		action 		= function() draw_actions( 2, true ) end,
	},
}
"#;
        let a = parse_actions(lua);
        assert_eq!(a.len(), 3);
        assert_eq!(
            a["LIGHT_BULLET_TRIGGER"].sprite,
            "data/ui_gfx/gun_actions/light_bullet_trigger.png"
        );
        assert_eq!(a["LIGHT_BULLET_TRIGGER"].draws, 1);
        assert_eq!(a["DAMAGE"].draws, 1);
        assert_eq!(a["BURST_2"].draws, 2);
    }

    #[test]
    fn one_bit_lights_bright_opaque_pixels_only() {
        let mut img = image::RgbaImage::new(16, 16);
        img.put_pixel(0, 0, image::Rgba([255, 255, 255, 255])); // lit
        img.put_pixel(15, 0, image::Rgba([255, 255, 255, 40])); // transparent: dark
        img.put_pixel(0, 15, image::Rgba([30, 30, 30, 255])); // too dark: dark
        img.put_pixel(15, 15, image::Rgba([200, 120, 40, 255])); // orange: lit
        let bits = one_bit(&img);
        assert_eq!(&bits[0..2], &[0x80, 0x00]);
        assert_eq!(&bits[30..32], &[0x00, 0x01]);
        assert_eq!(hex(&bits).len(), 64);
    }
}
