// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed adapter integration; configuration contains numbers, never shell commands.
use g13pad_core::gamepad::{Click, Mapping};
use std::{
    env, fs,
    io::{Read, Write},
    os::unix::{fs::FileTypeExt, fs::OpenOptionsExt, io::AsRawFd, process::CommandExt},
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Default, PartialEq)]
struct Calibration {
    x: Option<[i32; 3]>,
    y: Option<[i32; 3]>,
    deadzone: u16,
}

impl Calibration {
    fn parse(text: &str) -> Result<Self, String> {
        let mut result = Self {
            deadzone: 6000,
            ..Self::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for line in text.lines() {
            let fields: Vec<_> = line
                .split('#')
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let Some(key) = fields.first() else { continue };
            if !seen.insert(*key) {
                return Err(format!("duplicate setting: {key}"));
            }
            match *key {
                "deadzone" if fields.len() == 2 => {
                    result.deadzone = fields[1].parse().map_err(|_| "invalid deadzone")?;
                    if result.deadzone > 32767 {
                        return Err("deadzone must be 0–32767".into());
                    }
                }
                "calibration_x" | "calibration_y" if fields.len() == 4 => {
                    let mut range = [0; 3];
                    for (value, field) in range.iter_mut().zip(&fields[1..]) {
                        *value = field.parse().map_err(|_| "invalid calibration number")?;
                    }
                    if range[0] < -32768
                        || range[2] > 32767
                        || !(range[0] < range[1] && range[1] < range[2])
                    {
                        return Err(
                            "calibration requires -32768 <= min < centre < max <= 32767".into()
                        );
                    }
                    if *key == "calibration_x" {
                        result.x = Some(range);
                    } else {
                        result.y = Some(range);
                    }
                }
                _ => return Err(format!("unknown setting or wrong field count: {line}")),
            }
        }
        if result.x.is_some() != result.y.is_some() {
            return Err("supply both calibration_x and calibration_y, or neither".into());
        }
        Ok(result)
    }

    fn read(path: &Path) -> Result<Self, String> {
        let meta = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
        if !meta.is_file() || meta.len() > 8192 {
            return Err("adapter configuration must be a regular file under 8 KiB".into());
        }
        Self::parse(&fs::read_to_string(path).map_err(|e| e.to_string())?)
    }

    fn arguments(&self, source: &str) -> Vec<String> {
        self.mapped_arguments(source, Mapping::default())
    }
    fn mapped_arguments(&self, source: &str, mapping: Mapping) -> Vec<String> {
        let mut args: Vec<String> = [
            "--evdev",
            source,
            "--evdev-no-grab",
            "--evdev-absmap",
            "ABS_X=x1,ABS_Y=y1",
            "--evdev-keymap",
            "BTN_EXTRA=TL",
            "--mimic-xpad",
            "--ui-axismap",
            "x1=ABS_X,y1^resp:32767:0:-32768=ABS_Y",
            "--device-name",
            "G13 Analog Stick",
            "--device-usbid",
            "046d:c21c:0110",
            "--silent",
            "--dbus",
            "disabled",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        args[4] = mapping.absmap();
        args[6] = format!("BTN_EXTRA={}", mapping.click.source_button());
        args[9] = mapping.axismap();
        if mapping.click == Click::None {
            args.extend(["--ui-buttonmap".into(), "TL=void".into()]);
        }
        args.extend(["--deadzone".into(), self.deadzone.to_string()]);
        if let (Some(x), Some(y)) = (self.x, self.y) {
            args.extend([
                "--calibration".into(),
                format!(
                    "{}={}:{}:{},{}={}:{}:{}",
                    mapping.axes().0,
                    x[0],
                    x[1],
                    x[2],
                    mapping.axes().1,
                    y[0],
                    y[1],
                    y[2]
                ),
            ]);
        }
        args
    }
}

extern "C" {
    fn ioctl(fd: i32, request: std::ffi::c_ulong, ...) -> i32;
    fn geteuid() -> u32;
}

fn initialize_mapping(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let flags = 0o4000 | 0o400000 | 0o2000000;
    let (mut file, created) = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o660)
        .custom_flags(flags)
        .open(path)
    {
        Ok(file) => (file, true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(flags)
                .open(path)
                .map_err(|e| e.to_string())?,
            false,
        ),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    // SAFETY: geteuid takes no arguments and returns the process's effective UID.
    if !metadata.is_file() || metadata.uid() != unsafe { geteuid() } {
        return Err("mapping must be a regular file owned by the adapter account".into());
    }
    file.try_lock().map_err(|e| e.to_string())?;
    if created {
        file.write_all(Mapping::default().to_text().as_bytes())
            .map_err(|e| e.to_string())?;
    } else {
        let mut text = String::new();
        (&mut file)
            .take(513)
            .read_to_string(&mut text)
            .map_err(|e| e.to_string())?;
        Mapping::parse(&text)?;
    }
    file.set_permissions(fs::Permissions::from_mode(0o660))
        .map_err(|e| e.to_string())
}

/// Opens the daemon's command FIFO for writing, waiting briefly for the driver to create it
/// and open it for reading. On a driver restart the adapter unit (PartOf=g13.service) starts
/// as soon as g13d execs, before it has made the FIFO, so a plain open races it: ENOENT (not
/// created yet) or ENXIO (there, no reader yet). Both are transient; anything else, or the
/// five-second deadline, is a real failure. Without this the first start after a driver
/// restart fails and only recovers on the unit's own Restart=on-failure two seconds later.
fn open_pipe(pipe: &Path) -> Result<fs::File, String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::OpenOptions::new()
            .write(true)
            .custom_flags(0o4000 | 0o400000 | 0o2000000)
            .open(pipe)
        {
            Ok(file) => return Ok(file),
            Err(e) => {
                let transient = matches!(e.raw_os_error(), Some(2) | Some(6)); // ENOENT, ENXIO
                if !transient || Instant::now() >= deadline {
                    return Err(format!("{}: {e}", pipe.display()));
                }
                thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Waits up to five seconds for a path to exist. The adapter's source is g13d's uinput
/// device, destroyed and recreated when the driver restarts; the adapter unit
/// (PartOf=g13.service) restarts in lockstep and would otherwise fail its startup guard in
/// the ~100 ms before udev re-links the node. Replaces a bare `test -e` that lost that race.
fn wait_exists(path: &Path) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("{}: did not appear within 5 s", path.display()));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

fn send(pipe: &Path, commands: &[u8]) -> Result<(), String> {
    let mut file = open_pipe(pipe)?;
    if !file
        .metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_fifo()
    {
        return Err("adapter command destination is not a FIFO".into());
    }
    let drain = |file: &fs::File| -> Result<(), String> {
        let started = Instant::now();
        loop {
            let mut pending: i32 = 0;
            // SAFETY: Linux FIONREAD writes an int on a valid FIFO descriptor.
            if unsafe { ioctl(file.as_raw_fd(), 0x541B, &mut pending as *mut i32) } < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if pending == 0 {
                return Ok(());
            }
            if pending < 0 || started.elapsed() >= Duration::from_secs(2) {
                return Err("driver FIFO did not drain".into());
            }
            thread::sleep(Duration::from_millis(5));
        }
    };
    drain(&file)?;
    match file.write(commands) {
        Ok(n) if n == commands.len() => drain(&file),
        Ok(_) => Err("incomplete adapter FIFO write".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version"] => {
            println!("g13pad-analog {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["prepare", pipe] => send(Path::new(pipe), b"stickmode ABSOLUTE\nbind TOP MEXTRA\n"),
        ["await", path] => wait_exists(Path::new(path)),
        ["init-map", path] => initialize_mapping(Path::new(path)),
        ["release", pipe] => send(Path::new(pipe), b"stickmode KEYS\n"),
        ["check", config] => {
            let settings = Calibration::read(Path::new(config))?;
            println!(
                "xboxdrv {:?}",
                settings.arguments("/dev/input/g13-analog-source")
            );
            Ok(())
        }
        ["run", config, source] => {
            let settings = Calibration::read(Path::new(config))?;
            Err(Command::new("/usr/bin/xboxdrv")
                .args(settings.arguments(source))
                .exec()
                .to_string())
        }
        ["check", config, map] => {
            println!("xboxdrv {:?}", Calibration::read(Path::new(config))?
                .mapped_arguments("/dev/input/g13-analog-source", Mapping::read(Path::new(map))?));
            Ok(())
        }
        ["run", config, source, map] => {
            let settings = Calibration::read(Path::new(config))?;
            let mapping = Mapping::read(Path::new(map))?;
            Err(Command::new("/usr/bin/xboxdrv").args(settings.mapped_arguments(source, mapping)).exec().to_string())
        }
        _ => {
            Err("usage: g13pad-analog prepare|release FIFO; await PATH; init-map FILE; check CONFIG [MAP]; run CONFIG SOURCE [MAP]".into())
        }
    }
}

fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("g13pad-analog: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mappings_preserve_physical_calibration_and_disable_click_explicitly() {
        use g13pad_core::gamepad::Stick;
        let calibration = Calibration::parse(
            "calibration_x -26060 2322 28897\ncalibration_y -31736 -4387 23479\n",
        )
        .unwrap();
        let mapping = Mapping {
            stick: Stick::Right,
            click: Click::None,
            swap: true,
            invert_x: true,
            invert_y: false,
        };
        let args = calibration.mapped_arguments("source", mapping);
        let value = |key| args[args.iter().position(|a| a == key).unwrap() + 1].as_str();
        assert_eq!(value("--evdev-absmap"), "ABS_X=y2,ABS_Y=x2");
        assert_eq!(
            value("--calibration"),
            "y2=-26060:2322:28897,x2=-31736:-4387:23479"
        );
        assert_eq!(
            value("--ui-axismap"),
            "y2^resp:32767:0:-32768=ABS_RY,x2=ABS_RX"
        );
        assert_eq!(value("--ui-buttonmap"), "TL=void");
    }
    #[test]
    fn init_map_preserves_custom_settings_and_rejects_links_and_fifos() {
        use std::os::unix::fs::PermissionsExt;
        let dir = env::temp_dir().join(format!("g13pad-init-map-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("map");
        initialize_mapping(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o660
        );
        let custom = Mapping {
            click: Click::B,
            ..Default::default()
        };
        fs::write(&path, custom.to_text()).unwrap();
        initialize_mapping(&path).unwrap();
        assert_eq!(Mapping::read(&path).unwrap(), custom);
        let link = dir.join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(initialize_mapping(&link).is_err());
        let fifo = dir.join("fifo");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        assert!(initialize_mapping(&fifo).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    #[ignore = "run explicitly: cargo test -p g13pad-analog mutation_fuzz -- --ignored --nocapture"]
    fn mutation_fuzz_numeric_configuration() {
        let seeds: &[&[u8]] = &[
            b"deadzone 6000\n",
            b"calibration_x -32768 0 32767\ncalibration_y -1 0 1\ndeadzone 32767\n",
            b"deadzone 0\n# harmless comment ; $(touch /tmp/never)\n",
            b"deadzone 6000\ndeadzone 1\n",
            b"calibration_x -2147483649 0 2147483648\ncalibration_y -1 0 1\n",
            b"deadzone 6000; touch /tmp/never\n",
            b"calibration_x -1 0 1\n",
        ];
        let mut state = 0x13_2026_0930_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as usize
        };
        let mut accepted = 0;
        for case in 0..100_000 {
            let mut bytes = seeds[case % seeds.len()].to_vec();
            for _ in 0..1 + next() % 16 {
                let index = next() % (bytes.len() + 1);
                match next() % 4 {
                    0 if index < bytes.len() => {
                        bytes.remove(index);
                    }
                    1 if index < bytes.len() => bytes[index] ^= 1 << (next() % 8),
                    2 => bytes.truncate(index),
                    _ => bytes.insert(index, next() as u8),
                }
            }
            let text = String::from_utf8_lossy(&bytes);
            if let Ok(config) = Calibration::parse(&text) {
                accepted += 1;
                assert!(config.deadzone <= 32767, "case {case}: {text:?}");
                assert_eq!(config.x.is_some(), config.y.is_some());
                let mut canonical = format!("deadzone {}\n", config.deadzone);
                for (key, range) in [("calibration_x", config.x), ("calibration_y", config.y)] {
                    if let Some([min, centre, max]) = range {
                        assert!(-32768 <= min && min < centre && centre < max && max <= 32767);
                        canonical.push_str(&format!("{key} {min} {centre} {max}\n"));
                    }
                }
                assert_eq!(Calibration::parse(&canonical).unwrap(), config);
                let args = config.arguments("source");
                assert!(args
                    .iter()
                    .all(|arg| !arg.contains(['\n', '\r', '\0', ';', '$'])));
            }
        }
        assert!(accepted > 1000);
        for _ in 0..10_000 {
            let x = next() % 65534;
            let y = next() % 65534;
            let deadzone = next() % 32768;
            let text = format!(
                "calibration_x -32768 {} 32767\ncalibration_y -32768 {} 32767\ndeadzone {deadzone}\n",
                x as i32 - 32767, y as i32 - 32767
            );
            assert!(
                Calibration::parse(&text).is_ok(),
                "valid generated configuration: {text}"
            );
            assert!(Calibration::parse(&format!("{text}deadzone {deadzone}\n")).is_err());
            assert!(Calibration::parse(&format!("{text}command injected\n")).is_err());
        }
        println!("100,000 adapter mutations ({accepted} accepted), 10,000 valid configurations and 20,000 invalid extensions");
    }
    #[test]
    fn await_waits_for_a_path_then_bounds_its_wait() {
        let root = env::temp_dir().join(format!("g13pad-await-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let node = root.join("source");
        let node2 = node.clone();
        let maker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            fs::write(&node2, b"").unwrap();
        });
        wait_exists(&node).unwrap();
        maker.join().unwrap();
        let started = Instant::now();
        assert!(wait_exists(&root.join("never")).is_err());
        assert!(
            started.elapsed() >= Duration::from_secs(5)
                && started.elapsed() < Duration::from_secs(7)
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn send_waits_for_a_fifo_the_driver_has_not_made_yet() {
        use std::io::Read;
        use std::sync::mpsc;
        let root = env::temp_dir().join(format!("g13pad-late-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let pipe = root.join("pipe");
        // The FIFO appears 150 ms after send() is called, as on a driver restart.
        let (reading, ready) = mpsc::channel();
        let reader_path = pipe.clone();
        let reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(150));
            assert!(Command::new("mkfifo")
                .arg(&reader_path)
                .status()
                .unwrap()
                .success());
            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&reader_path)
                .unwrap();
            reading.send(()).unwrap();
            let mut bytes = vec![0; 6];
            file.read_exact(&mut bytes).unwrap();
            bytes
        });
        send(&pipe, b"mod 1\n").unwrap();
        ready.recv().unwrap();
        assert_eq!(reader.join().unwrap(), b"mod 1\n");
        // A path that never becomes a FIFO still fails, within the deadline.
        let gone = root.join("never");
        let started = Instant::now();
        assert!(send(&gone, b"mod 1\n").is_err());
        assert!(started.elapsed() < Duration::from_secs(7));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sends_complete_commands_and_refuses_other_file_types() {
        use std::io::Read;
        let root = env::temp_dir().join(format!("g13pad-adapter-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let pipe = root.join("pipe");
        let target = root.join("target");
        fs::write(&target, b"preserved").unwrap();
        assert!(send(&target, b"overwrite").is_err());
        std::os::unix::fs::symlink(&target, &pipe).unwrap();
        assert!(send(&pipe, b"overwrite").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"preserved");
        fs::remove_file(&pipe).unwrap();
        assert!(Command::new("mkfifo")
            .arg(&pipe)
            .status()
            .unwrap()
            .success());
        let commands = b"stickmode ABSOLUTE\nbind TOP MEXTRA\n";
        let (ready, waiting) = std::sync::mpsc::channel();
        let reader_path = pipe.clone();
        let reader = thread::spawn(move || {
            let mut file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(reader_path)
                .unwrap();
            ready.send(()).unwrap();
            let mut bytes = vec![0; commands.len()];
            file.read_exact(&mut bytes).unwrap();
            bytes
        });
        waiting.recv().unwrap();
        send(&pipe, commands).unwrap();
        assert_eq!(reader.join().unwrap(), commands);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn measured_config_preserves_the_working_adapter_arguments() {
        let config = Calibration::parse(
            "calibration_x -26060 2322 28897\ncalibration_y -31736 -4387 23479\ndeadzone 6000\n",
        )
        .unwrap();
        let args = config.arguments("/dev/input/test-source");
        assert!(args.windows(2).any(|pair| pair
            == [
                "--calibration",
                "x1=-26060:2322:28897,y1=-31736:-4387:23479"
            ]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--device-usbid", "046d:c21c:0110"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["--evdev-keymap", "BTN_EXTRA=TL"]));
        assert!(args.iter().any(|arg| arg == "--evdev-no-grab"));
    }
    #[test]
    fn rejects_invalid_or_command_bearing_configuration() {
        for text in [
            "deadzone -1",
            "deadzone 65536",
            "deadzone 32768",
            "deadzone 1\ndeadzone 2",
            "calibration_x 0 0 1\ncalibration_y -1 0 1",
            "calibration_x -32769 0 1\ncalibration_y -1 0 1",
            "calibration_x -1 0 1",
            "calibration_x -1 0 2147483648",
            "deadzone 1; touch /tmp/file",
            "command anything",
        ] {
            assert!(Calibration::parse(text).is_err(), "{text}");
        }
        assert!(
            !Calibration::parse(include_str!("../../../packaging/analog.conf"))
                .unwrap()
                .arguments("source")
                .iter()
                .any(|arg| arg == "--calibration")
        );
    }
}
