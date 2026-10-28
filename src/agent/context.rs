// SPDX-License-Identifier: GPL-3.0-or-later
//! Best-effort Codex usage metadata. The rollout is not a stable hook API:
//! unknown shapes yield no gauge. Never use cumulative totals or rate limits.
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const MAX_TAIL: u64 = 1024 * 1024;

pub(super) fn read(path: &Path) -> Option<u8> {
    let mut file = File::open(path).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() {
        return None;
    }
    let end = meta.len();
    let mut size = 8192u64.min(end);
    loop {
        let start = end.saturating_sub(size);
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = Vec::with_capacity(size as usize);
        (&mut file).take(size).read_to_end(&mut bytes).ok()?;
        // The first line may begin outside this window. Only parse complete records.
        let bytes = if start == 0 {
            bytes.as_slice()
        } else {
            bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(&[][..], |at| &bytes[at + 1..])
        };
        for line in bytes.rsplit(|b| *b == b'\n') {
            // Avoid parsing tool results, prompts or other large content records.
            if !line.windows(11).any(|w| w == b"token_count")
                && !line.windows(9).any(|w| w == b"compacted")
            {
                continue;
            }
            let Ok(v) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            if v["type"] == "compacted" {
                return None;
            }
            if v["type"] != "event_msg" || v["payload"]["type"] != "token_count" {
                continue;
            }
            let info = &v["payload"]["info"];
            if info.is_null() {
                continue;
            }
            let used = info["last_token_usage"]["total_tokens"].as_u64()?;
            let window = info["model_context_window"].as_u64()?;
            if window == 0 {
                return None;
            }
            return Some(
                ((u128::from(used.min(window)) * 100 + u128::from(window) / 2) / u128::from(window))
                    as u8,
            );
        }
        if start == 0 || size >= MAX_TAIL {
            return None;
        }
        size = (size * 2).min(MAX_TAIL).min(end);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Sandbox;
    use serde_json::json;

    fn usage(used: u64, window: u64) -> String {
        format!(
            "{}\n",
            json!({"type":"event_msg", "payload":{
            "type":"token_count", "info":{
                "last_token_usage":{"total_tokens":used,"cached_input_tokens":19},
                "total_token_usage":{"total_tokens":999999},
                "model_context_window":window
            }}})
        )
    }

    #[test]
    fn latest_usage_is_not_cumulative_and_compaction_resets_it() {
        let _s = Sandbox::new("codex-context-metadata");
        std::fs::create_dir_all(crate::config_dir()).unwrap();
        let path = crate::config_dir().join("rollout.jsonl");
        let write = |s: &str| std::fs::write(&path, s).unwrap();
        write(&(usage(900, 1000) + &usage(125, 1000)));
        assert_eq!(read(&path), Some(13));
        write(&(usage(125, 1000) + "{\"type\":\"compacted\"}\n"));
        assert_eq!(read(&path), None);
        write(&(usage(125, 1000) + "{\"type\":\"compacted\"}\n" + &usage(40, 1000)));
        assert_eq!(read(&path), Some(4));
        write(&usage(5, 0));
        assert_eq!(read(&path), None);
        write(&usage(u64::MAX, 1000));
        assert_eq!(read(&path), Some(100));
        // A null-info rate-limit update is not a new usage measurement.
        write(&(usage(250, 1000) + "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":null}}\n"));
        assert_eq!(read(&path), Some(25));
    }

    #[test]
    fn varied_valid_readings_ignore_cumulative_and_non_event_records() {
        let _s = Sandbox::new("codex-context-varied");
        std::fs::create_dir_all(crate::config_dir()).unwrap();
        let path = crate::config_dir().join("rollout.jsonl");
        let mut seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as u64;
        for _ in 0..256 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let used = seed % 1001;
            let text = usage(900, 1000) + &usage(used, 1000)
                + "{\"type\":\"response_item\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"total_tokens\":999},\"model_context_window\":1000}}}\n";
            std::fs::write(&path, text).unwrap();
            assert_eq!(read(&path), Some(((used + 5) / 10) as u8), "seed{seed}");
        }
    }

    #[test]
    fn tail_crosses_long_records_but_has_a_finite_read_budget() {
        let _s = Sandbox::new("codex-context-tail");
        std::fs::create_dir_all(crate::config_dir()).unwrap();
        let path = crate::config_dir().join("rollout.jsonl");
        let long_record = format!(
            "{{\"type\":\"response_item\",\"text\":\"{}\"}}\n",
            "x".repeat(30_000)
        );
        std::fs::write(&path, usage(625, 1000) + &long_record).unwrap();
        assert_eq!(read(&path), Some(63));
        std::fs::write(&path, usage(625, 1000) + &"x".repeat(MAX_TAIL as usize + 1)).unwrap();
        assert_eq!(read(&path), None);
    }
}
