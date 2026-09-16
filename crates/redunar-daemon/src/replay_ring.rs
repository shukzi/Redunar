use crate::ReplayBudget;
use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

const NANOSECONDS_PER_SECOND: u64 = 1_000_000_000;
const MAX_PACKET_DURATION_NS: u64 = NANOSECONDS_PER_SECOND;
const MAX_SINGLE_PACKET_BYTES: usize = 8 * 1024 * 1024;

/// One already-encoded video packet in capture order.
///
/// The foundation intentionally accepts no raw image data. A future encoder
/// adapter must produce low-latency packets with monotonic capture timestamps
/// before they can enter the daemon-owned replay ring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EncodedReplayPacket {
    timestamp_ns: u64,
    duration_ns: u64,
    keyframe: bool,
    bytes: Arc<[u8]>,
}

impl EncodedReplayPacket {
    /// Build one bounded encoded packet.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayPacketError`] for empty or oversized payloads, zero or
    /// implausibly long durations, and timestamp overflow.
    pub fn new(
        timestamp_ns: u64,
        duration_ns: u64,
        keyframe: bool,
        bytes: Vec<u8>,
    ) -> Result<Self, ReplayPacketError> {
        if bytes.is_empty() {
            return Err(ReplayPacketError::Empty);
        }
        if bytes.len() > MAX_SINGLE_PACKET_BYTES {
            return Err(ReplayPacketError::TooLarge);
        }
        if duration_ns == 0 || duration_ns > MAX_PACKET_DURATION_NS {
            return Err(ReplayPacketError::InvalidDuration);
        }
        timestamp_ns
            .checked_add(duration_ns)
            .ok_or(ReplayPacketError::TimestampOverflow)?;
        Ok(Self {
            timestamp_ns,
            duration_ns,
            keyframe,
            bytes: bytes.into(),
        })
    }

    #[must_use]
    pub const fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    #[must_use]
    pub const fn duration_ns(&self) -> u64 {
        self.duration_ns
    }

    #[must_use]
    pub const fn is_keyframe(&self) -> bool {
        self.keyframe
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPacketError {
    Empty,
    TooLarge,
    InvalidDuration,
    TimestampOverflow,
}

impl fmt::Display for ReplayPacketError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Empty => "encoded replay packet is empty",
            Self::TooLarge => "encoded replay packet exceeds 8 MiB",
            Self::InvalidDuration => "encoded replay packet duration is invalid",
            Self::TimestampOverflow => "encoded replay packet timestamp overflows",
        })
    }
}

impl Error for ReplayPacketError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReplayRingStats {
    pub stored_packets: u32,
    pub stored_bytes: u64,
    pub dropped_awaiting_keyframe: u64,
    pub evicted_packets: u64,
    pub evicted_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayPushOutcome {
    Stored {
        evicted_packets: u32,
        evicted_bytes: u64,
    },
    DroppedAwaitingKeyframe,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplayRingError {
    NonMonotonicTimestamp,
    PacketExceedsRingBudget,
}

impl fmt::Display for ReplayRingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonMonotonicTimestamp => "replay packet timestamp is not monotonic",
            Self::PacketExceedsRingBudget => "replay packet exceeds the encoded ring budget",
        })
    }
}

impl Error for ReplayRingError {}

/// Fixed-budget encoded packet history.
///
/// All limits are enforced synchronously on insertion. When eviction removes
/// a keyframe, dependent packets are also removed so every non-empty snapshot
/// begins at a decodable boundary.
pub struct ReplayRing {
    duration_limit_ns: u64,
    packet_limit: u32,
    byte_limit: u64,
    packets: VecDeque<EncodedReplayPacket>,
    last_timestamp_ns: Option<u64>,
    stored_bytes: u64,
    dropped_awaiting_keyframe: u64,
    evicted_packets: u64,
    evicted_bytes: u64,
}

impl ReplayRing {
    #[must_use]
    pub fn new(settings: redunar_core::ReplaySettings) -> Self {
        let budget = ReplayBudget::from_settings(settings);
        Self {
            duration_limit_ns: u64::from(settings.duration.seconds())
                .saturating_mul(NANOSECONDS_PER_SECOND),
            packet_limit: budget.frame_capacity.max(1),
            byte_limit: budget.maximum_ring_bytes.max(1),
            packets: VecDeque::new(),
            last_timestamp_ns: None,
            stored_bytes: 0,
            dropped_awaiting_keyframe: 0,
            evicted_packets: 0,
            evicted_bytes: 0,
        }
    }

    /// Insert one packet while preserving all fixed limits.
    ///
    /// # Errors
    ///
    /// Returns [`ReplayRingError`] without changing the ring when timestamps
    /// move backwards or one payload cannot fit within the configured ring.
    pub fn push(
        &mut self,
        packet: EncodedReplayPacket,
    ) -> Result<ReplayPushOutcome, ReplayRingError> {
        if u64::try_from(packet.bytes.len()).unwrap_or(u64::MAX) > self.byte_limit {
            return Err(ReplayRingError::PacketExceedsRingBudget);
        }
        if self
            .last_timestamp_ns
            .is_some_and(|last| packet.timestamp_ns <= last)
        {
            return Err(ReplayRingError::NonMonotonicTimestamp);
        }
        self.last_timestamp_ns = Some(packet.timestamp_ns);
        if self.packets.is_empty() && !packet.keyframe {
            self.dropped_awaiting_keyframe = self.dropped_awaiting_keyframe.saturating_add(1);
            return Ok(ReplayPushOutcome::DroppedAwaitingKeyframe);
        }

        self.stored_bytes = self
            .stored_bytes
            .saturating_add(u64::try_from(packet.bytes.len()).unwrap_or(u64::MAX));
        self.packets.push_back(packet);

        let mut evicted_packets = 0_u32;
        let mut evicted_bytes = 0_u64;
        while self.exceeds_any_limit() {
            self.evict_front(&mut evicted_packets, &mut evicted_bytes);
        }
        while self.packets.front().is_some_and(|packet| !packet.keyframe) {
            self.evict_front(&mut evicted_packets, &mut evicted_bytes);
        }
        self.evicted_packets = self
            .evicted_packets
            .saturating_add(u64::from(evicted_packets));
        self.evicted_bytes = self.evicted_bytes.saturating_add(evicted_bytes);
        Ok(ReplayPushOutcome::Stored {
            evicted_packets,
            evicted_bytes,
        })
    }

    #[must_use]
    pub fn packets(&self) -> impl ExactSizeIterator<Item = &EncodedReplayPacket> {
        self.packets.iter()
    }

    /// Maximum packet slots reserved for this bounded in-memory history.
    #[must_use]
    pub const fn packet_capacity(&self) -> u32 {
        self.packet_limit
    }

    /// Maximum encoded payload bytes retained by this in-memory history.
    #[must_use]
    pub const fn byte_capacity(&self) -> u64 {
        self.byte_limit
    }

    #[must_use]
    pub fn stats(&self) -> ReplayRingStats {
        ReplayRingStats {
            stored_packets: u32::try_from(self.packets.len()).unwrap_or(u32::MAX),
            stored_bytes: self.stored_bytes,
            dropped_awaiting_keyframe: self.dropped_awaiting_keyframe,
            evicted_packets: self.evicted_packets,
            evicted_bytes: self.evicted_bytes,
        }
    }

    /// Discard one encoded stream epoch after resize, encoder reset, failure,
    /// or shutdown. The next epoch may restart its monotonic timestamp while
    /// cumulative eviction diagnostics remain accurate.
    pub fn reset_epoch(&mut self) {
        let removed_packets = u64::try_from(self.packets.len()).unwrap_or(u64::MAX);
        self.evicted_packets = self.evicted_packets.saturating_add(removed_packets);
        self.evicted_bytes = self.evicted_bytes.saturating_add(self.stored_bytes);
        self.packets.clear();
        self.last_timestamp_ns = None;
        self.stored_bytes = 0;
    }

    fn exceeds_any_limit(&self) -> bool {
        let duration_exceeded =
            self.packets
                .front()
                .zip(self.packets.back())
                .is_some_and(|(first, last)| {
                    last.timestamp_ns
                        .saturating_add(last.duration_ns)
                        .saturating_sub(first.timestamp_ns)
                        > self.duration_limit_ns
                });
        self.packets.len() > usize::try_from(self.packet_limit).unwrap_or(usize::MAX)
            || self.stored_bytes > self.byte_limit
            || duration_exceeded
    }

    fn evict_front(&mut self, packet_count: &mut u32, byte_count: &mut u64) {
        if let Some(packet) = self.packets.pop_front() {
            let bytes = u64::try_from(packet.bytes.len()).unwrap_or(u64::MAX);
            self.stored_bytes = self.stored_bytes.saturating_sub(bytes);
            *packet_count = packet_count.saturating_add(1);
            *byte_count = byte_count.saturating_add(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use redunar_core::{
        ReplayDuration, ReplayFrameRate, ReplayQuality, ReplaySettings, ReplayStorageLimit,
    };

    fn settings() -> ReplaySettings {
        ReplaySettings {
            duration: ReplayDuration::Seconds15,
            frame_rate: ReplayFrameRate::Fps30,
            quality: ReplayQuality::Efficient,
            storage_limit: ReplayStorageLimit::GiB5,
        }
    }

    fn packet(timestamp_ns: u64, keyframe: bool, bytes: usize) -> EncodedReplayPacket {
        EncodedReplayPacket::new(timestamp_ns, 500_000_000, keyframe, vec![7; bytes])
            .expect("valid packet")
    }

    #[test]
    fn malformed_packets_are_rejected_before_allocation_enters_the_ring() {
        assert_eq!(
            EncodedReplayPacket::new(0, 1, true, Vec::new()),
            Err(ReplayPacketError::Empty)
        );
        assert_eq!(
            EncodedReplayPacket::new(0, 0, true, vec![1]),
            Err(ReplayPacketError::InvalidDuration)
        );
        assert_eq!(
            EncodedReplayPacket::new(u64::MAX, 1, true, vec![1]),
            Err(ReplayPacketError::TimestampOverflow)
        );
        assert_eq!(
            EncodedReplayPacket::new(0, 1, true, vec![1; MAX_SINGLE_PACKET_BYTES + 1]),
            Err(ReplayPacketError::TooLarge)
        );
    }

    #[test]
    fn packets_before_the_first_keyframe_are_dropped_without_growth() {
        let mut ring = ReplayRing::new(settings());
        assert_eq!(
            ring.push(packet(1, false, 64)),
            Ok(ReplayPushOutcome::DroppedAwaitingKeyframe)
        );
        assert_eq!(ring.stats().stored_packets, 0);
        assert_eq!(ring.stats().dropped_awaiting_keyframe, 1);
        assert_eq!(
            ring.push(packet(2, true, 64)),
            Ok(ReplayPushOutcome::Stored {
                evicted_packets: 0,
                evicted_bytes: 0
            })
        );
        assert!(
            ring.packets()
                .next()
                .expect("stored keyframe")
                .is_keyframe()
        );
    }

    #[test]
    fn non_monotonic_input_does_not_disturb_existing_packets() {
        let mut ring = ReplayRing::new(settings());
        ring.push(packet(10, true, 64)).expect("first packet");
        assert_eq!(
            ring.push(packet(10, true, 64)),
            Err(ReplayRingError::NonMonotonicTimestamp)
        );
        assert_eq!(ring.stats().stored_packets, 1);
        assert_eq!(ring.stats().stored_bytes, 64);
    }

    #[test]
    fn monotonicity_survives_packets_dropped_while_awaiting_a_keyframe() {
        let mut ring = ReplayRing::new(settings());
        assert_eq!(
            ring.push(packet(10, false, 64)),
            Ok(ReplayPushOutcome::DroppedAwaitingKeyframe)
        );
        assert_eq!(
            ring.push(packet(9, true, 64)),
            Err(ReplayRingError::NonMonotonicTimestamp)
        );
        assert_eq!(ring.stats().stored_packets, 0);
    }

    #[test]
    fn duration_eviction_keeps_a_decodable_keyframe_boundary() {
        let mut ring = ReplayRing::new(settings());
        ring.push(packet(0, true, 64)).expect("first keyframe");
        for index in 1..30 {
            ring.push(packet(index * 500_000_000, index == 20, 64))
                .expect("packet");
        }
        ring.push(packet(15_000_000_000, false, 64))
            .expect("evict old duration");
        let first = ring.packets().next().expect("newer keyframe retained");
        assert!(first.is_keyframe());
        assert_eq!(first.timestamp_ns(), 10_000_000_000);
        assert!(ring.stats().evicted_packets > 0);
    }

    #[test]
    fn byte_limit_is_never_exceeded() {
        let mut ring = ReplayRing::new(settings());
        ring.byte_limit = 128;
        ring.packet_limit = 10;
        ring.duration_limit_ns = 60 * NANOSECONDS_PER_SECOND;
        ring.push(packet(0, true, 64)).expect("keyframe");
        ring.push(packet(1, true, 96)).expect("second keyframe");
        assert!(ring.stats().stored_bytes <= 128);
        assert!(
            ring.packets()
                .next()
                .expect("packet retained")
                .is_keyframe()
        );
    }

    #[test]
    fn packet_over_ring_budget_is_rejected_without_advancing_timestamp() {
        let mut ring = ReplayRing::new(settings());
        ring.byte_limit = 32;
        assert_eq!(
            ring.push(packet(10, true, 64)),
            Err(ReplayRingError::PacketExceedsRingBudget)
        );
        assert_eq!(ring.stats(), ReplayRingStats::default());
        ring.push(packet(10, true, 16))
            .expect("same timestamp remains valid after rejection");
        assert_eq!(ring.stats().stored_packets, 1);
    }

    #[test]
    fn reset_discards_the_old_epoch_and_allows_timestamp_restart() {
        let mut ring = ReplayRing::new(settings());
        ring.push(packet(100, true, 64)).expect("old epoch");
        ring.reset_epoch();
        assert_eq!(ring.stats().stored_packets, 0);
        assert_eq!(ring.stats().stored_bytes, 0);
        assert_eq!(ring.stats().evicted_packets, 1);
        assert_eq!(ring.stats().evicted_bytes, 64);
        ring.push(packet(1, true, 64))
            .expect("new epoch timestamp may restart");
        assert_eq!(ring.stats().stored_packets, 1);
    }

    #[test]
    fn ten_minutes_of_packets_retains_only_the_newest_bounded_window() {
        let mut ring = ReplayRing::new(ReplaySettings::default());
        let frame_duration_ns = NANOSECONDS_PER_SECOND / 60;
        for frame in 0..(10 * 60 * 60_u64) {
            ring.push(packet(
                frame.saturating_mul(frame_duration_ns),
                frame.is_multiple_of(120),
                128,
            ))
            .expect("monotonic packet");
        }
        let stats = ring.stats();
        let budget = ReplayBudget::from_settings(ReplaySettings::default());
        assert!(stats.stored_packets <= budget.frame_capacity);
        assert!(stats.stored_bytes <= budget.maximum_ring_bytes);
        assert!(stats.evicted_packets > 30_000);
        assert!(ring.packets().next().expect("bounded window").is_keyframe());
    }
}
