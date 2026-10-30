//! GPU-only NVIDIA storage with a fence-polled copy between opaque-GL and DMA-BUF allocations.
use super::{
    ExportFunctions, GL_HANDLE_TYPE_OPAQUE_FD_EXT, GL_NO_ERROR, GL_PIXEL_PACK_BUFFER,
    MAX_SLOT_BYTES, Slot, close_descriptor,
};
use redunar_capture::ReplaySourceCandidate;
use std::ffi::{c_int, c_uint, c_void};
use std::sync::Mutex;

#[derive(Default)]
struct Retry {
    identity: Option<redunar_capture::CaptureGpuIdentity>,
    bytes: u64,
    deadline: u64,
    failures: u32,
    unsupported: bool,
}

impl Retry {
    fn permitted(
        &mut self,
        identity: redunar_capture::CaptureGpuIdentity,
        bytes: u64,
        now: u64,
    ) -> bool {
        if self.identity != Some(identity) {
            *self = Self {
                identity: Some(identity),
                ..Self::default()
            };
        }
        if self.unsupported {
            return false;
        }
        // A smaller source can fit even while the previous resolution cannot.
        if bytes < self.bytes {
            self.deadline = 0;
            self.failures = 0;
        }
        self.bytes = bytes;
        now >= self.deadline
    }

    fn failed(&mut self, permanent: bool, now: u64) {
        self.unsupported = permanent;
        self.failures = self.failures.saturating_add(1);
        let delay = match self.failures {
            1 => 100_000_000,
            2 => 500_000_000,
            _ => 2_000_000_000,
        };
        self.deadline = now.saturating_add(delay);
    }
}

static RETRY: Mutex<Retry> = Mutex::new(Retry {
    identity: None,
    bytes: 0,
    deadline: 0,
    failures: 0,
    unsupported: false,
});

type GenSemaphores = unsafe extern "C" fn(c_int, *mut c_uint);
type DeleteSemaphores = unsafe extern "C" fn(c_int, *const c_uint);
type ImportSemaphore = unsafe extern "C" fn(c_uint, c_uint, c_int);
type SemaphoreBarrier =
    unsafe extern "C" fn(c_uint, c_uint, *const c_uint, c_uint, *const c_uint, *const c_uint);

#[derive(Clone, Copy)]
pub(super) struct Semaphores {
    names: [c_uint; 2],
    delete: DeleteSemaphores,
    wait: SemaphoreBarrier,
    signal: SemaphoreBarrier,
}

unsafe extern "C" {
    fn redunar_nvidia_memory_allocate(
        uuid: *const u8,
        driver: *const u8,
        bytes: u64,
        opaque: *mut c_int,
        dma: *mut c_int,
        allocation_bytes: *mut u64,
        dedicated: *mut c_int,
        failure: *mut c_int,
    ) -> *mut c_void;
    fn redunar_nvidia_memory_semaphores(
        owner: *mut c_void,
        gl: *mut c_int,
        vk: *mut c_int,
    ) -> c_int;
    fn redunar_nvidia_memory_free(owner: *mut c_void);
    fn redunar_nvidia_memory_poll(owner: *mut c_void) -> c_int;
    fn redunar_nvidia_memory_rearm(owner: *mut c_void) -> c_int;
}

/// # Safety
/// The current GL context must match the source identity. Ownership transfers
/// into Slot and is released only after its GL fence and daemon leases end.
pub(super) unsafe fn allocate(
    functions: &ExportFunctions,
    source: ReplaySourceCandidate,
) -> Option<Slot> {
    if super::failure_injected("allocate") {
        return None;
    }
    let identity = source.gpu_identity?;
    if identity.vendor_id() != 0x10de {
        return None;
    }
    let stride = source.width.checked_mul(4)?;
    let bytes = u64::from(stride).checked_mul(u64::from(source.height))?;
    if source.width == 0 || source.height == 0 || bytes > MAX_SLOT_BYTES {
        return None;
    }
    let now = super::monotonic_now()?;
    let mut retry = RETRY.try_lock().ok()?;
    if !retry.permitted(identity, bytes, now) {
        return None;
    }
    if !super::has_extension(functions, b"GL_EXT_semaphore")
        || !super::has_extension(functions, b"GL_EXT_semaphore_fd")
    {
        retry.failed(true, now);
        super::report_unavailable_once(
            redunar_capture::ReplaySourceRejection::ExternalMemoryUnsupported,
        );
        return None;
    }
    let size = isize::try_from(bytes).ok()?;
    let uuid = identity.device_uuid();
    let driver = identity.driver_uuid();
    let (mut opaque, mut dma) = (-1, -1);
    let mut allocation_bytes = 0;
    let mut dedicated = 0;
    let mut failure = 0;
    // SAFETY: fixed UUID arrays and output fields remain live for the call.
    let owner = unsafe {
        redunar_nvidia_memory_allocate(
            uuid.as_ptr(),
            driver.as_ptr(),
            bytes,
            &raw mut opaque,
            &raw mut dma,
            &raw mut allocation_bytes,
            &raw mut dedicated,
            &raw mut failure,
        )
    };
    if owner.is_null() {
        retry.failed(failure < 0, now);
        return None;
    }
    if super::failure_injected("import") {
        // SAFETY: no GL import exists; native teardown retains initialization work.
        unsafe { redunar_nvidia_memory_free(owner) };
        close_descriptor(opaque);
        close_descriptor(dma);
        retry.failed(false, now);
        return None;
    }
    // SAFETY: inherited current-context and live allocation invariants.
    let (imported, memory, buffer) =
        unsafe { import_gl(functions, &mut opaque, allocation_bytes, dedicated, size) };
    if !imported {
        // SAFETY: GL objects are deleted first; native teardown retains pending
        // storage. The remaining DMA descriptor has never left this process.
        unsafe {
            if buffer != 0 {
                (functions.delete_buffers)(1, &raw const buffer);
            }
            if memory != 0 {
                (functions.delete_memory_objects)(1, &raw const memory);
            }
            redunar_nvidia_memory_free(owner);
        }
        if opaque >= 0 {
            close_descriptor(opaque);
        }
        if dma >= 0 {
            close_descriptor(dma);
        }
        retry.failed(false, now);
        return None;
    }
    // SAFETY: the same context and live owner are retained through all imports.
    let semaphores = unsafe { import_semaphores(owner) };
    let Some(semaphores) = semaphores else {
        unsafe {
            (functions.delete_buffers)(1, &raw const buffer);
            (functions.delete_memory_objects)(1, &raw const memory);
            redunar_nvidia_memory_free(owner);
        }
        close_descriptor(dma);
        retry.failed(false, now);
        return None;
    };
    retry.failures = 0;
    retry.deadline = 0;
    Some(Slot {
        nvidia_semaphores: Some(semaphores),
        memory,
        buffer,
        fd: dma,
        stride,
        bytes,
        nvidia_allocation: owner.addr(),
        ..Slot::IDLE
    })
}

/// # Safety
/// The current context owns these GL objects; opaque references a live Vulkan
/// export. Partial GL names are returned for the caller's transactional cleanup.
unsafe fn import_gl(
    functions: &ExportFunctions,
    opaque: &mut c_int,
    allocation_bytes: u64,
    dedicated: c_int,
    size: isize,
) -> (bool, c_uint, c_uint) {
    let (mut memory, mut buffer) = (0, 0);
    let mut stage = 1;
    let imported = (|| {
        // SAFETY: the caller keeps this GL context current and owns these names.
        unsafe {
            (functions.create_memory_objects)(1, &raw mut memory);
            if memory == 0 {
                return false;
            }
            if dedicated != 0 {
                type Parameter = unsafe extern "C" fn(c_uint, c_uint, *const c_int);
                let Some(parameter): Option<Parameter> =
                    crate::resolve_gl_function(b"glMemoryObjectParameterivEXT\0")
                else {
                    return false;
                };
                let enabled = 1;
                parameter(memory, 0x9581, &raw const enabled);
            }
            if (functions.get_error)() != GL_NO_ERROR {
                return false;
            }
            stage = 2;
            (functions.import_memory_fd)(
                memory,
                allocation_bytes,
                GL_HANDLE_TYPE_OPAQUE_FD_EXT,
                *opaque,
            );
            if (functions.get_error)() != GL_NO_ERROR {
                return false;
            }
            // EXT_external_objects_fd transfers ownership on successful import.
            *opaque = -1;
            stage = 3;
            (functions.gen_buffers)(1, &raw mut buffer);
            if buffer == 0 {
                return false;
            }
            (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, buffer);
            stage = 4;
            (functions.buffer_storage_mem)(GL_PIXEL_PACK_BUFFER, size, memory, 0);
            (functions.bind_buffer)(GL_PIXEL_PACK_BUFFER, 0);
            (functions.get_error)() == GL_NO_ERROR
        }
    })();
    if !imported {
        eprintln!("Redunar NVIDIA GL memory import stage={stage}");
    }
    (imported, memory, buffer)
}

/// Import two independent binary semaphore payloads, in GL -> Vulkan and
/// Vulkan -> GL order. Successful FD imports consume their descriptors.
unsafe fn import_semaphores(owner: *mut c_void) -> Option<Semaphores> {
    let gen_fn: GenSemaphores = crate::resolve_gl_function(b"glGenSemaphoresEXT\0")?;
    let delete: DeleteSemaphores = crate::resolve_gl_function(b"glDeleteSemaphoresEXT\0")?;
    let import: ImportSemaphore = crate::resolve_gl_function(b"glImportSemaphoreFdEXT\0")?;
    let wait = crate::resolve_gl_function(b"glWaitSemaphoreEXT\0")?;
    let signal = crate::resolve_gl_function(b"glSignalSemaphoreEXT\0")?;
    let error: unsafe extern "C" fn() -> c_uint = crate::resolve_gl_function(b"glGetError\0")?;
    let mut fds = [-1; 2];
    let mut names = [0; 2];
    unsafe {
        if redunar_nvidia_memory_semaphores(owner, &raw mut fds[0], &raw mut fds[1]) == 0 {
            return None;
        }
        gen_fn(2, names.as_mut_ptr());
        for index in 0..2 {
            if names[index] == 0 {
                break;
            }
            import(names[index], GL_HANDLE_TYPE_OPAQUE_FD_EXT, fds[index]);
            if error() != GL_NO_ERROR {
                break;
            }
            fds[index] = -1;
            if index == 1 {
                return Some(Semaphores {
                    names,
                    delete,
                    wait,
                    signal,
                });
            }
        }
        delete(2, names.as_ptr());
    }
    for fd in fds {
        close_descriptor(fd);
    }
    None
}

impl Semaphores {
    // The caller owns the current GL context, buffer and semaphore payloads.
    pub(super) unsafe fn wait(self, buffer: c_uint) {
        unsafe {
            (self.wait)(
                self.names[1],
                1,
                &raw const buffer,
                0,
                std::ptr::null(),
                std::ptr::null(),
            );
        };
    }
    pub(super) unsafe fn signal(self, buffer: c_uint) {
        unsafe {
            (self.signal)(
                self.names[0],
                1,
                &raw const buffer,
                0,
                std::ptr::null(),
                std::ptr::null(),
            );
        };
    }
    pub(super) unsafe fn delete(self) {
        unsafe { (self.delete)(2, self.names.as_ptr()) };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Completion {
    Pending,
    Ready,
    Failed,
}

fn completion(result: c_int) -> Completion {
    match result {
        0 => Completion::Pending,
        1 => Completion::Ready,
        _ => Completion::Failed,
    }
}

/// # Safety
/// Caller has ended GL and consumer leases; native teardown retains unproven work.
pub(super) unsafe fn release(owner: usize) {
    if owner != 0 {
        // SAFETY: each nonzero slot owner came from the native allocator and
        // the pool invokes release once; native teardown proves completion or retains.
        unsafe { redunar_nvidia_memory_free(owner as *mut c_void) };
    }
}

/// # Safety
/// The owner remains live and its GL copy fence has signaled.
pub(super) unsafe fn ready(owner: usize) -> Completion {
    if owner == 0 {
        Completion::Ready
    } else {
        completion(unsafe { redunar_nvidia_memory_poll(owner as *mut c_void) })
    }
}

/// # Safety
/// The owner remains live and its preceding daemon lease has ended.
pub(super) unsafe fn rearm(owner: usize) -> Completion {
    if owner == 0 {
        Completion::Ready
    } else {
        completion(unsafe { redunar_nvidia_memory_rearm(owner as *mut c_void) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_retry_is_bounded_and_smaller_sources_recover_immediately() {
        let mut retry = Retry::default();
        let identity = redunar_capture::CaptureGpuIdentity::new(0x10de, [1; 16], [2; 16]).unwrap();
        assert!(retry.permitted(identity, 32_000_000, 1_000));
        retry.failed(false, 1_000);
        assert_eq!(retry.deadline, 100_001_000);
        retry.failed(false, retry.deadline);
        assert_eq!(retry.deadline, 600_001_000);
        retry.failed(false, retry.deadline);
        assert_eq!(retry.deadline, 2_600_001_000);
        retry.failed(false, retry.deadline);
        assert_eq!(retry.deadline, 4_600_001_000);
        assert!(!retry.permitted(identity, 32_000_000, 4_000_000_000));
        assert!(retry.permitted(identity, 8_000_000, 4_000_000_000));
        retry.failed(true, 4_000_000_000);
        assert!(!retry.permitted(identity, 4_000_000, u64::MAX));
        let other = redunar_capture::CaptureGpuIdentity::new(0x10de, [3; 16], [2; 16]).unwrap();
        assert!(retry.permitted(other, 8_000_000, 1));
        assert_eq!(completion(-1), Completion::Failed);
        assert_eq!(completion(0), Completion::Pending);
        assert_eq!(completion(1), Completion::Ready);
    }

    #[test]
    fn invalid_native_allocations_fail_before_loading_the_driver() {
        let uuid = [0_u8; 16];
        for (pointer, bytes) in [
            (std::ptr::null(), 4096),
            (uuid.as_ptr(), 0),
            (uuid.as_ptr(), 64 * 1024 * 1024 + 1),
        ] {
            let (mut opaque, mut dma, mut size, mut dedicated) = (-1, -1, 0, 0);
            let mut failure = 0;
            // SAFETY: all nonnull pointers reference live local output storage.
            let owner = unsafe {
                redunar_nvidia_memory_allocate(
                    pointer,
                    uuid.as_ptr(),
                    bytes,
                    &raw mut opaque,
                    &raw mut dma,
                    &raw mut size,
                    &raw mut dedicated,
                    &raw mut failure,
                )
            };
            assert!(owner.is_null());
            assert_eq!((opaque, dma, size, dedicated), (-1, -1, 0, 0));
        }
        // SAFETY: a null owner is explicitly a no-op at the native boundary.
        unsafe { redunar_nvidia_memory_free(std::ptr::null_mut()) };
    }
}
