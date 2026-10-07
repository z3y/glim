use libloading::Library;
use std::{
    ffi::{CStr, c_char, c_void},
    ptr,
};

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

#[repr(C)]
pub struct OIDNBufferImpl(c_void);
pub type OIDNBuffer = *mut OIDNBufferImpl;

type FnNewBuffer = unsafe extern "C" fn(OIDNDevice, usize) -> OIDNBuffer;
type FnReleaseBuffer = unsafe extern "C" fn(OIDNBuffer);
type FnWriteBuffer = unsafe extern "C" fn(OIDNBuffer, usize, usize, *const c_void);
type FnReadBuffer = unsafe extern "C" fn(OIDNBuffer, usize, usize, *mut c_void);
type FnSyncDevice = unsafe extern "C" fn(OIDNDevice);
type FnSetFilterImage = unsafe extern "C" fn(
    OIDNFilter,
    *const c_char,
    OIDNBuffer,
    OIDNFormat,
    usize, // width
    usize, // height
    usize, // byteOffset
    usize, // pixelByteStride
    usize, // rowByteStride
);

type FnGetDeviceInt = unsafe extern "C" fn(OIDNDevice, *const c_char) -> i32;

#[allow(dead_code)]
pub struct Oidn {
    _lib: Library,
    release_device: FnReleaseDevice,
    commit_filter: FnCommitFilter,
    execute_filter: FnExecuteFilter,
    release_filter: FnReleaseFilter,
    set_filter_bool: FnSetFilterBool,
    get_device_error: FnGetDeviceError,
    new_buffer: FnNewBuffer,
    release_buffer: FnReleaseBuffer,
    write_buffer: FnWriteBuffer,
    read_buffer: FnReadBuffer,
    sync_device: FnSyncDevice,
    set_filter_image: FnSetFilterImage,
    get_device_int: FnGetDeviceInt,

    device: OIDNDevice,
    filter: OIDNFilter,

    // reusable device-accessible buffer shared by all lightmaps
    buffer: OIDNBuffer,
    buffer_size: usize,
}
impl Oidn {
    pub fn load() -> Result<Self, libloading::Error> {
        let lib_name = if cfg!(windows) {
            "OpenImageDenoise.dll"
        } else if cfg!(target_os = "macos") {
            "libOpenImageDenoise.2.dylib"
        } else {
            "libOpenImageDenoise.so.2"
        };

        let lib_path = if let Ok(root) = std::env::var("OpenImageDenoise_DIR") {
            std::path::Path::new(&root).join("bin").join(lib_name)
        } else if cfg!(target_os = "macos") {
            [
                "/opt/homebrew/lib", // Apple Silicon Homebrew
                "/usr/local/lib",    // Intel Homebrew
            ]
            .iter()
            .map(|dir| std::path::Path::new(dir).join(lib_name))
            .find(|p| p.exists())
            .unwrap_or_else(|| std::path::Path::new(lib_name).to_path_buf())
        } else {
            std::path::Path::new(lib_name).to_path_buf()
        };

        unsafe {
            let lib = Library::new(&lib_path)?;

            println!("Loading oidn from {:?}", &lib_path);

            let new_device: FnNewDevice = *lib.get(b"oidnNewDevice\0")?;
            let commit_device: FnCommitDevice = *lib.get(b"oidnCommitDevice\0")?;
            let new_filter: FnNewFilter = *lib.get(b"oidnNewFilter\0")?;

            let device = new_device(OIDNDeviceType::Default);
            commit_device(device);
            let filter = new_filter(device, c"RTLightmap".as_ptr());

            Ok(Self {
                release_device: *lib.get(b"oidnReleaseDevice\0")?,
                commit_filter: *lib.get(b"oidnCommitFilter\0")?,
                execute_filter: *lib.get(b"oidnExecuteFilter\0")?,
                release_filter: *lib.get(b"oidnReleaseFilter\0")?,
                set_filter_bool: *lib.get(b"oidnSetFilterBool\0")?,
                get_device_error: *lib.get(b"oidnGetDeviceError\0")?,
                new_buffer: *lib.get(b"oidnNewBuffer\0")?,
                release_buffer: *lib.get(b"oidnReleaseBuffer\0")?,
                write_buffer: *lib.get(b"oidnWriteBuffer\0")?,
                read_buffer: *lib.get(b"oidnReadBuffer\0")?,
                sync_device: *lib.get(b"oidnSyncDevice\0")?,
                set_filter_image: *lib.get(b"oidnSetFilterImage\0")?,
                get_device_int: *lib.get(b"oidnGetDeviceInt\0")?,
                _lib: lib,
                device,
                filter,
                buffer: ptr::null_mut(),
                buffer_size: 0,
            })
        }
    }

    pub fn device_type(&self) -> OIDNDeviceType {
        let t = unsafe { (self.get_device_int)(self.device, c"type".as_ptr()) };
        match t {
            1 => OIDNDeviceType::CPU,
            2 => OIDNDeviceType::SYCL,
            3 => OIDNDeviceType::CUDA,
            4 => OIDNDeviceType::HIP,
            5 => OIDNDeviceType::METAL,
            _ => OIDNDeviceType::Default,
        }
    }

    pub fn reserve_buffer(&mut self, size: usize) -> bool {
        if size <= self.buffer_size && !self.buffer.is_null() {
            return true;
        }
        unsafe {
            if !self.buffer.is_null() {
                (self.release_buffer)(self.buffer);
                self.buffer = ptr::null_mut();
                self.buffer_size = 0;
            }
            let buf = (self.new_buffer)(self.device, size);
            if buf.is_null() {
                self.check_error();
                return false;
            }
            self.buffer = buf;
            self.buffer_size = size;
        }
        true
    }

    pub fn denoise(&self, pixels: &mut [f32], width: usize, height: usize, directional: bool) {
        let pixel_stride = 4 * std::mem::size_of::<f32>();
        let byte_size = width * height * pixel_stride;
        assert!(pixels.len() * std::mem::size_of::<f32>() >= byte_size);

        if self.buffer.is_null() {
            panic!("reserve_buffer first")
        }

        let filter = self.filter;

        unsafe {
            (self.write_buffer)(self.buffer, 0, byte_size, pixels.as_ptr() as *const c_void);

            (self.set_filter_image)(
                filter,
                c"color".as_ptr(),
                self.buffer,
                OIDNFormat::Float3,
                width,
                height,
                0,
                pixel_stride,
                0,
            );
            (self.set_filter_image)(
                filter,
                c"output".as_ptr(),
                self.buffer,
                OIDNFormat::Float3,
                width,
                height,
                0,
                pixel_stride,
                0,
            );

            (self.set_filter_bool)(filter, c"directional".as_ptr(), directional);

            (self.commit_filter)(filter);
            (self.execute_filter)(filter);
            (self.sync_device)(self.device);

            (self.read_buffer)(
                self.buffer,
                0,
                byte_size,
                pixels.as_mut_ptr() as *mut c_void,
            );
        }

        self.check_error();
    }

    fn check_error(&self) {
        unsafe {
            let mut msg: *const c_char = ptr::null();
            let err = (self.get_device_error)(self.device, &mut msg);
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
            if !self.buffer.is_null() {
                (self.release_buffer)(self.buffer);
            }
            (self.release_filter)(self.filter);
            (self.release_device)(self.device);
        }
    }
}
