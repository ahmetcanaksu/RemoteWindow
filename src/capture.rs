use std::io;

#[cfg(target_os = "macos")]
use std::ffi::{c_char, c_void, CStr};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

pub trait ScreenCapturer {
    fn geometry(&self) -> (u32, u32);
    fn capture_frame(&mut self) -> io::Result<Vec<u32>>;
}

pub fn create_default_capturer() -> io::Result<Box<dyn ScreenCapturer>> {
    platform_create_default_capturer()
}

#[cfg(target_os = "linux")]
fn platform_create_default_capturer() -> io::Result<Box<dyn ScreenCapturer>> {
    Ok(Box::new(LinuxCapturer::new()?))
}

#[cfg(target_os = "macos")]
fn platform_create_default_capturer() -> io::Result<Box<dyn ScreenCapturer>> {
    match macos_capture_backend_from_env() {
        MacosCaptureBackend::Auto => match SwiftCapturer::new() {
            Ok(capturer) => Ok(Box::new(capturer)),
            Err(err) => {
                println!("[server] Swift ScreenCaptureKit backend failed: {}", err);
                println!("[server] falling back to scrap backend");
                Ok(Box::new(ScrapCapturer::new()?))
            }
        },
        MacosCaptureBackend::Swift => Ok(Box::new(SwiftCapturer::new()?)),
        MacosCaptureBackend::Scrap => Ok(Box::new(ScrapCapturer::new()?)),
    }
}

#[cfg(target_os = "windows")]
fn platform_create_default_capturer() -> io::Result<Box<dyn ScreenCapturer>> {
    Ok(Box::new(ScrapCapturer::new()?))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn platform_create_default_capturer() -> io::Result<Box<dyn ScreenCapturer>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "screen capture is not supported on this operating system",
    ))
}

#[cfg(target_os = "linux")]
struct LinuxCapturer {
    inner: captrs::Capturer,
    width: u32,
    height: u32,
}

#[cfg(target_os = "linux")]
impl LinuxCapturer {
    fn new() -> io::Result<Self> {
        let inner = captrs::Capturer::new(0)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let (width, height) = inner.geometry();

        Ok(Self {
            inner,
            width,
            height,
        })
    }
}

#[cfg(target_os = "linux")]
impl ScreenCapturer for LinuxCapturer {
    fn geometry(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn capture_frame(&mut self) -> io::Result<Vec<u32>> {
        let frame = self
            .inner
            .capture_frame()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let mut pixels = Vec::with_capacity(frame.len());

        for captrs::Bgr8 { r, g, b, .. } in &frame {
            pixels.push(((*r as u32) << 16) | ((*g as u32) << 8) | (*b as u32));
        }

        Ok(pixels)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
struct ScrapCapturer {
    inner: scrap::Capturer,
    width: u32,
    height: u32,
}

#[cfg(target_os = "macos")]
struct SwiftCapturer {
    handle: *mut c_void,
    width: u32,
    height: u32,
}

#[cfg(target_os = "macos")]
enum MacosCaptureBackend {
    Auto,
    Swift,
    Scrap,
}

#[cfg(target_os = "macos")]
fn macos_capture_backend_from_env() -> MacosCaptureBackend {
    let raw = crate::config::capture_backend();
    match raw.to_ascii_lowercase().as_str() {
        "auto" => MacosCaptureBackend::Auto,
        "scrap" => MacosCaptureBackend::Scrap,
        "swift" => MacosCaptureBackend::Swift,
        _ => {
            println!(
                "[server] unknown RW_CAPTURE_BACKEND={}; defaulting to auto",
                raw
            );
            MacosCaptureBackend::Auto
        }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "screen_capture_bridge", kind = "static")]
unsafe extern "C" {
    fn rw_sc_create(
        display_index: u32,
        out_width: *mut u32,
        out_height: *mut u32,
        out_error: *mut *mut c_char,
    ) -> *mut c_void;
    fn rw_sc_capture_frame(
        handle: *mut c_void,
        timeout_ms: u32,
        out_length: *mut usize,
        out_error: *mut *mut c_char,
    ) -> *mut c_void;
    fn rw_sc_destroy(handle: *mut c_void);
    fn rw_sc_free_frame(frame: *mut c_void);
    fn rw_sc_free_error(error: *mut c_char);
}

#[cfg(target_os = "macos")]
fn ensure_macos_screen_capture_access() -> io::Result<()> {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        println!("[server] screen recording permission already granted");
        return Ok(());
    }

    println!("[server] screen recording permission not granted; requesting access...");

    if unsafe { CGRequestScreenCaptureAccess() } {
        println!("[server] screen recording permission granted");
        return Ok(());
    } else {
        println!("[server] screen recording permission denied");
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "screen recording permission denied; enable Screen Recording for Terminal or VS Code in System Settings -> Privacy & Security -> Screen Recording, then restart the app",
    ))
}

#[cfg(target_os = "macos")]
fn take_swift_error(error: *mut c_char) -> io::Error {
    if error.is_null() {
        return io::Error::new(
            io::ErrorKind::Other,
            "screen capture bridge returned an unknown error",
        );
    }

    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { rw_sc_free_error(error) };

    let kind = if message.contains("timed out") {
        io::ErrorKind::TimedOut
    } else if message.contains("permission") {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::Other
    };

    io::Error::new(kind, message)
}

#[cfg(target_os = "macos")]
impl SwiftCapturer {
    fn new() -> io::Result<Self> {
        ensure_macos_screen_capture_access()?;

        let mut width = 0_u32;
        let mut height = 0_u32;
        let mut error: *mut c_char = std::ptr::null_mut();

        println!("[server] initializing ScreenCaptureKit backend via Swift bridge");

        let handle = unsafe { rw_sc_create(0, &mut width, &mut height, &mut error) };
        if handle.is_null() {
            return Err(take_swift_error(error));
        }

        println!(
            "[server] ScreenCaptureKit backend initialized successfully at {}x{}",
            width, height
        );

        Ok(Self {
            handle,
            width,
            height,
        })
    }
}

#[cfg(target_os = "macos")]
impl Drop for SwiftCapturer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { rw_sc_destroy(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

#[cfg(target_os = "macos")]
impl ScreenCapturer for SwiftCapturer {
    fn geometry(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn capture_frame(&mut self) -> io::Result<Vec<u32>> {
        let mut length = 0_usize;
        let mut error: *mut c_char = std::ptr::null_mut();
        let frame_ptr = unsafe { rw_sc_capture_frame(self.handle, 250, &mut length, &mut error) };

        if frame_ptr.is_null() {
            return Err(take_swift_error(error));
        }

        let expected_length = (self.width as usize) * (self.height as usize) * 4;
        let frame_bytes = unsafe { std::slice::from_raw_parts(frame_ptr as *const u8, length) };

        if frame_bytes.len() != expected_length {
            unsafe { rw_sc_free_frame(frame_ptr) };
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "ScreenCaptureKit frame size mismatch: got {} bytes, expected {}",
                    frame_bytes.len(),
                    expected_length
                ),
            ));
        }

        let mut pixels = Vec::with_capacity((self.width * self.height) as usize);
        for pixel in frame_bytes.chunks_exact(4) {
            let b = pixel[0] as u32;
            let g = pixel[1] as u32;
            let r = pixel[2] as u32;
            pixels.push((r << 16) | (g << 8) | b);
        }

        unsafe { rw_sc_free_frame(frame_ptr) };
        Ok(pixels)
    }
}

#[cfg(target_os = "macos")]
impl ScrapCapturer {
    fn new() -> io::Result<Self> {
        ensure_macos_screen_capture_access()?;

        let display = scrap::Display::primary()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let width = display.width() as u32;
        let height = display.height() as u32;
        println!(
            "[server] initializing scrap capturer for display {}x{}",
            width, height
        );
        let inner = scrap::Capturer::new(display)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        println!("[server] scrap capturer initialized successfully");

        Ok(Self {
            inner,
            width,
            height,
        })
    }
}

#[cfg(target_os = "windows")]
impl ScrapCapturer {
    fn new() -> io::Result<Self> {
        let display = scrap::Display::primary()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
        let width = display.width() as u32;
        let height = display.height() as u32;
        let inner = scrap::Capturer::new(display)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        Ok(Self {
            inner,
            width,
            height,
        })
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl ScreenCapturer for ScrapCapturer {
    fn geometry(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn capture_frame(&mut self) -> io::Result<Vec<u32>> {
        loop {
            match self.inner.frame() {
                Ok(frame) => {
                    let mut pixels = Vec::with_capacity((self.width * self.height) as usize);
                    for pixel in frame.chunks_exact(4) {
                        let b = pixel[0] as u32;
                        let g = pixel[1] as u32;
                        let r = pixel[2] as u32;
                        pixels.push((r << 16) | (g << 8) | b);
                    }
                    return Ok(pixels);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Err(e) => return Err(e),
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MonitorData {
    pub name: String,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
    pub x: i32,
    pub y: i32,
}

pub fn get_monitors() -> Vec<MonitorData> {
    let mut monitors = Vec::new();

    unsafe {
        // We pass a pointer to our Vec as the LPARAM so the callback can fill it
        let _ = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(monitor_enum_proc),
            LPARAM(&mut monitors as *mut Vec<MonitorData> as isize),
        );
    }

    monitors
}

unsafe extern "system" fn monitor_enum_proc(
    hmonitor: HMONITOR,
    _: HDC,
    _: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let monitors = &mut *(lparam.0 as *mut Vec<MonitorData>);

    // We use MONITORINFOEXW to also get the device name (like "\\.\DISPLAY1")
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    if GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
        let r = info.monitorInfo.rcMonitor;

        // Check if this is the DEFAULT (Primary) monitor
        // Correct way to check the flag
        let is_primary = (info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;
        monitors.push(MonitorData {
            name: String::from_utf16_lossy(&info.szDevice)
                .trim_matches(char::from(0))
                .to_string(),
            width: r.right - r.left,
            height: r.bottom - r.top,
            is_primary,
            x: r.left,
            y: r.top,
        });
    }

    BOOL::from(true)
}

pub fn getSystemMonitors() -> Vec<(u32, u32, i32, i32)> {
    get_monitors()
        .into_iter()
        .map(|m| (m.width as u32, m.height as u32, m.x, m.y))
        .collect()
}
