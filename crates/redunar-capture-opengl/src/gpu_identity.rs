//! Context-owned UUIDs shared by GL external-memory and Vulkan. Query once
//! per context, never infer a GPU from a renderer label or DRM node ordering.
use redunar_capture::CaptureGpuIdentity;
use std::collections::BTreeMap;
use std::ffi::{CStr, c_int, c_uint};
use std::sync::{LazyLock, Mutex};

const MAX_CONTEXTS: usize = 16;
static IDENTITIES: LazyLock<Mutex<BTreeMap<usize, Option<CaptureGpuIdentity>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));
type GetInteger = unsafe extern "C" fn(c_uint, *mut c_int);
type GetString = unsafe extern "C" fn(c_uint) -> *const u8;
type GetStringIndexed = unsafe extern "C" fn(c_uint, c_uint) -> *const u8;
type GetBytes = unsafe extern "C" fn(c_uint, *mut u8);
type GetIndexedBytes = unsafe extern "C" fn(c_uint, c_uint, *mut u8);

struct Functions {
    integer: GetInteger,
    string: GetString,
    string_indexed: GetStringIndexed,
    bytes: GetBytes,
    bytes_indexed: GetIndexedBytes,
}

pub(super) fn for_context(context: usize) -> Option<CaptureGpuIdentity> {
    if context == 0 {
        return None;
    }
    let Ok(mut identities) = IDENTITIES.try_lock() else {
        return None;
    };
    if let Some(identity) = identities.get(&context) {
        return *identity;
    }
    if identities.len() >= MAX_CONTEXTS {
        return None;
    }
    // The selected presentation hook has this context current. UUID querying
    // is guarded by its complete extension token and exact function ABI.
    let identity = unsafe { query_current() };
    identities.insert(context, identity);
    identity
}

pub(super) fn destroyed(context: usize) {
    IDENTITIES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&context);
}

unsafe fn query_current() -> Option<CaptureGpuIdentity> {
    let string: GetString = super::resolve_gl_function(b"glGetString\0")?;
    let version = unsafe { string(0x1f02) };
    if version.is_null()
        || !supports_uuid_query(unsafe { CStr::from_ptr(version.cast()) }.to_bytes())
    {
        return None;
    }
    let functions = Functions {
        integer: super::resolve_gl_function(b"glGetIntegerv\0")?,
        string,
        string_indexed: super::resolve_gl_function(b"glGetStringi\0")?,
        bytes: super::resolve_gl_function(b"glGetUnsignedBytevEXT\0")?,
        bytes_indexed: super::resolve_gl_function(b"glGetUnsignedBytei_vEXT\0")?,
    };
    unsafe { query_identity(&functions) }
}

fn supports_uuid_query(version: &[u8]) -> bool {
    // Match the production desktop-GL export gate. Never query modern enums
    // on legacy/ES contexts or consume the application's GL error state.
    matches!(version.first(), Some(b'3'..=b'9'))
}

unsafe fn query_identity(functions: &Functions) -> Option<CaptureGpuIdentity> {
    let vendor = unsafe { (functions.string)(0x1f00) };
    if vendor.is_null() {
        return None;
    }
    let vendor = vendor_id(unsafe { CStr::from_ptr(vendor.cast()) }.to_bytes())?;
    let mut count = 0;
    unsafe { (functions.integer)(0x821d, &raw mut count) };
    let supported = (1..=4096).contains(&count)
        && (0..count.cast_unsigned()).any(|index| {
            let token = unsafe { (functions.string_indexed)(0x1f03, index) };
            !token.is_null()
                && unsafe { CStr::from_ptr(token.cast()) }.to_bytes() == b"GL_EXT_memory_object"
        });
    if !supported {
        return None;
    }
    let mut count = 0;
    unsafe { (functions.integer)(0x9596, &raw mut count) };
    // A multi-device GL context cannot be attributed to one encoder.
    if count != 1 {
        return None;
    }
    let mut device = [0; 16];
    let mut driver = [0; 16];
    unsafe {
        (functions.bytes_indexed)(0x9597, 0, device.as_mut_ptr());
        (functions.bytes)(0x9598, driver.as_mut_ptr());
    }
    CaptureGpuIdentity::new(vendor, device, driver).ok()
}

fn vendor_id(vendor: &[u8]) -> Option<u32> {
    match vendor {
        b"NVIDIA Corporation" => Some(0x10de),
        b"AMD" | b"ATI Technologies Inc." => Some(0x1002),
        b"Intel" | b"Intel Inc." | b"Intel Open Source Technology Center" => Some(0x8086),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" fn vendor(_: c_uint) -> *const u8 {
        c"NVIDIA Corporation".as_ptr().cast()
    }
    unsafe extern "C" fn extension(_: c_uint, _: c_uint) -> *const u8 {
        c"GL_EXT_memory_object".as_ptr().cast()
    }
    unsafe extern "C" fn partial_extension(_: c_uint, _: c_uint) -> *const u8 {
        c"GL_EXT_memory_object_fd".as_ptr().cast()
    }
    unsafe extern "C" fn one_device(_: c_uint, out: *mut c_int) {
        unsafe { *out = 1 };
    }
    unsafe extern "C" fn multiple_devices(name: c_uint, out: *mut c_int) {
        unsafe { *out = if name == 0x9596 { 2 } else { 1 } };
    }
    unsafe extern "C" fn driver(name: c_uint, out: *mut u8) {
        assert_eq!(name, 0x9598);
        unsafe { std::ptr::copy_nonoverlapping([2; 16].as_ptr(), out, 16) };
    }
    unsafe extern "C" fn device(name: c_uint, index: c_uint, out: *mut u8) {
        assert_eq!((name, index), (0x9597, 0));
        unsafe { std::ptr::copy_nonoverlapping([1; 16].as_ptr(), out, 16) };
    }

    #[test]
    fn context_uuid_query_requires_complete_extension_and_one_device() {
        let mut functions = Functions {
            integer: one_device,
            string: vendor,
            string_indexed: extension,
            bytes: driver,
            bytes_indexed: device,
        };
        assert_eq!(
            unsafe { query_identity(&functions) },
            Some(CaptureGpuIdentity::new(0x10de, [1; 16], [2; 16]).unwrap())
        );
        functions.integer = multiple_devices;
        assert_eq!(unsafe { query_identity(&functions) }, None);
        functions.integer = one_device;
        functions.string_indexed = partial_extension;
        assert_eq!(unsafe { query_identity(&functions) }, None);
        assert!(!supports_uuid_query(b"2.1 Mesa"));
        assert!(!supports_uuid_query(b"OpenGL ES 3.2"));
        assert!(supports_uuid_query(b"4.6.0 NVIDIA"));
    }
    #[test]
    fn vendor_identity_is_allowlisted_and_cache_is_cleared_on_context_destroy() {
        assert_eq!(vendor_id(b"NVIDIA Corporation"), Some(0x10de));
        assert_eq!(vendor_id(b"Intel"), Some(0x8086));
        assert_eq!(vendor_id(b"unknown RTX label"), None);
        let identity = CaptureGpuIdentity::new(0x10de, [1; 16], [2; 16]).unwrap();
        IDENTITIES
            .lock()
            .unwrap()
            .insert(usize::MAX, Some(identity));
        assert_eq!(for_context(usize::MAX), Some(identity));
        destroyed(usize::MAX);
        assert!(!IDENTITIES.lock().unwrap().contains_key(&usize::MAX));
    }
}
