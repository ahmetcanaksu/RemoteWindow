use crate::capture::DisplayInfo;
use crate::capture::ScreenCapturer;
use std::ffi::{c_char, c_void, CStr};
use std::io;

#[repr(C)] // Crucial: Tells Rust to use C-style memory alignment
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    unsafe fn CGGetActiveDisplayList(
        max_displays: u32,
        active_displays: *mut u32,
        display_count: *mut u32,
    ) -> i32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGDisplayPixelsWide(display: u32) -> usize;
    fn CGDisplayPixelsHigh(display: u32) -> usize;

    fn CGDisplayCreateImage(display_id: u32) -> *mut usize;
    fn CGImageGetDataProvider(image: *mut usize) -> *mut usize;
    fn CGDataProviderCopyData(provider: *mut usize) -> *mut usize;
    fn CGImageGetWidth(image: *mut usize) -> usize;
    fn CGImageGetHeight(image: *mut usize) -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDataGetBytePtr(data: *mut usize) -> *const u8;
    fn CFDataGetLength(data: *mut usize) -> isize;
    fn CFRelease(obj: *mut usize);
}

pub fn get_monitors_raw() -> Vec<DisplayInfo> {
    let max_displays = 16u32;
    let mut display_ids = vec![0u32; max_displays as usize];
    let mut count = 0u32;
    let mut displays = Vec::new();
    unsafe {
        // .as_mut_ptr() gives you the Raw Pointer (UnsafeMutablePointer)
        let result = CGGetActiveDisplayList(
            max_displays,
            display_ids.as_mut_ptr(),
            &mut count as *mut u32,
        );

        if result == 0 {
            println!("Found {} active monitors!", count);

            // Only iterate up to what Apple actually found
            for i in 0..count as usize {
                let display_id = display_ids[i];
                // let bounds = CGDisplayBounds(display_id);
                let width = CGDisplayPixelsWide(display_id);
                let height = CGDisplayPixelsHigh(display_id);

                displays.push(DisplayInfo {
                    id: display_id,
                    width,
                    height,
                });
            }
        } else {
            eprintln!("Error calling Apple API: {}", result);
        }
    }
    displays
}

pub fn capture_frame_into_macos(display_id: u32, target_buffer: &mut [u8]) -> bool {
    unsafe {
        // 1. Take the snapshot
        let image = CGDisplayCreateImage(display_id);
        if image.is_null() {
            return false;
        }

        // 2. Access the data provider
        let provider = CGImageGetDataProvider(image);
        let data = CGDataProviderCopyData(provider);
        if data.is_null() {
            CFRelease(image as *mut usize);
            return false;
        }

        // 3. Get the raw pointer and length
        let ptr = CFDataGetBytePtr(data);
        let len = CFDataGetLength(data) as usize;

        // 4. Safety Check & Copy
        // Note: macOS usually returns BGRA or RGBA.
        // If the buffer is 1920x1080, len should be 8,294,400 bytes.
        if target_buffer.len() >= len {
            std::ptr::copy_nonoverlapping(ptr, target_buffer.as_mut_ptr(), len);
        }

        // 5. Cleanup - CRITICAL to prevent RAM ballooning
        CFRelease(data as *mut usize);
        CFRelease(image as *mut usize);

        true
    }
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

pub struct SwiftCapturer {
    handle: *mut c_void,
    width: u32,
    height: u32,
}

impl SwiftCapturer {
    pub fn new() -> io::Result<Self> {
        Self::new_with_display_index(0)
    }

    pub fn new_with_display_index(display_index: u32) -> io::Result<Self> {
        ensure_macos_screen_capture_access()?;

        let mut width = 0_u32;
        let mut height = 0_u32;
        let mut error: *mut c_char = std::ptr::null_mut();

        println!("[server] initializing ScreenCaptureKit backend via Swift bridge");

        let handle = unsafe { rw_sc_create(display_index, &mut width, &mut height, &mut error) };
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

impl Drop for SwiftCapturer {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { rw_sc_destroy(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

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
