// SPDX-License-Identifier: GPL-3.0-or-later
//! Key-free emulator achievement logs, progress and FIFO notification cards.
use crate::{draw::Draw, lcd::Bitmap, meter::text};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_LINE: usize = 4096;
const BATCH: u64 = 65536;
const MAX_QUEUE: usize = 10000;
const DOLPHIN_HISTORY: u64 = 4 * 1024 * 1024;

mod adapters;
mod bn2;
mod core_memory;
mod fzero;

pub fn bn2_command(args: &[&str]) -> Result<String, String> {
    match args {
        ["launch", rest @ ..] => launch(rest),
        _ => bn2::command(args),
    }
}

pub fn fzero_command(args: &[&str]) -> Result<String, String> {
    fzero::command(args)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Source {
    RetroArch,
    Dolphin,
}
impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::RetroArch => "RetroArch",
            Self::Dolphin => "Dolphin",
        }
    }
    fn comm(self) -> &'static str {
        match self {
            Self::RetroArch => "retroarch\n",
            Self::Dolphin => "dolphin-emu\n",
        }
    }
}

mod achievements;
use achievements::*;
mod transport;
use transport::*;
mod display;
use display::*;
mod runtime;

pub(crate) use display::preview;
pub(crate) use runtime::run;
pub use transport::launch;
#[cfg(test)]
mod tests;
