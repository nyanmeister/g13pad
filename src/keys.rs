// SPDX-License-Identifier: GPL-3.0-or-later
// Linux key codes the daemon can emit (uinput keybits 0..255), first name per code,
// from /usr/include/linux/input-event-codes.h on 2026-09-29. `KEY_` prefix stripped.
pub const KEYS: &[(u16, &str)] = &[
    (1, "ESC"),
    (2, "1"),
    (3, "2"),
    (4, "3"),
    (5, "4"),
    (6, "5"),
    (7, "6"),
    (8, "7"),
    (9, "8"),
    (10, "9"),
    (11, "0"),
    (12, "MINUS"),
    (13, "EQUAL"),
    (14, "BACKSPACE"),
    (15, "TAB"),
    (16, "Q"),
    (17, "W"),
    (18, "E"),
    (19, "R"),
    (20, "T"),
    (21, "Y"),
    (22, "U"),
    (23, "I"),
    (24, "O"),
    (25, "P"),
    (26, "LEFTBRACE"),
    (27, "RIGHTBRACE"),
    (28, "ENTER"),
    (29, "LEFTCTRL"),
    (30, "A"),
    (31, "S"),
    (32, "D"),
    (33, "F"),
    (34, "G"),
    (35, "H"),
    (36, "J"),
    (37, "K"),
    (38, "L"),
    (39, "SEMICOLON"),
    (40, "APOSTROPHE"),
    (41, "GRAVE"),
    (42, "LEFTSHIFT"),
    (43, "BACKSLASH"),
    (44, "Z"),
    (45, "X"),
    (46, "C"),
    (47, "V"),
    (48, "B"),
    (49, "N"),
    (50, "M"),
    (51, "COMMA"),
    (52, "DOT"),
    (53, "SLASH"),
    (54, "RIGHTSHIFT"),
    (55, "KPASTERISK"),
    (56, "LEFTALT"),
    (57, "SPACE"),
    (58, "CAPSLOCK"),
    (59, "F1"),
    (60, "F2"),
    (61, "F3"),
    (62, "F4"),
    (63, "F5"),
    (64, "F6"),
    (65, "F7"),
    (66, "F8"),
    (67, "F9"),
    (68, "F10"),
    (69, "NUMLOCK"),
    (70, "SCROLLLOCK"),
    (71, "KP7"),
    (72, "KP8"),
    (73, "KP9"),
    (74, "KPMINUS"),
    (75, "KP4"),
    (76, "KP5"),
    (77, "KP6"),
    (78, "KPPLUS"),
    (79, "KP1"),
    (80, "KP2"),
    (81, "KP3"),
    (82, "KP0"),
    (83, "KPDOT"),
    (85, "ZENKAKUHANKAKU"),
    (86, "102ND"),
    (87, "F11"),
    (88, "F12"),
    (89, "RO"),
    (90, "KATAKANA"),
    (91, "HIRAGANA"),
    (92, "HENKAN"),
    (93, "KATAKANAHIRAGANA"),
    (94, "MUHENKAN"),
    (95, "KPJPCOMMA"),
    (96, "KPENTER"),
    (97, "RIGHTCTRL"),
    (98, "KPSLASH"),
    (99, "SYSRQ"),
    (100, "RIGHTALT"),
    (101, "LINEFEED"),
    (102, "HOME"),
    (103, "UP"),
    (104, "PAGEUP"),
    (105, "LEFT"),
    (106, "RIGHT"),
    (107, "END"),
    (108, "DOWN"),
    (109, "PAGEDOWN"),
    (110, "INSERT"),
    (111, "DELETE"),
    (112, "MACRO"),
    (113, "MUTE"),
    (114, "VOLUMEDOWN"),
    (115, "VOLUMEUP"),
    (116, "POWER"),
    (117, "KPEQUAL"),
    (118, "KPPLUSMINUS"),
    (119, "PAUSE"),
    (120, "SCALE"),
    (121, "KPCOMMA"),
    (122, "HANGEUL"),
    (123, "HANJA"),
    (124, "YEN"),
    (125, "LEFTMETA"),
    (126, "RIGHTMETA"),
    (127, "COMPOSE"),
    (128, "STOP"),
    (129, "AGAIN"),
    (130, "PROPS"),
    (131, "UNDO"),
    (132, "FRONT"),
    (133, "COPY"),
    (134, "OPEN"),
    (135, "PASTE"),
    (136, "FIND"),
    (137, "CUT"),
    (138, "HELP"),
    (139, "MENU"),
    (140, "CALC"),
    (141, "SETUP"),
    (142, "SLEEP"),
    (143, "WAKEUP"),
    (144, "FILE"),
    (145, "SENDFILE"),
    (146, "DELETEFILE"),
    (147, "XFER"),
    (148, "PROG1"),
    (149, "PROG2"),
    (150, "WWW"),
    (151, "MSDOS"),
    (152, "COFFEE"),
    (153, "ROTATE_DISPLAY"),
    (154, "CYCLEWINDOWS"),
    (155, "MAIL"),
    (156, "BOOKMARKS"),
    (157, "COMPUTER"),
    (158, "BACK"),
    (159, "FORWARD"),
    (160, "CLOSECD"),
    (161, "EJECTCD"),
    (162, "EJECTCLOSECD"),
    (163, "NEXTSONG"),
    (164, "PLAYPAUSE"),
    (165, "PREVIOUSSONG"),
    (166, "STOPCD"),
    (167, "RECORD"),
    (168, "REWIND"),
    (169, "PHONE"),
    (170, "ISO"),
    (171, "CONFIG"),
    (172, "HOMEPAGE"),
    (173, "REFRESH"),
    (174, "EXIT"),
    (175, "MOVE"),
    (176, "EDIT"),
    (177, "SCROLLUP"),
    (178, "SCROLLDOWN"),
    (179, "KPLEFTPAREN"),
    (180, "KPRIGHTPAREN"),
    (181, "NEW"),
    (182, "REDO"),
    (183, "F13"),
    (184, "F14"),
    (185, "F15"),
    (186, "F16"),
    (187, "F17"),
    (188, "F18"),
    (189, "F19"),
    (190, "F20"),
    (191, "F21"),
    (192, "F22"),
    (193, "F23"),
    (194, "F24"),
    (200, "PLAYCD"),
    (201, "PAUSECD"),
    (202, "PROG3"),
    (203, "PROG4"),
    (204, "ALL_APPLICATIONS"),
    (205, "SUSPEND"),
    (206, "CLOSE"),
    (207, "PLAY"),
    (208, "FASTFORWARD"),
    (209, "BASSBOOST"),
    (210, "PRINT"),
    (211, "HP"),
    (212, "CAMERA"),
    (213, "SOUND"),
    (214, "QUESTION"),
    (215, "EMAIL"),
    (216, "CHAT"),
    (217, "SEARCH"),
    (218, "CONNECT"),
    (219, "FINANCE"),
    (220, "SPORT"),
    (221, "SHOP"),
    (222, "ALTERASE"),
    (223, "CANCEL"),
    (224, "BRIGHTNESSDOWN"),
    (225, "BRIGHTNESSUP"),
    (226, "MEDIA"),
    (227, "SWITCHVIDEOMODE"),
    (228, "KBDILLUMTOGGLE"),
    (229, "KBDILLUMDOWN"),
    (230, "KBDILLUMUP"),
    (231, "SEND"),
    (232, "REPLY"),
    (233, "FORWARDMAIL"),
    (234, "SAVE"),
    (235, "DOCUMENTS"),
    (236, "BATTERY"),
    (237, "BLUETOOTH"),
    (238, "WLAN"),
    (239, "UWB"),
    (240, "UNKNOWN"),
    (241, "VIDEO_NEXT"),
    (242, "VIDEO_PREV"),
    (243, "BRIGHTNESS_CYCLE"),
    (244, "BRIGHTNESS_AUTO"),
    (245, "DISPLAY_OFF"),
    (246, "WWAN"),
    (247, "RFKILL"),
    (248, "MICMUTE"),
];

use std::collections::HashMap;

/// Short label for a Linux key name (without `KEY_`): "LEFTCTRL" -> "Ctrl", "KP7" -> "Num 7".
pub fn label(name: &str) -> String {
    let fixed = match name {
        "LEFTCTRL" => "Ctrl",
        "RIGHTCTRL" => "RCtrl",
        "LEFTSHIFT" => "Shift",
        "RIGHTSHIFT" => "RShift",
        "LEFTALT" => "Alt",
        "RIGHTALT" => "AltGr",
        "LEFTMETA" => "Super",
        "RIGHTMETA" => "RSuper",
        "ESC" => "Esc",
        "GRAVE" => "`",
        "MINUS" => "-",
        "EQUAL" => "=",
        "LEFTBRACE" => "[",
        "RIGHTBRACE" => "]",
        "SEMICOLON" => ";",
        "APOSTROPHE" => "'",
        "BACKSLASH" => "\\",
        "COMMA" => ",",
        "DOT" => ".",
        "SLASH" => "/",
        "102ND" => "<",
        "KPASTERISK" => "Num *",
        "KPMINUS" => "Num -",
        "KPPLUS" => "Num +",
        "KPDOT" => "Num .",
        "KPSLASH" => "Num /",
        "KPENTER" => "Num Enter",
        "RESERVED" => "—",
        _ => "",
    };
    if !fixed.is_empty() {
        return fixed.to_string();
    }
    if let Some(n) = name.strip_prefix("KP") {
        if n.chars().all(|c| c.is_ascii_digit()) {
            return format!("Num {n}");
        }
    }
    if name.len() <= 3 || name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit()) {
        return name.to_string();
    }
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        s.push(if i == 0 { c } else { c.to_ascii_lowercase() });
    }
    s
}

/// Label for a whole daemon action: chords, release actions, pipe-out and commands.
pub fn action_label(action: &str, layout: &HashMap<String, String>) -> String {
    if action.is_empty() {
        return "—".into();
    }
    if let Some(t) = action.strip_prefix('>') {
        return format!("pipe: {t}");
    }
    if let Some(t) = action.strip_prefix('!') {
        return format!("! {t}");
    }
    let one = |s: &str| -> String {
        s.split('+')
            .map(|k| {
                let (rel, k) = match k.strip_prefix('-') {
                    Some(k) => ("release ", k),
                    None => ("", k),
                };
                match k.strip_prefix("KEY_") {
                    Some(n) => format!("{rel}{}", layout_label(n, layout)),
                    None => match k.strip_prefix('M') {
                        Some(b) if k != "M" => format!("{rel}Mouse {}", b.to_lowercase()),
                        _ => format!("{rel}{k}"),
                    },
                }
            })
            .collect::<Vec<_>>()
            .join("+")
    };
    let mut parts = action.splitn(2, ' ');
    let down = one(parts.next().unwrap_or(""));
    match parts.next() {
        Some(up) => format!("{down}, on release {}", one(up.trim())),
        None => down,
    }
}

/// Display the active layout's character while retaining the physical Linux key identity.
pub fn layout_label(name: &str, layout: &HashMap<String, String>) -> String {
    layout
        .get(name)
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| label(name))
}

pub fn matches_filter(name: &str, layout: &HashMap<String, String>, filter: &str) -> bool {
    let filter = filter.to_uppercase();
    filter.is_empty()
        || format!("KEY_{name}").contains(&filter)
        || layout_label(name, layout).contains(&filter)
}

#[repr(C)]
#[derive(Default)]
#[allow(dead_code)] // the C layout, read only for its group
/// XkbStateRec from <X11/extensions/XKBstr.h>, field for field: 18 bytes. XkbGetState writes
/// all of it, so a shorter mirror is a stack overwrite (the earlier one was 16 bytes).
struct XkbState {
    group: u8,
    locked_group: u8,
    base_group: u16,
    latched_group: u16,
    mods: u8,
    base_mods: u8,
    latched_mods: u8,
    locked_mods: u8,
    compat_state: u8,
    grab_mods: u8,
    compat_grab_mods: u8,
    lookup_mods: u8,
    compat_lookup_mods: u8,
    pointer_buttons: u16,
}
#[link(name = "X11")]
extern "C" {
    fn XInitThreads() -> i32;
    fn XOpenDisplay(name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
    fn XCloseDisplay(display: *mut std::ffi::c_void) -> i32;
    fn XkbGetState(display: *mut std::ffi::c_void, device: u32, state: *mut XkbState) -> i32;
    fn XkbKeycodeToKeysym(
        display: *mut std::ffi::c_void,
        keycode: u8,
        group: i32,
        level: i32,
    ) -> std::ffi::c_ulong;
}
#[link(name = "xkbcommon")]
extern "C" {
    fn xkb_keysym_to_utf32(keysym: u32) -> u32;
    fn xkb_keysym_get_name(keysym: u32, buffer: *mut std::ffi::c_char, size: usize) -> i32;
}

fn keysym_label(sym: u32) -> Option<String> {
    // SAFETY: These xkbcommon calls have no display state; the name buffer is bounded.
    unsafe {
        if let Some(c) = char::from_u32(xkb_keysym_to_utf32(sym))
            .filter(|c| !c.is_control() && !c.is_whitespace())
        {
            return Some(c.to_string());
        }
        let mut buffer = [0u8; 64];
        let len = xkb_keysym_get_name(sym, buffer.as_mut_ptr().cast(), buffer.len());
        if len <= 0 || len as usize >= buffer.len() {
            return None;
        }
        let name = std::str::from_utf8(&buffer[..len as usize]).ok()?;
        name.strip_prefix("dead_")
            .map(|name| format!("Dead {}", name.replace('_', " ")))
    }
}

/// Read unshifted characters in the current XKB group. No window, input or focus changes.
/// X keycode = Linux code + 8. No X display yields explicit physical-key labels.
pub fn layout_map() -> HashMap<String, String> {
    let mut map = HashMap::new();
    // SAFETY: Xlib is initialized for threading before opening a private connection.
    // XkbState has the 16-byte C layout from XKBstr.h. Every handle is closed here.
    unsafe {
        if XInitThreads() == 0 {
            return map;
        }
        let display = XOpenDisplay(std::ptr::null());
        if display.is_null() {
            return map;
        }
        let mut state = XkbState::default();
        if XkbGetState(display, 0x100, &mut state) == 0 {
            for &(code, name) in KEYS {
                let Ok(keycode) = u8::try_from(code + 8) else {
                    continue;
                };
                // Keypad and named controls keep their useful labels (Space, Num 7, ...).
                if name.starts_with("KP") || name == "SPACE" {
                    continue;
                }
                let sym = XkbKeycodeToKeysym(display, keycode, state.group as i32, 0);
                if let Some(label) = keysym_label(sym as u32) {
                    map.insert(name.to_string(), label);
                }
            }
        }
        XCloseDisplay(display);
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dead_key_and_unicode_labels_are_explicit() {
        assert_eq!(keysym_label(0xfe51).as_deref(), Some("Dead acute"));
        assert_eq!(keysym_label(0xfe52).as_deref(), Some("Dead circumflex"));
        assert_eq!(keysym_label(0x010005d2).as_deref(), Some("ג"));
        assert_eq!(keysym_label(0xff1b), None);
        assert_eq!(keysym_label(0), None);
    }
    #[test]
    fn labels() {
        let action_label = |s| super::action_label(s, &HashMap::new());
        assert_eq!(label("LEFTCTRL"), "Ctrl");
        assert_eq!(label("KP7"), "Num 7");
        assert_eq!(label("F12"), "F12");
        assert_eq!(label("PAGEDOWN"), "Pagedown");
        assert_eq!(label("A"), "A");
        assert_eq!(action_label("KEY_LEFTCTRL+KEY_X"), "Ctrl+X");
        assert_eq!(action_label("KEY_A KEY_B"), "A, on release B");
        assert_eq!(action_label("MEXTRA"), "Mouse extra");
        assert_eq!(action_label(">hi there"), "pipe: hi there");
        assert_eq!(action_label("KEY_RESERVED"), "—");
    }
    #[test]
    fn norman_labels_change_display_without_changing_physical_identity() {
        let layout = HashMap::from([
            ("D".into(), "e".into()),
            ("E".into(), "d".into()),
            ("LEFTBRACE".into(), "ü".into()),
        ]);
        assert_eq!(layout_label("D", &layout), "E");
        assert_eq!(layout_label("E", &layout), "D");
        assert_eq!(layout_label("LEFTBRACE", &layout), "Ü");
        assert!(matches_filter("D", &layout, "KEY_D"));
        assert!(matches_filter("D", &layout, "e"));
        let cyrillic = HashMap::from([("D".into(), "в".into())]);
        assert!(matches_filter("D", &cyrillic, "В"));
        assert!(matches_filter("D", &cyrillic, "в"));
        assert!(!matches_filter("D", &cyrillic, "Г"));
        assert_eq!(
            action_label("KEY_LEFTCTRL+KEY_D KEY_E", &layout),
            "Ctrl+E, on release D"
        );
        assert_eq!(action_label("!bind G1 KEY_D", &layout), "! bind G1 KEY_D");
        assert_eq!(crate::profile::chord_action(&[], Some("D")), "KEY_D");
    }
    #[test]
    fn table_is_sorted_and_unique() {
        assert!(KEYS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(KEYS.iter().any(|&(_, n)| n == "ENTER"));
    }
}
