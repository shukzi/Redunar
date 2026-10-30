//! Reuse completed NVIDIA linear-buffer imports within one encoder generation.
use super::{
    NVIDIA_VENDOR_ID, VulkanPackedPixelFormat, VulkanVideoDeviceError, VulkanVideoDmaBufFrame,
    VulkanVideoH264Parameters, VulkanVideoImportedBuffer, VulkanVideoInputMode,
};
use std::collections::VecDeque;
use std::fs::File;
use std::os::unix::fs::MetadataExt;

// Includes the imports in pending GPU slots, not eight extra idle imports.
const MAX_IMPORTS: usize = 8;
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct BufferIdentity {
    device: u64,
    inode: u64,
    bytes: u64,
    width: u32,
    height: u32,
    format: VulkanPackedPixelFormat,
    offset: u32,
    stride: u32,
}

impl BufferIdentity {
    fn read(frame: &VulkanVideoDmaBufFrame) -> Option<Self> {
        if frame.allocation_size > MAX_IMPORT_BYTES {
            return None;
        }
        let metadata = frame.file.metadata().ok()?;
        (metadata.ino() != 0).then_some(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: frame.allocation_size,
            width: frame.width,
            height: frame.height,
            format: frame.format,
            offset: frame.offset,
            stride: frame.stride,
        })
    }
}

pub(super) struct ImportedInput {
    pub(super) buffer: VulkanVideoImportedBuffer,
    identity: Option<BufferIdentity>,
    // Keep the file identity live independently of descriptor numbers and the
    // FD consumed by Vulkan. Destroy native resources before closing this pin.
    _identity_pin: Option<File>,
}

#[derive(Default)]
pub(super) struct ImportCache {
    idle: VecDeque<ImportedInput>,
}

impl ImportCache {
    pub(super) fn import(
        &mut self,
        parameters: &VulkanVideoH264Parameters,
        frame: VulkanVideoDmaBufFrame,
        pending: usize,
    ) -> Result<ImportedInput, VulkanVideoDeviceError> {
        if frame.width != parameters.request().width || frame.height != parameters.request().height
        {
            return Err(VulkanVideoDeviceError::InvalidDmaBufFrame);
        }
        // Keep AMD and DRM-image import/lifetime behavior unchanged. The key
        // is local to this UUID-checked encoder/device and includes every
        // packed-buffer layout field; presentation timestamps are not identity.
        let identity = (parameters.session.device.candidate.vendor_id == NVIDIA_VENDOR_ID
            && frame.input_mode() == VulkanVideoInputMode::LinearBuffer)
            .then(|| BufferIdentity::read(&frame))
            .flatten();
        if let Some(identity) = identity
            && let Some(mut input) = self.take(identity)
        {
            input.buffer.timestamp_ns = frame.timestamp_ns;
            input.buffer.duration_ns = frame.duration_ns;
            return Ok(input);
        }
        let pin = identity.and_then(|_| frame.file.try_clone().ok());
        self.make_room(pending);
        let buffer = parameters.import_frame(frame)?;
        Ok(ImportedInput {
            buffer,
            identity: identity.filter(|_| pin.is_some()),
            _identity_pin: pin,
        })
    }

    fn take(&mut self, identity: BufferIdentity) -> Option<ImportedInput> {
        let position = self
            .idle
            .iter()
            .position(|input| input.identity == Some(identity))?;
        self.idle.remove(position)
    }

    fn make_room(&mut self, pending: usize) {
        while self.idle.len().saturating_add(pending) >= MAX_IMPORTS {
            if self.idle.pop_front().is_none() {
                break;
            }
        }
    }

    /// Only completed-slot collection calls this, after both GPU fences
    /// prove completion and the external-ownership release has executed.
    pub(super) fn completed(&mut self, input: ImportedInput) {
        if input.identity.is_some() {
            self.idle.push_back(input);
        }
    }

    pub(super) fn clear(&mut self) {
        self.idle.clear();
    }

    pub(super) fn retain_until_exit(&mut self) {
        for input in self.idle.drain(..) {
            std::mem::forget(input);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::OwnedFd;

    fn frame(file: File) -> VulkanVideoDmaBufFrame {
        file.set_len(64).expect("fixture size");
        VulkanVideoDmaBufFrame::new(
            OwnedFd::from(file),
            2,
            2,
            VulkanPackedPixelFormat::Rgba8,
            0,
            8,
            1,
            1,
        )
        .expect("fixture frame")
    }

    fn file(suffix: &str) -> File {
        let path = std::env::temp_dir().join(format!(
            "redunar-import-cache-{}-{suffix}",
            std::process::id()
        ));
        let file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .expect("private fixture file");
        std::fs::remove_file(path).expect("unlink owned fixture file");
        file
    }

    #[test]
    fn identity_tracks_allocation_and_layout_instead_of_fd_or_timestamp() {
        let first = frame(file("identity"));
        let mut duplicate = frame(first.file.try_clone().expect("duplicate"));
        duplicate.timestamp_ns = 500;
        duplicate.duration_ns = 20;
        assert_eq!(
            BufferIdentity::read(&first),
            BufferIdentity::read(&duplicate)
        );
        duplicate.stride = 16;
        assert_ne!(
            BufferIdentity::read(&first),
            BufferIdentity::read(&duplicate)
        );
        duplicate.stride = first.stride;
        duplicate.offset = 4;
        assert_ne!(
            BufferIdentity::read(&first),
            BufferIdentity::read(&duplicate)
        );
        let different = frame(file("different"));
        assert_ne!(
            BufferIdentity::read(&first),
            BufferIdentity::read(&different)
        );
    }

    #[test]
    fn cached_identity_pin_survives_original_descriptor_close() {
        let original = frame(file("pin"));
        let identity = BufferIdentity::read(&original);
        let pin = original.file.try_clone().expect("pin");
        drop(original);
        let another_reference = frame(pin);
        assert_eq!(identity, BufferIdentity::read(&another_reference));
    }

    #[test]
    fn oversized_allocations_are_not_retained() {
        let mut oversized = frame(file("oversized"));
        oversized.allocation_size = MAX_IMPORT_BYTES + 1;
        assert_eq!(BufferIdentity::read(&oversized), None);
    }

    fn input(suffix: &str) -> ImportedInput {
        use super::super::super::VulkanVideoImportedResource;
        use crate::ffi::{VkAllocationCallbacks, VkDevice};
        unsafe extern "system" fn unused(_: VkDevice, _: u64, _: *const VkAllocationCallbacks) {}
        let frame = frame(file(suffix));
        let identity = BufferIdentity::read(&frame);
        ImportedInput {
            buffer: VulkanVideoImportedBuffer {
                device_address: 0,
                resource: VulkanVideoImportedResource::Buffer {
                    buffer: 0,
                    memory: 0,
                    buffer_size: 64,
                    destroy_buffer: unused,
                    free_memory: unused,
                },
                width: 2,
                height: 2,
                format: VulkanPackedPixelFormat::Rgba8,
                offset_words: 0,
                stride_words: 2,
                timestamp_ns: 1,
                duration_ns: 1,
            },
            identity,
            _identity_pin: Some(frame.file),
        }
    }

    #[test]
    fn idle_eviction_reserves_space_for_pending_and_next_import() {
        let mut cache = ImportCache::default();
        let mut keys = Vec::new();
        for index in 0..MAX_IMPORTS {
            let input = input(&format!("budget-{index}"));
            keys.push(input.identity.expect("identity"));
            cache.completed(input);
        }
        cache.make_room(4);
        assert_eq!(cache.idle.len(), 3);
        assert!(cache.take(keys[0]).is_none(), "oldest idle input evicted");
        assert!(
            cache.take(keys[7]).is_some(),
            "recent completed input reusable"
        );
        assert!(
            cache.take(keys[7]).is_none(),
            "checked-out input cannot be reused concurrently"
        );
        cache.clear();
        assert!(cache.idle.is_empty());
    }
}
