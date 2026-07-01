use libloading::Library;
use std::ffi::{CStr, c_char, c_void};

#[repr(C)]
pub struct OIDNDeviceImpl(c_void);
#[repr(C)]
pub struct OIDNFilterImpl(c_void);
pub type OIDNDevice = *mut OIDNDeviceImpl;
pub type OIDNFilter = *mut OIDNFilterImpl;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OIDNDeviceType {
    Default = 0,
    CPU = 1,
    SYCL = 2,
    CUDA = 3,
    HIP = 4,
    METAL = 5,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OIDNError {
    None = 0,
    Unknown = 1,
    InvalidArgument = 2,
    InvalidOperation = 3,
    OutOfMemory = 4,
    UnsupportedHardware = 5,
    Cancelled = 6,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum OIDNFormat {
    Undefined = 0,
    Float = 1,
    Float2 = 2,
    Float3 = 3,
    Float4 = 4,
    Half = 257,
    Half2 = 258,
    Half3 = 259,
    Half4 = 260,
}

type FnNewDevice = unsafe extern "C" fn(OIDNDeviceType) -> OIDNDevice;
type FnCommitDevice = unsafe extern "C" fn(OIDNDevice);
type FnReleaseDevice = unsafe extern "C" fn(OIDNDevice);
type FnNewFilter = unsafe extern "C" fn(OIDNDevice, *const c_char) -> OIDNFilter;
type FnCommitFilter = unsafe extern "C" fn(OIDNFilter);
type FnExecuteFilter = unsafe extern "C" fn(OIDNFilter);
type FnReleaseFilter = unsafe extern "C" fn(OIDNFilter);
type FnSetFilterBool = unsafe extern "C" fn(OIDNFilter, *const c_char, bool);
type FnGetDeviceError = unsafe extern "C" fn(OIDNDevice, *mut *const c_char) -> OIDNError;
type FnSetSharedFilterImage = unsafe extern "C" fn(
    OIDNFilter,
    *const c_char,
    *mut c_void,
    OIDNFormat,
    usize,
    usize,
    usize,
    usize,
    usize,
);

#[allow(dead_code)]
pub struct Oidn {
    _lib: Library,
    release_device: FnReleaseDevice,
    commit_filter: FnCommitFilter,
    execute_filter: FnExecuteFilter,
    release_filter: FnReleaseFilter,
    set_filter_bool: FnSetFilterBool,
    get_device_error: FnGetDeviceError,
    set_shared_filter_image: FnSetSharedFilterImage,

    device: OIDNDevice,
    filter: OIDNFilter,
}

impl Oidn {
    pub fn load() -> Result<Self, libloading::Error> {
        let lib_name = if cfg!(windows) {
            "OpenImageDenoise.dll"
        } else if cfg!(target_os = "macos") {
            "libOpenImageDenoise.dylib"
        } else {
            "libOpenImageDenoise.so.2"
        };

        // todo proper oidn path
        let lib_path = if let Ok(root) = std::env::var("OpenImageDenoise_DIR") {
            std::path::Path::new(&root).join("bin").join(lib_name)
        } else {
            std::path::Path::new(lib_name).to_path_buf()
        };

        unsafe {
            let lib = Library::new(lib_path)?;

            let new_device: FnNewDevice = *lib.get(b"oidnNewDevice\0")?;
            let commit_device: FnCommitDevice = *lib.get(b"oidnCommitDevice\0")?;
            let new_filter: FnNewFilter = *lib.get(b"oidnNewFilter\0")?;

            // todo GPU device
            let device = new_device(OIDNDeviceType::CPU);
            commit_device(device);
            let filter = new_filter(device, c"RTLightmap".as_ptr());

            Ok(Self {
                release_device: *lib.get(b"oidnReleaseDevice\0")?,
                commit_filter: *lib.get(b"oidnCommitFilter\0")?,
                execute_filter: *lib.get(b"oidnExecuteFilter\0")?,
                release_filter: *lib.get(b"oidnReleaseFilter\0")?,
                set_filter_bool: *lib.get(b"oidnSetFilterBool\0")?,
                // .or_else(|_| lib.get(b"oidnSetFilter1b\0"))?,
                get_device_error: *lib.get(b"oidnGetDeviceError\0")?,
                set_shared_filter_image: *lib.get(b"oidnSetSharedFilterImage\0")?,
                _lib: lib,
                device,
                filter,
            })
        }
    }

    pub fn denoise(&self, pixels: &mut [f32], width: usize, height: usize) {
        let pixel_stride = 4 * std::mem::size_of::<f32>();

        let filter = self.filter;
        let device = self.device;

        unsafe {
            (self.set_shared_filter_image)(
                filter,
                c"color".as_ptr(),
                pixels.as_mut_ptr() as *mut c_void,
                OIDNFormat::Float3,
                width,
                height,
                0,
                pixel_stride,
                0,
            );
            (self.set_shared_filter_image)(
                filter,
                c"output".as_ptr(),
                pixels.as_mut_ptr() as *mut c_void,
                OIDNFormat::Float3,
                width,
                height,
                0,
                pixel_stride,
                0,
            );

            (self.commit_filter)(filter);
            (self.execute_filter)(filter);

            let mut msg: *const c_char = std::ptr::null();
            let err = (self.get_device_error)(device, &mut msg);
            if err != OIDNError::None {
                let s = if msg.is_null() {
                    "unknown error".into()
                } else {
                    CStr::from_ptr(msg).to_string_lossy()
                };
                eprintln!("OIDN error {:?}: {}", err, s);
            }
        }
    }
}

impl Drop for Oidn {
    fn drop(&mut self) {
        unsafe {
            (self.release_filter)(self.filter);
            (self.release_device)(self.device);
        }
    }
}
