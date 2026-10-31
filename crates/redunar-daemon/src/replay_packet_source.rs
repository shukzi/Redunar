//! Video payloads are pulled one at a time; timing is known before muxing.
use crate::{EncodedReplayPacket, ReplayRing};
use std::collections::VecDeque;
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(crate) trait ReplayPacketSource {
    fn window(&self) -> (u64, u64);
    fn next_packet(&mut self) -> io::Result<Option<EncodedReplayPacket>>;
    fn check_cancel(&self) -> io::Result<()> {
        Ok(())
    }
}

/// An immutable RAM snapshot shares payloads with capture while using the same
/// per-packet cancellation boundary as a pinned disk snapshot.
pub(crate) struct MemoryPackets {
    packets: VecDeque<EncodedReplayPacket>,
    window: (u64, u64),
    cancel: Arc<AtomicBool>,
}

impl MemoryPackets {
    pub(crate) fn new(ring: ReplayRing, cancel: Arc<AtomicBool>) -> io::Result<Self> {
        let packets = ring.into_packets();
        let first = packets
            .front()
            .ok_or_else(|| io::Error::other("empty Replay snapshot"))?;
        let last = packets.back().expect("nonempty Replay snapshot");
        let window = (
            first.timestamp_ns(),
            last.timestamp_ns() + last.duration_ns(),
        );
        Ok(Self {
            packets,
            window,
            cancel,
        })
    }
}

impl ReplayPacketSource for MemoryPackets {
    fn window(&self) -> (u64, u64) {
        self.window
    }

    fn next_packet(&mut self) -> io::Result<Option<EncodedReplayPacket>> {
        self.check_cancel()?;
        Ok(self.packets.pop_front())
    }

    fn check_cancel(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Replay save cancelled",
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ReplayAudioSnapshot, ReplayAudioTracks, ReplayBudget, ReplayClipStore, ReplayOutputFormat,
        ReplayPacketFormat, ReplayVideoCodec, ReplayVideoStream,
    };
    use redunar_core::ReplaySettings;
    use std::sync::atomic::AtomicU64;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct CancelDuringRead(MemoryPackets);

    impl ReplayPacketSource for CancelDuringRead {
        fn window(&self) -> (u64, u64) {
            self.0.window()
        }
        fn next_packet(&mut self) -> io::Result<Option<EncodedReplayPacket>> {
            let packet = self.0.next_packet()?;
            // Cancel after muxing has begun, rather than just before startup.
            self.0.cancel.store(true, Ordering::Release);
            Ok(packet)
        }
        fn check_cancel(&self) -> io::Result<()> {
            self.0.check_cancel()
        }
    }

    #[test]
    fn cancelled_ram_mux_cleans_temporary_output_in_both_containers() {
        for format in [ReplayOutputFormat::Matroska, ReplayOutputFormat::Mp4] {
            let root = std::env::temp_dir().join(format!(
                "redunar-memory-cancel-{}-{}",
                std::process::id(),
                NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
            ));
            let settings = ReplaySettings::default();
            let store =
                ReplayClipStore::open(&root, ReplayBudget::from_settings(settings)).unwrap();
            let stream = ReplayVideoStream::new(
                ReplayVideoCodec::H264,
                ReplayPacketFormat::H264LengthPrefixed4,
                320,
                240,
                60,
                vec![
                    1, 66, 0, 30, 0xff, 0xe1, 0, 2, 0x67, 0x42, 1, 0, 2, 0x68, 0xce,
                ],
            )
            .unwrap();
            let mut ring = ReplayRing::new(settings);
            for index in 0..3 {
                ring.push(
                    EncodedReplayPacket::new(
                        index * 1_000_000_000,
                        1_000_000_000,
                        true,
                        vec![0, 0, 0, 2, 0x65, 0x88],
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            let mut packets = CancelDuringRead(
                MemoryPackets::new(ring, Arc::new(AtomicBool::new(false))).unwrap(),
            );
            let audio = ReplayAudioTracks {
                system: ReplayAudioSnapshot {
                    stream: redunar_capture_audio::OpusStreamDescription::default(),
                    packets: vec![],
                },
                microphone: None,
            };
            let error = store
                .save_streaming_tracks(&stream, &mut packets, &audio, format)
                .unwrap_err();
            assert!(error.to_string().contains("cancelled"), "{error}");
            assert!(store.inventory().unwrap().is_empty());
            assert_eq!(
                std::fs::read_dir(&root).unwrap().count(),
                1,
                "only ownership marker remains"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
