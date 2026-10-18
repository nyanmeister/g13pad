// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use std::fs::OpenOptions;
struct Fixture {
    reader: Reader,
    path: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "g13-everspace-reader-{}-{:?}",
            process::id(),
            thread::current().id()
        ));
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.set_len(0x60000).unwrap();
        let fixture = Self {
            reader: Reader {
                mem: file,
                globals: [0x10000, 0x10100, 0x10200, 0x10300, 0x10400, 0x10500],
                image: 0,
                energy_sample: Cell::new(None),
            },
            path,
        };
        // Independently laid out from the executable's DWARF (layouts.txt), not
        // a serialization of Reader. Deliberately unequal resources distinguish swaps.
        for (at, value) in [
            (0x10000, 0x20000),
            (0x20140, 0x21000), // GWorld, OwningGameInstance
            (0x21038, 0x22000),
            (0x22000, 0x23000), // LocalPlayers, first local player
            (0x23030, 0x24000),
            (0x24390, 0x25000), // PlayerController, Pawn
            (0x253e8, 0x26000),
            (0x253f0, 0x27000),
            (0x25418, 0x28000),
            (0x10200, 0x30000),
            (0x10300, 0x31000),
            (0x10400, 0x32000),
            (0x10500, 0x33000),
            (0x25010, 0x30000),
            (0x26010, 0x31000),
            (0x27010, 0x32000),
            (0x28010, 0x33000),
            (0x10110, 0x40000),
            (0x40030, 0x29000), // FUObjectArray data, slot index2
        ] {
            fixture.word(at, value);
        }
        for (at, value) in [
            (0x21040, 1),
            (0x21044, 1),
            (0x10118, 8),
            (0x1011c, 3),
            (0x27308, 2),
            (0x2730c, 77),
            (0x40040, 77),
        ] {
            fixture.int(at, value);
        }
        for (at, value) in [
            (0x26200, 150.0),
            (0x26204, 37.5), // hull 25%
            (0x271d0, 42.0),
            (0x29444, 40.0), // shield: 40*(1+0.5)*2=120;42/120=35%
            (0x272b0, 0.5),
            (0x2728c, 2.0),
            (0x281dc, 200.0),
            (0x28260, 40.0),
            (0x2825c, 96.0), // 96/(200-40)=60%
        ] {
            fixture.float(at, value);
        }
        fixture
    }
    fn word(&self, at: u64, n: u64) {
        self.reader.mem.write_all_at(&n.to_le_bytes(), at).unwrap();
    }
    fn int(&self, at: u64, n: i32) {
        self.reader.mem.write_all_at(&n.to_le_bytes(), at).unwrap();
    }
    fn float(&self, at: u64, n: f32) {
        self.reader.mem.write_all_at(&n.to_le_bytes(), at).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
#[test]
fn hand_counted_native_ship_and_effective_capacities() {
    let f = Fixture::new();
    assert_eq!(
        f.reader.snapshot().unwrap(),
        "ship 1 hull 25 shield 35 energy 60 ttl 3 shield_delay 0\n"
    );
    f.float(0x271d0, 0.0);
    assert_eq!(
        f.reader.snapshot().unwrap(),
        "ship 1 hull 25 shield 0 energy 60 ttl 3 shield_delay 0\n"
    );
    f.int(0x2730c, 0);
    assert_eq!(
        f.reader.snapshot().unwrap(),
        "ship 1 hull 25 shield none energy 60 ttl 3\n"
    );
    f.float(0x26204, -1.0);
    assert_eq!(
        f.reader.snapshot().unwrap(),
        "ship 1 hull 0 shield none energy 60 ttl 3\n"
    );
}
#[test]
fn absent_pawn_foreign_class_and_loading_are_wait_not_hull() {
    let f = Fixture::new();
    f.word(0x25010, 0x34000);
    assert_eq!(f.reader.snapshot().unwrap(), "wait ttl 3\n");
    // Blueprint subclass follows SuperStruct to the native ship class.
    f.word(0x34030, 0x30000);
    assert!(f.reader.snapshot().unwrap().starts_with("ship 1 hull 25"));
    f.word(0x24390, 0);
    assert_eq!(f.reader.snapshot().unwrap(), "wait ttl 3\n");
    f.word(0x10000, 0);
    assert_eq!(f.reader.snapshot().unwrap(), "wait ttl 3\n");
}
#[test]
fn stale_weak_serial_is_absent_but_corrupt_resources_are_rejected() {
    let f = Fixture::new();
    f.int(0x40040, 78);
    assert!(f.reader.snapshot().unwrap().contains("shield none"));
    f.int(0x40040, 77);
    f.int(0x27308, 3); // count3 means valid slots0,1,2 only.
    assert!(f.reader.snapshot().is_err());
    f.int(0x27308, 2);
    f.float(0x26204, f32::NAN);
    assert!(f.reader.snapshot().is_err());
    f.float(0x26204, 37.5);
    f.reader.mem.write_all_at(&[1], 0x272a0).unwrap();
    assert_eq!(
        f.reader.snapshot().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    f.reader.mem.write_all_at(&[0], 0x272a0).unwrap();
    f.int(0x21040, 5);
    assert!(f.reader.snapshot().is_err());
}
#[test]
fn resource_ranges_and_rounding_use_independent_integer_oracle() {
    let mut entropy = [0; 8];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact_at(&mut entropy, 0)
        .unwrap();
    let mut seed = u64::from_ne_bytes(entropy) | 1;
    eprintln!("everspace ratio seed {seed}");
    for _ in 0..2000 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let maximum = seed % 100_000 + 1;
        let current = (seed >> 20) % (maximum + 1);
        let expected = (current * 100 + maximum / 2) / maximum;
        assert_eq!(
            ratio(current as f64, maximum as f64).unwrap(),
            expected as u32
        );
        assert!(ratio(current as f64, 0.0).is_err());
    }
    assert!(ratio(f64::NAN, 100.0).is_err());
    assert!(ratio(10.0, f64::INFINITY).is_err());
}

#[test]
fn repair_budget_matches_resource_class_and_caps_missing_hull() {
    let f = Fixture::new();
    f.word(0x26210, 0x35000); // Health.NanoBotPickupClass
    f.float(0x2620c, 30.0);
    f.word(0x210f0, 0x41000); // GameInstance.CurrentRunData
    f.word(0x41090, 0x42000);
    f.int(0x41098, 2);
    f.int(0x4109c, 2);
    f.word(0x42000, 0x36000);
    f.int(0x42008, 999); // another resource
    f.word(0x42010, 0x35000);
    f.int(0x42018, 3);
    // 3 * 30 / 150 = 60% recoverable, not the other resource's amount.
    assert!(f.reader.snapshot().unwrap().contains("repair 60"));
    f.int(0x42018, 10);
    // Missing112.5/150=75%; unused repair capacity cannot exceed full hull.
    assert!(f.reader.snapshot().unwrap().contains("repair 75"));
    // Resource classes marked persistent use player inventory outside hardcore.
    f.word(0x21420, 0x43000);
    f.int(0x21428, 1);
    f.int(0x2142c, 1);
    f.word(0x43000, 0x35000);
    f.word(0x210f8, 0x44000);
    f.word(0x44170, 0x45000);
    f.int(0x44178, 1);
    f.int(0x4417c, 1);
    f.word(0x45000, 0x35000);
    f.int(0x45008, 1);
    assert!(f.reader.snapshot().unwrap().contains("repair 20"));
    f.reader.mem.write_all_at(&[1], 0x4118e).unwrap();
    assert!(f.reader.snapshot().unwrap().contains("repair 75"));
}
#[test]
fn shield_cooldown_uses_longer_parallel_timer() {
    let f = Fixture::new();
    f.float(0x29448, 4.0);
    f.float(0x272ac, 0.5);
    f.float(0x27264, 2.0);
    f.float(0x27310, 5.0);
    f.float(0x27314, 2.0);
    // Timers run concurrently: max(5,2)=5 seconds before recharging.
    assert!(f.reader.snapshot().unwrap().contains("shield_delay 5"));
    f.float(0x27310, 1.0);
    f.float(0x27314, 4.0);
    assert!(f.reader.snapshot().unwrap().contains("shield_delay 4"));
    f.float(0x27310, 0.0);
    f.float(0x27314, 0.0);
    assert!(f.reader.snapshot().unwrap().contains("shield_delay 0"));
}
#[test]
fn energy_prediction_uses_game_time_and_changes_direction() {
    let f = Fixture::new();
    f.float(0x20900, 10.0); // UWorld.TimeSeconds
    f.float(0x28204, 10.0);
    f.float(0x28278, 1.2);
    // Available160-current96 =>64 at10/sec +1.2 delay=7.6 =>8sec.
    assert!(f.reader.snapshot().unwrap().contains("energy_time 8"));
    f.float(0x20900, 10.2);
    f.float(0x2825c, 88.0);
    // Eight units consumed in0.2s =>88/40=2.2sec =>3sec to empty.
    assert!(f.reader.snapshot().unwrap().contains("energy_time -3"));
    assert!(f.reader.snapshot().unwrap().contains("energy_time -3")); // paused game
    f.float(0x20900, 0.0); // new world/game-time epoch must discard prior drain
    assert!(f.reader.snapshot().unwrap().contains("energy_time 9"));
}

#[test]
fn structured_elf_symbols_move_and_out_of_file_sections_fail() {
    let f = Fixture::new();
    let file = &f.reader.mem;
    let mut header = [0u8; 64];
    header[..6].copy_from_slice(b"\x7fELF\x02\x01");
    header[18..20].copy_from_slice(&62u16.to_le_bytes());
    header[40..48].copy_from_slice(&0x100u64.to_le_bytes());
    header[58..60].copy_from_slice(&64u16.to_le_bytes());
    header[60..62].copy_from_slice(&3u16.to_le_bytes());
    file.write_all_at(&header, 0).unwrap();
    let mut names = [0u8; 64];
    names[4..8].copy_from_slice(&3u32.to_le_bytes());
    names[24..32].copy_from_slice(&0x300u64.to_le_bytes());
    names[32..40].copy_from_slice(&8u64.to_le_bytes());
    file.write_all_at(&names, 0x140).unwrap();
    file.write_all_at(b"\0GWorld\0", 0x300).unwrap();
    let mut table = [0u8; 64];
    table[4..8].copy_from_slice(&2u32.to_le_bytes());
    table[24..32].copy_from_slice(&0x400u64.to_le_bytes());
    table[32..40].copy_from_slice(&24u64.to_le_bytes());
    table[40..44].copy_from_slice(&1u32.to_le_bytes());
    table[56..64].copy_from_slice(&24u64.to_le_bytes());
    file.write_all_at(&table, 0x180).unwrap();
    let mut symbol = [0u8; 24];
    symbol[..4].copy_from_slice(&1u32.to_le_bytes());
    symbol[6..8].copy_from_slice(&1u16.to_le_bytes());
    let mut entropy = [0; 8];
    File::open("/dev/urandom")
        .unwrap()
        .read_exact_at(&mut entropy, 0)
        .unwrap();
    let mut seed = u64::from_ne_bytes(entropy) | 1;
    eprintln!("everspace ELF seed {seed}");
    for _ in 0..128 {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        let address = seed % 0x10000000 + 0x10000;
        symbol[8..16].copy_from_slice(&address.to_le_bytes());
        file.write_all_at(&symbol, 0x400).unwrap();
        let (found, count) = globals(file).unwrap();
        assert_eq!(count, 1);
        assert_eq!(found[0], address);
        assert_eq!(found[1..], FALLBACK[1..]);
        table[24..32].copy_from_slice(&0x60000u64.to_le_bytes());
        file.write_all_at(&table, 0x180).unwrap();
        assert!(globals(file).is_err());
        table[24..32].copy_from_slice(&0x400u64.to_le_bytes());
        file.write_all_at(&table, 0x180).unwrap();
    }
}
