//! One-descriptor transport for OpenGL Replay exports.
//!
//! The GBM DMA-BUF stays in GPU memory; only the descriptor travels, through
//! exactly the same `SCM_RIGHTS` boundary the Vulkan producer uses. The GL
//! producer never receives descriptors, so this side is send-only.

use nix::fcntl::{FcntlArg, fcntl};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use std::io::{self, IoSlice};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixDatagram;

/// Send exactly one descriptor alongside one connected datagram.
///
/// The caller retains ownership of the descriptor; the kernel installs a
/// fresh descriptor in the receiver.
///
/// # Errors
///
/// Returns an operating-system error when the datagram or descriptor cannot
/// be transferred atomically.
pub fn send_datagram_fd(
    socket: &UnixDatagram,
    payload: &[u8],
    descriptor: RawFd,
) -> io::Result<usize> {
    let payload = [IoSlice::new(payload)];
    let descriptors = [descriptor];
    let control = [ControlMessage::ScmRights(&descriptors)];
    sendmsg::<()>(
        socket.as_raw_fd(),
        &payload,
        &control,
        MsgFlags::empty(),
        None,
    )
    .map_err(io::Error::from)
}

/// Duplicate a descriptor with close-on-exec set, ready for one transfer.
///
/// # Errors
///
/// Returns an operating-system error when the descriptor table cannot accept
/// another entry.
pub fn duplicate_for_transfer(descriptor: RawFd) -> io::Result<RawFd> {
    // F_DUPFD_CLOEXEC installs the duplicate at descriptor 3 or above, so a
    // transfer copy can never collide with stdio or look like a protocol fd.
    // SAFETY: fcntl on an open descriptor with a duplicate argument only
    // reads the descriptor table; the returned value is a fresh owned fd.
    fcntl(descriptor, FcntlArg::F_DUPFD_CLOEXEC(3)).map_err(io::Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::os::fd::{FromRawFd, OwnedFd};

    #[test]
    fn descriptor_and_metadata_share_one_datagram() {
        let (sender, receiver) = UnixDatagram::pair().expect("local datagram pair");
        let source = File::open("/dev/null").expect("test descriptor");
        let duplicate =
            duplicate_for_transfer(source.as_raw_fd()).expect("close-on-exec duplicate");
        send_datagram_fd(&sender, b"replay-frame", duplicate).expect("descriptor transfer");
        // The transferring process always closes its own copy; the kernel
        // installed an independent descriptor in the receiver instead.
        drop(unsafe { OwnedFd::from_raw_fd(duplicate) });

        let mut payload = [0_u8; 32];
        receiver.recv(&mut payload).expect("datagram receive");
        assert_eq!(&payload[..12], b"replay-frame");
    }
}
