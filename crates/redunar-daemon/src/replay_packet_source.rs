//! Video payloads are pulled one at a time; timing is known before muxing.
use crate::EncodedReplayPacket;
use std::io;

pub(crate) trait ReplayPacketSource {
    fn window(&self) -> (u64, u64);
    fn next_packet(&mut self) -> io::Result<Option<EncodedReplayPacket>>;
    fn check_cancel(&self) -> io::Result<()> {
        Ok(())
    }
}
