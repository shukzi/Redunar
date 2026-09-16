//! Small Linux Unix-socket descriptor transport boundary.
//!
//! Replay pixels stay in DMA-BUF memory. `SCM_RIGHTS` is the only reliable way
//! to transfer ownership of an exported descriptor between the game process
//! and Redunar's daemon; reopening `/proc/<pid>/fd` does not work for DMA-BUFs.

use nix::sys::socket::{
    ControlMessage, ControlMessageOwned, MsgFlags, SockaddrLike, UnixAddr, recvmsg, sendmsg,
};
use std::io::{self, IoSlice, IoSliceMut};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;

pub struct ReceivedDatagram {
    pub length: usize,
    pub descriptor: Option<OwnedFd>,
    pub sender_path: Option<PathBuf>,
}

/// Send exactly one descriptor alongside one connected datagram.
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

/// Receive one datagram and at most one descriptor, with close-on-exec set by
/// the kernel before the descriptor becomes visible to this process.
///
/// # Errors
///
/// Returns an operating-system error when the datagram cannot be received or
/// its ancillary descriptor data is truncated.
pub fn receive_datagram_fd(
    socket: &UnixDatagram,
    buffer: &mut [u8],
) -> io::Result<ReceivedDatagram> {
    let mut payload = [IoSliceMut::new(buffer)];
    let mut control = nix::cmsg_space!([RawFd; 1]);
    let message = recvmsg::<UnixAddr>(
        socket.as_raw_fd(),
        &mut payload,
        Some(&mut control),
        MsgFlags::MSG_CMSG_CLOEXEC,
    )
    .map_err(io::Error::from)?;
    let length = message.bytes;
    let sender_path = message.address.and_then(|address| {
        (address.len() > 0)
            .then(|| address.path().map(PathBuf::from))
            .flatten()
    });
    let mut descriptors = Vec::new();
    for control_message in message.cmsgs().map_err(io::Error::from)? {
        if let ControlMessageOwned::ScmRights(rights) = control_message {
            descriptors.extend(rights);
        }
    }
    let descriptor = if descriptors.len() == 1 {
        descriptors.pop().map(|raw| {
            // SAFETY: SCM_RIGHTS returned a new descriptor owned by this process.
            unsafe { OwnedFd::from_raw_fd(raw) }
        })
    } else {
        for raw in descriptors {
            // SAFETY: every SCM_RIGHTS descriptor must be closed on rejection.
            drop(unsafe { OwnedFd::from_raw_fd(raw) });
        }
        None
    };
    Ok(ReceivedDatagram {
        length,
        descriptor,
        sender_path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn descriptor_and_metadata_share_one_datagram() {
        let (sender, receiver) = UnixDatagram::pair().expect("local datagram pair");
        let source = File::open("/dev/null").expect("test descriptor");
        send_datagram_fd(&sender, b"replay-frame", source.as_raw_fd())
            .expect("descriptor transfer");

        let mut payload = [0_u8; 32];
        let datagram = receive_datagram_fd(&receiver, &mut payload).expect("descriptor receive");
        assert_eq!(&payload[..datagram.length], b"replay-frame");
        assert!(datagram.descriptor.is_some());
        assert!(datagram.sender_path.is_none());
    }

    #[test]
    #[ignore = "requires filesystem Unix sockets allowed by the host"]
    fn named_sender_path_survives_descriptor_transfer() {
        let root =
            std::env::temp_dir().join(format!("redunar-fd-transport-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).expect("create transport fixture");
        let receiver_path = root.join("capture.sock");
        let sender_path = root.join("r-42.sock");
        let receiver = UnixDatagram::bind(&receiver_path).expect("bind receiver");
        let sender = UnixDatagram::bind(&sender_path).expect("bind named sender");
        sender.connect(&receiver_path).expect("connect sender");
        let source = File::open("/dev/null").expect("test descriptor");
        send_datagram_fd(&sender, b"replay-frame", source.as_raw_fd()).expect("send descriptor");

        let mut payload = [0_u8; 32];
        let datagram = receive_datagram_fd(&receiver, &mut payload).expect("receive descriptor");
        assert_eq!(datagram.sender_path.as_deref(), Some(sender_path.as_path()));
        assert!(datagram.descriptor.is_some());
        drop(sender);
        drop(receiver);
        std::fs::remove_dir_all(root).expect("remove transport fixture");
    }
}
