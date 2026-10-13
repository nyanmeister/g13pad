// SPDX-License-Identifier: GPL-3.0-or-later
//! Linux x86_64 native evdev boundary check. Run only in the isolated test VM.
//! rustc --edition=2021 tools/controller-proof.rs -o /tmp/controller-proof
//! sudo /tmp/controller-proof /path/to/g13pad-analog /path/to/output
use std::{
    env, fs,
    io::{self, Write},
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Event {
    time: [i64; 2],
    kind: u16,
    code: u16,
    value: i32,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct Abs {
    value: i32,
    min: i32,
    max: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}
#[repr(C)]
struct Device {
    name: [u8; 80],
    id: [u16; 4],
    effects: u32,
    max: [i32; 64],
    min: [i32; 64],
    fuzz: [i32; 64],
    flat: [i32; 64],
}
unsafe extern "C" {
    fn ioctl(fd: i32, request: usize, ...) -> i32;
}
fn ioct(fd: i32, request: usize, arg: usize) -> io::Result<()> {
    // SAFETY: callers supply a value or a valid appropriately sized C buffer.
    if unsafe { ioctl(fd, request, arg) } < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn bytes<T>(value: &T) -> &[u8] {
    // SAFETY: only fully initialized repr(C) structs without padding are used.
    unsafe { std::slice::from_raw_parts((value as *const T).cast(), size_of::<T>()) }
}
fn send(source: &mut fs::File, events: &[(u16, u16, i32)]) -> io::Result<()> {
    for &(kind, code, value) in events {
        source.write_all(bytes(&Event {
            kind,
            code,
            value,
            ..Default::default()
        }))?;
    }
    source.write_all(bytes(&Event::default()))
}
fn abs(file: &fs::File, code: u16) -> io::Result<i32> {
    let mut value = Abs::default();
    ioct(
        file.as_raw_fd(),
        0x80184540 + usize::from(code),
        (&mut value as *mut Abs) as usize,
    )?;
    Ok(value.value)
}
fn keys(file: &fs::File) -> io::Result<[u8; 96]> {
    let mut value = [0u8; 96];
    ioct(file.as_raw_fd(), 0x80604518, value.as_mut_ptr() as usize)?;
    Ok(value)
}
fn bit(bits: &[u8], code: u16) -> bool {
    bits[usize::from(code) / 8] & (1 << (code % 8)) != 0
}
fn nodes() -> io::Result<Vec<(String, PathBuf)>> {
    let mut result = Vec::new();
    for entry in fs::read_dir("/sys/class/input")? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with("event") {
            continue;
        }
        if let Ok(name) = fs::read_to_string(entry.path().join("device/name")) {
            result.push((
                name.trim().into(),
                Path::new("/dev/input").join(entry.file_name()),
            ));
        }
    }
    Ok(result)
}
struct Adapter(Child);
impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn await_output(before: &[(String, PathBuf)], adapter: &mut Adapter) -> io::Result<PathBuf> {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = adapter.0.try_wait()? {
            return Err(io::Error::other(format!("adapter exited {status}")));
        }
        for (name, path) in nodes()? {
            if name == "G13 Analog Stick" && !before.iter().any(|(_, old)| old == &path) {
                return Ok(path);
            }
        }
        if Instant::now() > end {
            return Err(io::Error::other("controller did not appear"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}
fn check(
    file: &fs::File,
    expected_key: Option<u16>,
    expected_abs: Option<(u16, i32)>,
) -> io::Result<()> {
    let end = Instant::now() + Duration::from_secs(2);
    loop {
        let held = keys(file)?;
        let key_ok = (0..768).all(|code| bit(&held, code) == (expected_key == Some(code)));
        let axes_ok = [0, 1, 2, 3, 4, 5, 16, 17].into_iter().all(|code| {
            abs(file, code)
                .is_ok_and(|value| value == expected_abs.filter(|p| p.0 == code).map_or(0, |p| p.1))
        });
        if key_ok && axes_ok {
            return Ok(());
        }
        if Instant::now() > end {
            return Err(io::Error::other(format!(
                "unexpected controller state, expected key={expected_key:?} axis={expected_abs:?}, axes={:?}, keys={held:?}",
                [0,1,2,3,4,5,16,17].map(|code| (code,abs(file,code)))
            )));
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn run(adapter_binary: &str, output: &Path) -> io::Result<()> {
    fs::create_dir_all(output)?;
    let mut source = fs::OpenOptions::new().write(true).open("/dev/uinput")?;
    let mut device = Device {
        name: [0; 80],
        id: [3, 0x1234, 0x5678, 1],
        effects: 0,
        max: [0; 64],
        min: [0; 64],
        fuzz: [0; 64],
        flat: [0; 64],
    };
    let name = format!("G13 controller proof {}", std::process::id());
    device.name[..name.len()].copy_from_slice(name.as_bytes());
    device.max[0] = 255;
    device.max[1] = 255;
    ioct(source.as_raw_fd(), 0x40045564, 1)?; // UI_SET_EVBIT EV_KEY
    ioct(source.as_raw_fd(), 0x40045564, 3)?; // EV_ABS
    for code in 0..2 {
        ioct(source.as_raw_fd(), 0x40045567, code)?;
    }
    for code in 704..721 {
        ioct(source.as_raw_fd(), 0x40045565, code)?;
    }
    source.write_all(bytes(&device))?;
    ioct(source.as_raw_fd(), 0x5501, 0)?;
    let end = Instant::now() + Duration::from_secs(3);
    let source_path = loop {
        if let Some((_, path)) = nodes()?.into_iter().find(|(n, _)| n == &name) {
            break path;
        }
        if Instant::now() > end {
            return Err(io::Error::other("source did not appear"));
        }
        thread::sleep(Duration::from_millis(10));
    };
    send(&mut source, &[(3, 0, 127), (3, 1, 127)])?;
    let config = output.join("analog.conf");
    // The 256-step source has two central samples. Use the normal dead zone.
    fs::write(&config, "deadzone 6000\n")?;
    let mapping = output.join("analog.map");
    fs::write(
        &mapping,
        "stick left\nclick l3\nswap 0\ninvert_x 0\ninvert_y 1\n",
    )?;
    let before = nodes()?;
    let log = fs::File::create(output.join("adapter.log"))?;
    let mut adapter = Adapter(
        Command::new(adapter_binary)
            .args(["run"])
            .arg(&config)
            .arg(&source_path)
            .arg(&mapping)
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()?,
    );
    let path = await_output(&before, &mut adapter)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&path)?;
    // Ensure source values are delivered after the adapter starts listening.
    send(&mut source, &[(3, 0, 128), (3, 1, 128)])?;
    send(&mut source, &[(3, 0, 127), (3, 1, 127)])?;
    check(&file, None, None)?;
    let expected = [304, 305, 308, 307, 310, 311, 314, 315, 316, 317, 318];
    for (i, key) in expected.into_iter().enumerate() {
        send(&mut source, &[(1, 704 + i as u16, 1)])?;
        check(&file, Some(key), None)?;
        send(&mut source, &[(1, 704 + i as u16, 0)])?;
        check(&file, None, None)?;
    }
    for (i, axis, value) in [
        (11, 2, 255),
        (12, 5, 255),
        (13, 17, -1),
        (14, 17, 1),
        (15, 16, -1),
        (16, 16, 1),
    ] {
        send(&mut source, &[(1, 704 + i, 1)])?;
        check(&file, None, Some((axis, value)))?;
        send(&mut source, &[(1, 704 + i, 0)])?;
        check(&file, None, None)?;
    }
    send(&mut source, &[(1, 717, 1), (1, 718, 1)])?;
    check(&file, None, None)?;
    send(&mut source, &[(1, 718, 0)])?;
    check(&file, None, Some((17, -1)))?;
    send(&mut source, &[(1, 719, 1)])?;
    let end = Instant::now() + Duration::from_secs(2);
    while abs(&file, 16)? != -1 || abs(&file, 17)? != -1 {
        if Instant::now() > end {
            return Err(io::Error::other("diagonal failed"));
        }
        thread::sleep(Duration::from_millis(10));
    }
    send(&mut source, &[(1, 717, 0), (1, 719, 0)])?;
    check(&file, None, None)?;
    send(&mut source, &[(3, 0, 255)])?;
    check(&file, None, Some((0, 32767)))?;
    send(&mut source, &[(3, 0, 127)])?;
    check(&file, None, None)?;
    let mut capabilities = [0u8; 96];
    ioct(
        file.as_raw_fd(),
        0x80604521,
        capabilities.as_mut_ptr() as usize,
    )?;
    assert!(
        (1..256).all(|code| !bit(&capabilities, code)),
        "controller exposes keyboard keys"
    );
    drop(file);
    drop(adapter);
    assert!(!path.exists(), "controller remained after adapter exit");
    drop(source);
    assert!(!source_path.exists(), "test source remained after close");
    fs::write(output.join("PASS.txt"),"All 17 controller channels, neutral startup/release, full triggers, opposite and diagonal D-pad, continuous stick endpoint and no keyboard capabilities passed. Both private devices removed.\n")?;
    Ok(())
}
fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    if args == ["--version"] {
        println!("g13pad-controller-proof 0.2.78");
        return;
    }
    // Like check-vm-application.sh: refuse to run anywhere but the authorized isolated test
    // VM, named by G13PAD_VM_HOST. Real USB and a driver restart are involved, so this must
    // never run on a developer's or user's own machine.
    let vm_host = env::var("G13PAD_VM_HOST").unwrap_or_default();
    if env::consts::ARCH != "x86_64"
        || vm_host.is_empty()
        || !fs::read_to_string("/etc/hostname").is_ok_and(|n| n.trim() == vm_host)
    {
        eprintln!(
            "run only on the authorized isolated x86_64 test VM: set G13PAD_VM_HOST to its hostname"
        );
        std::process::exit(2);
    }
    if let [binary, output] = args.as_slice() {
        if let Err(e) = run(binary, Path::new(output)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
    } else {
        eprintln!("usage: controller-proof ADAPTER OUTPUT_DIRECTORY");
        std::process::exit(2);
    }
}
