//! One bounded procfs read per existing monitor sample. Missing or malformed
//! `MemAvailable` is unavailable, not a guess based on free pages alone.
use redunar_core::MemorySnapshot;
use std::{fs::File, io::Read, path::Path};

pub(crate) fn read_memory(path: &Path) -> Option<MemorySnapshot> {
    let mut text = String::new();
    File::open(path)
        .ok()?
        .take(65_537)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() > 65_536 {
        return None;
    }
    parse_memory(&text)
}

fn parse_memory(text: &str) -> Option<MemorySnapshot> {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let slot = match fields.next()? {
            "MemTotal:" => &mut total,
            "MemAvailable:" => &mut available,
            _ => continue,
        };
        if slot.is_some() {
            return None;
        }
        let value = fields.next()?.parse::<u64>().ok()?.checked_mul(1024)?;
        if fields.next()? != "kB" || fields.next().is_some() {
            return None;
        }
        *slot = Some(value);
    }
    let total_bytes = total.filter(|value| *value > 0)?;
    Some(MemorySnapshot {
        total_bytes,
        used_bytes: total_bytes.checked_sub(available?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_used_excluding_available_cache() {
        assert_eq!(
            parse_memory("MemTotal: 1000 kB\nMemFree: 10 kB\nMemAvailable: 400 kB\n"),
            Some(MemorySnapshot {
                total_bytes: 1_024_000,
                used_bytes: 614_400
            })
        );
    }
    #[test]
    fn rejects_missing_malformed_incoherent_or_overflowed_readings() {
        for text in [
            "",
            "MemTotal: 100 kB",
            "MemTotal: 100 kB\nMemAvailable: 101 kB",
            "MemTotal: 0 kB\nMemAvailable: 0 kB",
            "MemTotal: 18446744073709551615 kB\nMemAvailable: 0 kB",
            "MemTotal: 100 MB\nMemAvailable: 10 kB",
            "MemTotal: 100 kB\nMemTotal: 100 kB\nMemAvailable: 10 kB",
        ] {
            assert_eq!(parse_memory(text), None, "{text}");
        }
    }
}
