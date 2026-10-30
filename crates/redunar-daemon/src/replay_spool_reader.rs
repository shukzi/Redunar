//! Bounded metadata index over pinned spool files, never a clip-sized payload.
use crate::replay_packet_source::ReplayPacketSource;
use crate::{EncodedReplayPacket, ReplayBudget, ReplaySpoolSegment};
use redunar_core::ReplaySettings;
use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

const MAX_PACKET_BYTES: u32 = 8 * 1024 * 1024;

struct Record {
    file: usize,
    offset: u64,
    timestamp: u64,
    duration: u64,
    keyframe: bool,
    bytes: u32,
}

pub(crate) struct SpoolPackets {
    files: Vec<File>,
    records: VecDeque<Record>,
    window: (u64, u64),
    cancel: Arc<AtomicBool>,
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid Replay snapshot record")
}
fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Replay save cancelled",
        ))
    } else {
        Ok(())
    }
}

impl SpoolPackets {
    pub(crate) fn scan(
        segments: Vec<(ReplaySpoolSegment, File)>,
        settings: ReplaySettings,
        cancel: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let budget = ReplayBudget::from_settings(settings);
        let mut records: VecDeque<Record> = VecDeque::new();
        let mut files = Vec::with_capacity(segments.len());
        let mut previous = None;
        let mut bytes = 0_u64;
        for (segment, mut file) in segments {
            let mut magic = [0; 8];
            file.read_exact(&mut magic)?;
            if &magic != b"RDNSEG01" || file.metadata()?.len() != segment.bytes {
                return Err(invalid());
            }
            let mut offset = 8_u64;
            while offset < segment.bytes {
                check_cancel(&cancel)?;
                let mut header = [0_u8; 21];
                file.read_exact(&mut header)?;
                let timestamp = u64::from_le_bytes(header[..8].try_into().map_err(|_| invalid())?);
                let duration = u64::from_le_bytes(header[8..16].try_into().map_err(|_| invalid())?);
                let length = u32::from_le_bytes(header[17..21].try_into().map_err(|_| invalid())?);
                let end = timestamp.checked_add(duration).ok_or_else(invalid)?;
                offset = offset.checked_add(21).ok_or_else(invalid)?;
                let next = offset.checked_add(u64::from(length)).ok_or_else(invalid)?;
                if header[16] > 1
                    || duration == 0
                    || duration > 1_000_000_000
                    || length == 0
                    || length > MAX_PACKET_BYTES
                    || next > segment.bytes
                    || previous.is_some_and(|last| timestamp <= last)
                {
                    return Err(invalid());
                }
                previous = Some(timestamp);
                if u64::from(length) > budget.maximum_ring_bytes {
                    return Err(invalid());
                }
                // Match the existing ring suffix/keyframe policy using only
                // offsets. Payload allocation is independent of clip duration.
                if !records.is_empty() || header[16] == 1 {
                    bytes += u64::from(length);
                    records.push_back(Record {
                        file: files.len(),
                        offset,
                        timestamp,
                        duration,
                        keyframe: header[16] == 1,
                        bytes: length,
                    });
                    while records.front().is_some_and(|first| {
                        records.len() > budget.frame_capacity as usize
                            || bytes > budget.maximum_ring_bytes
                            || end.saturating_sub(first.timestamp)
                                > u64::from(settings.duration.seconds()) * 1_000_000_000
                            || !first.keyframe
                    }) {
                        bytes -=
                            u64::from(records.pop_front().expect("nonempty record index").bytes);
                    }
                }
                file.seek(SeekFrom::Start(next))?;
                offset = next;
            }
            files.push(file);
        }
        let first = records.front().ok_or_else(invalid)?;
        let last = records.back().ok_or_else(invalid)?;
        let window = (first.timestamp, last.timestamp + last.duration);
        Ok(Self {
            files,
            records,
            window,
            cancel,
        })
    }
}

impl ReplayPacketSource for SpoolPackets {
    fn check_cancel(&self) -> io::Result<()> {
        check_cancel(&self.cancel)
    }
    fn window(&self) -> (u64, u64) {
        self.window
    }
    fn next_packet(&mut self) -> io::Result<Option<EncodedReplayPacket>> {
        check_cancel(&self.cancel)?;
        let Some(record) = self.records.pop_front() else {
            return Ok(None);
        };
        let file = &mut self.files[record.file];
        file.seek(SeekFrom::Start(record.offset))?;
        let mut bytes = vec![0; record.bytes as usize];
        file.read_exact(&mut bytes)?;
        EncodedReplayPacket::new(record.timestamp, record.duration, record.keyframe, bytes)
            .map(Some)
            .map_err(|_| invalid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ReplayRing;
    use redunar_core::ReplayDuration;
    use std::io::Write;

    #[test]
    fn metadata_suffix_matches_ring_and_pinned_files_survive_retention() {
        let path = std::env::temp_dir().join(format!("redunar-reader-{}", std::process::id()));
        let mut file = File::create(&path).unwrap();
        file.write_all(b"RDNSEG01").unwrap();
        let settings = ReplaySettings {
            duration: ReplayDuration::Seconds15,
            ..ReplaySettings::default()
        };
        let mut ring = ReplayRing::new(settings);
        for index in 0..40_u64 {
            let packet = EncodedReplayPacket::new(
                index * 500_000_000,
                500_000_000,
                index % 4 == 0,
                vec![1, 2, 3],
            )
            .unwrap();
            file.write_all(&packet.timestamp_ns().to_le_bytes())
                .unwrap();
            file.write_all(&packet.duration_ns().to_le_bytes()).unwrap();
            file.write_all(&[u8::from(packet.is_keyframe())]).unwrap();
            file.write_all(&3_u32.to_le_bytes()).unwrap();
            file.write_all(packet.bytes()).unwrap();
            ring.push(packet).unwrap();
        }
        drop(file);
        let file = File::open(&path).unwrap();
        let segment = ReplaySpoolSegment {
            path: path.clone(),
            start_timestamp_ns: 0,
            end_timestamp_ns: 20_000_000_000,
            bytes: file.metadata().unwrap().len(),
        };
        std::fs::remove_file(path).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let mut source =
            SpoolPackets::scan(vec![(segment, file)], settings, cancel.clone()).unwrap();
        assert!(
            source.records.len() <= ReplayBudget::from_settings(settings).frame_capacity as usize
        );
        for expected in ring.packets() {
            assert_eq!(source.next_packet().unwrap().as_ref(), Some(expected));
        }
        assert!(source.next_packet().unwrap().is_none());
        cancel.store(true, Ordering::Release);
        assert_eq!(
            source.next_packet().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
}
