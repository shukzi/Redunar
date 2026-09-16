//! Narrow read-only libdrm/KMS framebuffer export boundary.
//!
//! The selected card path is constructed from a bounded numeric index. This
//! module never modesets, changes an object property, creates a framebuffer,
//! or submits GPU work. Its only state-changing ioctl enables universal-plane
//! visibility for this file descriptor.

use std::ffi::{c_char, c_int, c_void};
use std::fmt;
use std::fs::{File, OpenOptions};
use std::mem;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::ptr;

const MAX_DRM_OBJECTS: usize = 64;
const DRM_CLIENT_CAP_UNIVERSAL_PLANES: u64 = 2;
const DRM_MODE_CONNECTED: u32 = 1;
const DRM_MODE_OBJECT_PLANE: u32 = 0xeeee_eeee;
const DRM_PLANE_TYPE_PRIMARY: u64 = 1;
const DRM_CLOEXEC: u32 = 0x0008_0000;
const DRM_RDWR: u32 = 0x2;
const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");
const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");

#[derive(Debug)]
pub enum KmsDrmError {
    InvalidSelection,
    LibraryUnavailable,
    SymbolUnavailable(&'static str),
    DeviceUnavailable,
    KmsUnavailable,
    OutputUnavailable,
    PrimaryPlaneUnavailable,
    FramebufferUnavailable,
    FramebufferLayoutUnsupported,
    DescriptorExportFailed,
}

impl fmt::Display for KmsDrmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSelection => formatter.write_str("invalid KMS output selection"),
            Self::LibraryUnavailable => formatter.write_str("libdrm is unavailable"),
            Self::SymbolUnavailable(symbol) => {
                write!(formatter, "libdrm symbol is unavailable: {symbol}")
            }
            Self::DeviceUnavailable => formatter.write_str("selected DRM card is unavailable"),
            Self::KmsUnavailable => formatter.write_str("selected DRM card does not provide KMS"),
            Self::OutputUnavailable => formatter.write_str("selected KMS output is unavailable"),
            Self::PrimaryPlaneUnavailable => {
                formatter.write_str("selected output has no readable primary plane")
            }
            Self::FramebufferUnavailable => {
                formatter.write_str("the current KMS framebuffer is unavailable")
            }
            Self::FramebufferLayoutUnsupported => {
                formatter.write_str("the current KMS framebuffer layout is unsupported")
            }
            Self::DescriptorExportFailed => {
                formatter.write_str("the current framebuffer cannot be exported as DMA-BUF")
            }
        }
    }
}

impl std::error::Error for KmsDrmError {}

#[derive(Debug)]
pub struct KmsExportedFrame {
    pub width: u32,
    pub height: u32,
    pub drm_fourcc: u32,
    pub modifier: u64,
    pub offset: u32,
    pub stride: u32,
    pub objects: Vec<OwnedFd>,
}

pub struct KmsDrmCapture {
    _library: DynamicLibrary,
    functions: DrmFunctions,
    device: File,
    crtc_id: u32,
    primary_plane_id: u32,
    width: u32,
    height: u32,
}

// The capture owns its DRM file and dynamic-library handle and is moved to one
// Replay worker. No native pointer escapes or is accessed concurrently.
// SAFETY: all native access remains serialized through `&self` on that worker.
unsafe impl Send for KmsDrmCapture {}

impl KmsDrmCapture {
    /// Opens exactly `/dev/dri/cardN` and binds the session to one connected
    /// connector and its current primary plane.
    ///
    /// # Errors
    ///
    /// Returns a stage-specific error if the bounded selection, libdrm,
    /// device, connector, CRTC, or primary plane cannot be validated.
    pub fn open(card_index: u8, connector_name: &str) -> Result<Self, KmsDrmError> {
        if card_index > 15 || !valid_connector_name(connector_name) {
            return Err(KmsDrmError::InvalidSelection);
        }
        let library = DynamicLibrary::open()?;
        let functions = DrmFunctions::load(&library)?;
        let path = PathBuf::from(format!("/dev/dri/card{card_index}"));
        let device = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| KmsDrmError::DeviceUnavailable)?;
        let fd = std::os::fd::AsRawFd::as_raw_fd(&device);
        // SAFETY: functions were resolved from libdrm and `fd` is a live DRM file.
        if unsafe { (functions.is_kms)(fd) } != 1 {
            return Err(KmsDrmError::KmsUnavailable);
        }
        // This changes only per-file descriptor object visibility; it does not
        // change display state or any KMS object property.
        // SAFETY: the capability and value are defined by the public DRM UAPI.
        if unsafe { (functions.set_client_cap)(fd, DRM_CLIENT_CAP_UNIVERSAL_PLANES, 1) } != 0 {
            return Err(KmsDrmError::PrimaryPlaneUnavailable);
        }
        let selection = unsafe { select_output(fd, connector_name, functions) }?;
        Ok(Self {
            _library: library,
            functions,
            device,
            crtc_id: selection.crtc_id,
            primary_plane_id: selection.primary_plane_id,
            width: selection.width,
            height: selection.height,
        })
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Exports the currently scanned-out primary framebuffer as DMA-BUF.
    ///
    /// # Errors
    ///
    /// Fails closed if the connector, CRTC, plane, dimensions, packed RGB
    /// format, modifier metadata, or descriptor export changed unexpectedly.
    pub fn acquire_frame(&self) -> Result<KmsExportedFrame, KmsDrmError> {
        let fd = std::os::fd::AsRawFd::as_raw_fd(&self.device);
        // SAFETY: all IDs were selected from this live DRM file and returned
        // objects are guarded and freed before this call returns.
        unsafe { acquire_frame(fd, self, self.functions) }
    }
}

fn valid_connector_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(Clone, Copy)]
struct DrmSelection {
    crtc_id: u32,
    primary_plane_id: u32,
    width: u32,
    height: u32,
}

unsafe fn select_output(
    fd: c_int,
    requested_name: &str,
    functions: DrmFunctions,
) -> Result<DrmSelection, KmsDrmError> {
    let resources = ResourceGuard::new(unsafe { (functions.get_resources)(fd) }, functions)
        .ok_or(KmsDrmError::OutputUnavailable)?;
    let resource = unsafe { resources.pointer.as_ref() }.ok_or(KmsDrmError::OutputUnavailable)?;
    let connector_count = bounded_count(resource.count_connectors)?;
    let connector_ids = unsafe { bounded_slice(resource.connectors, connector_count) }?;
    for connector_id in connector_ids {
        let connector = ConnectorGuard::new(
            unsafe { (functions.get_connector)(fd, *connector_id) },
            functions,
        );
        let Some(connector) = connector else { continue };
        let Some(value) = (unsafe { connector.pointer.as_ref() }) else {
            continue;
        };
        if value.connection != DRM_MODE_CONNECTED
            || connector_label(value.connector_type, value.connector_type_id).as_deref()
                != Some(requested_name)
            || value.encoder_id == 0
        {
            continue;
        }
        let encoder = EncoderGuard::new(
            unsafe { (functions.get_encoder)(fd, value.encoder_id) },
            functions,
        )
        .ok_or(KmsDrmError::OutputUnavailable)?;
        let encoder = unsafe { encoder.pointer.as_ref() }.ok_or(KmsDrmError::OutputUnavailable)?;
        if encoder.crtc_id == 0 {
            return Err(KmsDrmError::OutputUnavailable);
        }
        let crtc = CrtcGuard::new(
            unsafe { (functions.get_crtc)(fd, encoder.crtc_id) },
            functions,
        )
        .ok_or(KmsDrmError::OutputUnavailable)?;
        let crtc = unsafe { crtc.pointer.as_ref() }.ok_or(KmsDrmError::OutputUnavailable)?;
        if crtc.mode_valid == 0 || crtc.width == 0 || crtc.height == 0 {
            return Err(KmsDrmError::OutputUnavailable);
        }
        let primary_plane_id = unsafe { find_primary_plane(fd, encoder.crtc_id, functions) }?;
        return Ok(DrmSelection {
            crtc_id: encoder.crtc_id,
            primary_plane_id,
            width: crtc.width,
            height: crtc.height,
        });
    }
    Err(KmsDrmError::OutputUnavailable)
}

unsafe fn find_primary_plane(
    fd: c_int,
    crtc_id: u32,
    functions: DrmFunctions,
) -> Result<u32, KmsDrmError> {
    let resources =
        PlaneResourcesGuard::new(unsafe { (functions.get_plane_resources)(fd) }, functions)
            .ok_or(KmsDrmError::PrimaryPlaneUnavailable)?;
    let resources =
        unsafe { resources.pointer.as_ref() }.ok_or(KmsDrmError::PrimaryPlaneUnavailable)?;
    let count = usize::try_from(resources.count_planes)
        .ok()
        .filter(|count| *count <= MAX_DRM_OBJECTS)
        .ok_or(KmsDrmError::PrimaryPlaneUnavailable)?;
    let ids = unsafe { bounded_slice(resources.planes, count) }?;
    for plane_id in ids {
        let plane = PlaneGuard::new(unsafe { (functions.get_plane)(fd, *plane_id) }, functions);
        let Some(plane) = plane else { continue };
        let Some(plane) = (unsafe { plane.pointer.as_ref() }) else {
            continue;
        };
        if plane.crtc_id == crtc_id
            && plane.fb_id != 0
            && unsafe { plane_is_primary(fd, plane.plane_id, functions) }
        {
            return Ok(plane.plane_id);
        }
    }
    Err(KmsDrmError::PrimaryPlaneUnavailable)
}

unsafe fn plane_is_primary(fd: c_int, plane_id: u32, functions: DrmFunctions) -> bool {
    let Some(properties) = ObjectPropertiesGuard::new(
        unsafe { (functions.object_get_properties)(fd, plane_id, DRM_MODE_OBJECT_PLANE) },
        functions,
    ) else {
        return false;
    };
    let Some(properties) = (unsafe { properties.pointer.as_ref() }) else {
        return false;
    };
    let Ok(count) = usize::try_from(properties.count_props) else {
        return false;
    };
    if count > MAX_DRM_OBJECTS {
        return false;
    }
    let Ok(ids) = (unsafe { bounded_slice(properties.props, count) }) else {
        return false;
    };
    let Ok(values) = (unsafe { bounded_slice(properties.prop_values, count) }) else {
        return false;
    };
    ids.iter().zip(values).any(|(property_id, value)| {
        let property = PropertyGuard::new(
            unsafe { (functions.get_property)(fd, *property_id) },
            functions,
        );
        property.is_some_and(|property| {
            let name = unsafe { &property.pointer.as_ref().expect("guarded property").name };
            c_name_equals(name, b"type") && *value == DRM_PLANE_TYPE_PRIMARY
        })
    })
}

unsafe fn acquire_frame(
    fd: c_int,
    capture: &KmsDrmCapture,
    functions: DrmFunctions,
) -> Result<KmsExportedFrame, KmsDrmError> {
    let plane = PlaneGuard::new(
        unsafe { (functions.get_plane)(fd, capture.primary_plane_id) },
        functions,
    )
    .ok_or(KmsDrmError::PrimaryPlaneUnavailable)?;
    let plane = unsafe { plane.pointer.as_ref() }.ok_or(KmsDrmError::PrimaryPlaneUnavailable)?;
    if plane.crtc_id != capture.crtc_id || plane.fb_id == 0 {
        return Err(KmsDrmError::PrimaryPlaneUnavailable);
    }
    let framebuffer =
        FramebufferGuard::new(unsafe { (functions.get_fb2)(fd, plane.fb_id) }, functions)
            .ok_or(KmsDrmError::FramebufferUnavailable)?;
    let framebuffer =
        unsafe { framebuffer.pointer.as_ref() }.ok_or(KmsDrmError::FramebufferUnavailable)?;
    if framebuffer.width != capture.width
        || framebuffer.height != capture.height
        || !matches!(
            framebuffer.pixel_format,
            DRM_FORMAT_XRGB8888 | DRM_FORMAT_ARGB8888 | DRM_FORMAT_XBGR8888 | DRM_FORMAT_ABGR8888
        )
        || framebuffer.handles[0] == 0
        || framebuffer.pitches[0] < capture.width.saturating_mul(4)
        || framebuffer.handles[1..].iter().any(|handle| *handle != 0)
        || framebuffer.pitches[1..].iter().any(|pitch| *pitch != 0)
        || framebuffer.offsets[1..].iter().any(|offset| *offset != 0)
    {
        return Err(KmsDrmError::FramebufferLayoutUnsupported);
    }
    let mut prime_fd = -1;
    if unsafe {
        (functions.prime_handle_to_fd)(
            fd,
            framebuffer.handles[0],
            DRM_CLOEXEC | DRM_RDWR,
            &raw mut prime_fd,
        )
    } != 0
        || prime_fd < 0
    {
        return Err(KmsDrmError::DescriptorExportFailed);
    }
    // SAFETY: successful drmPrimeHandleToFD returned one new owned descriptor.
    let object = unsafe { OwnedFd::from_raw_fd(prime_fd) };
    Ok(KmsExportedFrame {
        width: framebuffer.width,
        height: framebuffer.height,
        drm_fourcc: framebuffer.pixel_format,
        modifier: framebuffer.modifier,
        offset: framebuffer.offsets[0],
        stride: framebuffer.pitches[0],
        objects: vec![object],
    })
}

fn c_name_equals(name: &[c_char; 32], expected: &[u8]) -> bool {
    let bytes = name.map(i8::cast_unsigned);
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    &bytes[..length] == expected
}

fn connector_label(connector_type: u32, type_id: u32) -> Option<String> {
    let name = match connector_type {
        1 => "VGA",
        2 => "DVI-I",
        3 => "DVI-D",
        4 => "DVI-A",
        7 => "LVDS",
        10 => "DP",
        11 => "HDMI-A",
        12 => "HDMI-B",
        14 => "eDP",
        15 => "Virtual",
        16 => "DSI",
        20 => "USB",
        _ => return None,
    };
    Some(format!("{name}-{type_id}"))
}

fn bounded_count(value: c_int) -> Result<usize, KmsDrmError> {
    usize::try_from(value)
        .ok()
        .filter(|count| *count <= MAX_DRM_OBJECTS)
        .ok_or(KmsDrmError::OutputUnavailable)
}

unsafe fn bounded_slice<'a, T>(pointer: *const T, length: usize) -> Result<&'a [T], KmsDrmError> {
    if length == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() || length > MAX_DRM_OBJECTS {
        return Err(KmsDrmError::OutputUnavailable);
    }
    // SAFETY: libdrm owns at least `length` entries for the lifetime of the guard.
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

#[repr(C)]
struct DrmModeRes {
    count_fbs: c_int,
    fbs: *mut u32,
    count_crtcs: c_int,
    crtcs: *mut u32,
    count_connectors: c_int,
    connectors: *mut u32,
    count_encoders: c_int,
    encoders: *mut u32,
    min_width: u32,
    max_width: u32,
    min_height: u32,
    max_height: u32,
}

#[repr(C)]
struct DrmModeConnector {
    connector_id: u32,
    encoder_id: u32,
    connector_type: u32,
    connector_type_id: u32,
    connection: u32,
    mm_width: u32,
    mm_height: u32,
    subpixel: u32,
    count_modes: c_int,
    modes: *mut c_void,
    count_props: c_int,
    props: *mut u32,
    prop_values: *mut u64,
    count_encoders: c_int,
    encoders: *mut u32,
}

#[repr(C)]
struct DrmModeEncoder {
    encoder_id: u32,
    encoder_type: u32,
    crtc_id: u32,
    possible_crtcs: u32,
    possible_clones: u32,
}

#[repr(C)]
struct DrmModeModeInfo {
    clock: u32,
    timing: [u16; 10],
    vrefresh: u32,
    flags: u32,
    mode_type: u32,
    name: [c_char; 32],
}

#[repr(C)]
struct DrmModeCrtc {
    crtc_id: u32,
    buffer_id: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    mode_valid: c_int,
    mode: DrmModeModeInfo,
    gamma_size: c_int,
}

#[repr(C)]
struct DrmModePlane {
    count_formats: u32,
    formats: *mut u32,
    plane_id: u32,
    crtc_id: u32,
    fb_id: u32,
    crtc_x: u32,
    crtc_y: u32,
    x: u32,
    y: u32,
    possible_crtcs: u32,
    gamma_size: u32,
}

#[repr(C)]
struct DrmModePlaneRes {
    count_planes: u32,
    planes: *mut u32,
}

#[repr(C)]
struct DrmModeObjectProperties {
    count_props: u32,
    props: *mut u32,
    prop_values: *mut u64,
}

#[repr(C)]
struct DrmModeProperty {
    prop_id: u32,
    flags: u32,
    name: [c_char; 32],
    count_values: c_int,
    values: *mut u64,
    count_enums: c_int,
    enums: *mut c_void,
    count_blobs: c_int,
    blob_ids: *mut u32,
}

#[repr(C)]
struct DrmModeFb2 {
    fb_id: u32,
    width: u32,
    height: u32,
    pixel_format: u32,
    modifier: u64,
    flags: u32,
    handles: [u32; 4],
    pitches: [u32; 4],
    offsets: [u32; 4],
}

type IsKms = unsafe extern "C" fn(c_int) -> c_int;
type SetClientCap = unsafe extern "C" fn(c_int, u64, u64) -> c_int;
type GetResources = unsafe extern "C" fn(c_int) -> *mut DrmModeRes;
type FreeResources = unsafe extern "C" fn(*mut DrmModeRes);
type GetConnector = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeConnector;
type FreeConnector = unsafe extern "C" fn(*mut DrmModeConnector);
type GetEncoder = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeEncoder;
type FreeEncoder = unsafe extern "C" fn(*mut DrmModeEncoder);
type GetCrtc = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeCrtc;
type FreeCrtc = unsafe extern "C" fn(*mut DrmModeCrtc);
type GetPlaneResources = unsafe extern "C" fn(c_int) -> *mut DrmModePlaneRes;
type FreePlaneResources = unsafe extern "C" fn(*mut DrmModePlaneRes);
type GetPlane = unsafe extern "C" fn(c_int, u32) -> *mut DrmModePlane;
type FreePlane = unsafe extern "C" fn(*mut DrmModePlane);
type ObjectGetProperties = unsafe extern "C" fn(c_int, u32, u32) -> *mut DrmModeObjectProperties;
type FreeObjectProperties = unsafe extern "C" fn(*mut DrmModeObjectProperties);
type GetProperty = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeProperty;
type FreeProperty = unsafe extern "C" fn(*mut DrmModeProperty);
type GetFb2 = unsafe extern "C" fn(c_int, u32) -> *mut DrmModeFb2;
type FreeFb2 = unsafe extern "C" fn(*mut DrmModeFb2);
type PrimeHandleToFd = unsafe extern "C" fn(c_int, u32, u32, *mut c_int) -> c_int;

#[derive(Clone, Copy)]
struct DrmFunctions {
    is_kms: IsKms,
    set_client_cap: SetClientCap,
    get_resources: GetResources,
    free_resources: FreeResources,
    get_connector: GetConnector,
    free_connector: FreeConnector,
    get_encoder: GetEncoder,
    free_encoder: FreeEncoder,
    get_crtc: GetCrtc,
    free_crtc: FreeCrtc,
    get_plane_resources: GetPlaneResources,
    free_plane_resources: FreePlaneResources,
    get_plane: GetPlane,
    free_plane: FreePlane,
    object_get_properties: ObjectGetProperties,
    free_object_properties: FreeObjectProperties,
    get_property: GetProperty,
    free_property: FreeProperty,
    get_fb2: GetFb2,
    free_fb2: FreeFb2,
    prime_handle_to_fd: PrimeHandleToFd,
}

impl DrmFunctions {
    fn load(library: &DynamicLibrary) -> Result<Self, KmsDrmError> {
        macro_rules! load {
            ($name:literal, $ty:ty) => {{
                let pointer = library.symbol(concat!($name, "\0"), $name)?;
                // SAFETY: the public libdrm symbol fixes the exact C ABI.
                unsafe { mem::transmute::<*mut c_void, $ty>(pointer) }
            }};
        }
        Ok(Self {
            is_kms: load!("drmIsKMS", IsKms),
            set_client_cap: load!("drmSetClientCap", SetClientCap),
            get_resources: load!("drmModeGetResources", GetResources),
            free_resources: load!("drmModeFreeResources", FreeResources),
            get_connector: load!("drmModeGetConnector", GetConnector),
            free_connector: load!("drmModeFreeConnector", FreeConnector),
            get_encoder: load!("drmModeGetEncoder", GetEncoder),
            free_encoder: load!("drmModeFreeEncoder", FreeEncoder),
            get_crtc: load!("drmModeGetCrtc", GetCrtc),
            free_crtc: load!("drmModeFreeCrtc", FreeCrtc),
            get_plane_resources: load!("drmModeGetPlaneResources", GetPlaneResources),
            free_plane_resources: load!("drmModeFreePlaneResources", FreePlaneResources),
            get_plane: load!("drmModeGetPlane", GetPlane),
            free_plane: load!("drmModeFreePlane", FreePlane),
            object_get_properties: load!("drmModeObjectGetProperties", ObjectGetProperties),
            free_object_properties: load!("drmModeFreeObjectProperties", FreeObjectProperties),
            get_property: load!("drmModeGetProperty", GetProperty),
            free_property: load!("drmModeFreeProperty", FreeProperty),
            get_fb2: load!("drmModeGetFB2", GetFb2),
            free_fb2: load!("drmModeFreeFB2", FreeFb2),
            prime_handle_to_fd: load!("drmPrimeHandleToFD", PrimeHandleToFd),
        })
    }
}

struct DynamicLibrary(*mut c_void);

impl DynamicLibrary {
    fn open() -> Result<Self, KmsDrmError> {
        let name = b"libdrm.so.2\0";
        // SAFETY: the static name is NUL-terminated and requests immediate binding.
        let handle = unsafe { dlopen(name.as_ptr().cast(), 2) };
        if handle.is_null() {
            Err(KmsDrmError::LibraryUnavailable)
        } else {
            Ok(Self(handle))
        }
    }

    fn symbol(&self, name: &'static str, label: &'static str) -> Result<*mut c_void, KmsDrmError> {
        // SAFETY: `self` owns a live dlopen handle and `name` is NUL-terminated.
        let symbol = unsafe { dlsym(self.0, name.as_ptr().cast()) };
        if symbol.is_null() {
            Err(KmsDrmError::SymbolUnavailable(label))
        } else {
            Ok(symbol)
        }
    }
}

impl Drop for DynamicLibrary {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: this handle was returned by dlopen and is closed once.
            unsafe { dlclose(self.0) };
            self.0 = ptr::null_mut();
        }
    }
}

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

macro_rules! guard {
    ($name:ident, $value:ty, $free:ident) => {
        struct $name {
            pointer: *mut $value,
            functions: DrmFunctions,
        }
        impl $name {
            fn new(pointer: *mut $value, functions: DrmFunctions) -> Option<Self> {
                (!pointer.is_null()).then_some(Self { pointer, functions })
            }
        }
        impl Drop for $name {
            fn drop(&mut self) {
                if !self.pointer.is_null() {
                    // SAFETY: this pointer came from the matching libdrm allocator.
                    unsafe { (self.functions.$free)(self.pointer) };
                }
            }
        }
    };
}

guard!(ResourceGuard, DrmModeRes, free_resources);
guard!(ConnectorGuard, DrmModeConnector, free_connector);
guard!(EncoderGuard, DrmModeEncoder, free_encoder);
guard!(CrtcGuard, DrmModeCrtc, free_crtc);
guard!(PlaneResourcesGuard, DrmModePlaneRes, free_plane_resources);
guard!(PlaneGuard, DrmModePlane, free_plane);
guard!(
    ObjectPropertiesGuard,
    DrmModeObjectProperties,
    free_object_properties
);
guard!(PropertyGuard, DrmModeProperty, free_property);
guard!(FramebufferGuard, DrmModeFb2, free_fb2);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connector_names_are_bounded_and_generated_without_external_text() {
        assert!(valid_connector_name("DP-2"));
        assert!(!valid_connector_name("../card0"));
        assert!(!valid_connector_name(&"D".repeat(65)));
        assert_eq!(connector_label(10, 2).as_deref(), Some("DP-2"));
        assert_eq!(connector_label(11, 1).as_deref(), Some("HDMI-A-1"));
        assert_eq!(connector_label(999, 1), None);
    }

    #[test]
    fn libdrm_abi_layouts_match_the_downloaded_public_header_contract() {
        assert_eq!(mem::size_of::<DrmModeFb2>(), 80);
        assert_eq!(mem::size_of::<DrmModeEncoder>(), 20);
        assert_eq!(mem::size_of::<DrmModePlane>(), 56);
        assert_eq!(mem::size_of::<DrmModeCrtc>(), 100);
        assert_eq!(mem::size_of::<DrmModeConnector>(), 88);
        assert_eq!(mem::size_of::<DrmModeRes>(), 80);
        assert_eq!(mem::size_of::<DrmModeProperty>(), 88);
        assert_eq!(mem::size_of::<DrmModeObjectProperties>(), 24);
        assert_eq!(mem::size_of::<[u32; 4]>(), 16);
    }
}
