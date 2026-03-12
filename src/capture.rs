use std::io;

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
    Ok(Box::new(ScrapCapturer::new()?))
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
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[cfg(target_os = "macos")]
fn ensure_macos_screen_capture_access() -> io::Result<()> {
    if unsafe { CGPreflightScreenCaptureAccess() } {
        return Ok(());
    }

    println!("[server] screen recording permission not granted; requesting access...");

    if unsafe { CGRequestScreenCaptureAccess() } {
        println!("[server] screen recording permission granted");
        return Ok(());
    }

    Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "screen recording permission denied; enable Screen Recording for Terminal or VS Code in System Settings -> Privacy & Security -> Screen Recording, then restart the app",
    ))
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
