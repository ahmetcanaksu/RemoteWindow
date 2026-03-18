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
    use crate::macos::SwiftCapturer;
    match SwiftCapturer::new() {
        Ok(capturer) => Ok(Box::new(capturer)),
        Err(err) => {
            panic!(
                "[server] failed to initialize Swift-based capturer: {:?}\n\
                This likely means ScreenCaptureKit permissions were not granted.\n\
                Please ensure you have granted screen recording permissions to this application in System Preferences > Security & Privacy > Screen Recording.",
                err
            );
        }
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

#[derive(Debug, Copy, Clone)]
pub struct DisplayInfo {
    pub id: u32,
    pub width: usize,
    pub height: usize,
}

#[cfg(target_os = "macos")]
pub fn get_monitor_list() -> Vec<DisplayInfo> {
    use crate::macos::get_monitors_raw;
    return get_monitors_raw();
}

#[cfg(target_os = "windows")]
pub fn get_monitor_list() -> Vec<DisplayInfo> {
    getSystemMonitors()
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
