use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::Controls::{TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOG_BUTTON};
use windows::Win32::UI::WindowsAndMessaging::IDOK;
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;

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

pub fn get_system_monitors() -> Vec<(u32, u32, i32, i32)> {
    get_monitors()
        .into_iter()
        .map(|m| (m.width as u32, m.height as u32, m.x, m.y))
        .collect()
}

pub fn select_monitor_dialog(monitors: &[MonitorData]) -> usize {
    // 1. Convert your monitor list into native Windows buttons
    let mut buttons: Vec<TASKDIALOG_BUTTON> = Vec::new();
    let mut button_labels: Vec<Vec<u16>> = Vec::new();

    for (i, mon) in monitors.iter().enumerate() {
        let label = format!(
            "Monitor {}: {}x{} ({})\0",
            i,
            mon.width,
            mon.height,
            if mon.is_primary {
                "Primary"
            } else {
                "Extended"
            }
        );
        let utf16: Vec<u16> = label.encode_utf16().collect();

        buttons.push(TASKDIALOG_BUTTON {
            nButtonID: (i + 100) as i32, // ID to identify which was clicked
            pszButtonText: PCWSTR(utf16.as_ptr()),
        });
        button_labels.push(utf16); // Keep alive until dialog closes
    }

    let config = TASKDIALOGCONFIG {
        cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
        pszWindowTitle: w!("AhmedoViewer - Display Selection"),
        pszMainInstruction: w!("Which display would you like to share?"),
        pszContent: w!("Select a monitor to begin the high-speed UDP stream."),
        pButtons: buttons.as_ptr(),
        cButtons: buttons.len() as u32,
        ..Default::default()
    };

    let mut selected_id = 0;
    unsafe {
        TaskDialogIndirect(&config, Some(&mut selected_id), None, None).unwrap();
    }

    // Convert back to index
    (selected_id - 100) as usize
}

/* if dxgi_capturer.capture_frame_into(&mut raw_frame) {
    static mut SAVED: bool = false;
    unsafe {
        if !SAVED {
            let is_all_zeros = raw_frame.iter().all(|&x| x == 0);
            if is_all_zeros {
                println!(
                    "[ERROR] The captured buffer is completely empty (all zeros)!"
                );
            } else {
                let mut png_buffer = raw_frame.clone();
                // Manual BGRA -> RGBA swap
                for chunk in png_buffer.chunks_exact_mut(4) {
                    chunk.swap(0, 2);
                }

                match image::save_buffer(
                    "server_debug.png",
                    &png_buffer,
                    w as u32,
                    h as u32,
                    image::ExtendedColorType::Rgba8,
                ) {
                    Ok(_) => {
                        println!(
                            "SUCCESS: PNG saved ({} bytes)",
                            std::fs::metadata("server_debug.png").unwrap().len()
                        );
                        SAVED = true;
                    }
                    Err(e) => println!("[ERROR] Failed to save PNG: {:?}", e),
                }
                SAVED = true;
            }
        }
    }

    //let raw_bytes: &[u8] = bytemuck::cast_slice(&frame);
    // Store the new frame. This is an atomic pointer swap.
    // The old frame will be dropped automatically when no one is reading it.
    //frame_size = raw_bytes.len();
    producer_handle.store(Arc::new(raw_frame));
} else {
    eprintln!("[t1]: Error capturing frame");
    thread::sleep(Duration::from_secs(1));
} */
