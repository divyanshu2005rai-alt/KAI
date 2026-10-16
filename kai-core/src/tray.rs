//! System-tray controls for the desktop application.
//!
//! The tray is an adapter over `AppState`; it does not own trigger, provider,
//! or injection behavior. The settings window uses the validated configuration
//! service and the OS credential-store boundary.

use std::sync::Arc;

use anyhow::{Context, Result};
use log::{error, info};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};
#[cfg(windows)]
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

use crate::settings::SettingsService;
use crate::state::AppState;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::ptr::null_mut;

#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MessageBoxW, TranslateMessage, MB_ICONINFORMATION,
    MB_ICONWARNING, MB_OK, MSG,
};

const PAUSE_ID: &str = "pause";
const STATUS_ID: &str = "status";
const CONFIGURE_ID: &str = "configure";
const CONFIG_ID: &str = "config";
const QUIT_ID: &str = "quit";

/// Starts the tray adapter and keeps the tray icon alive on its own thread.
pub fn start(state: Arc<AppState>) -> Result<()> {
    std::thread::Builder::new()
        .name("kai-tray".to_string())
        .spawn(move || {
            if let Err(error) = run(state) {
                error!("System tray stopped: {:#}", error);
            }
        })
        .context("failed to start system-tray thread")?;
    Ok(())
}

fn run(state: Arc<AppState>) -> Result<()> {
    let menu = Menu::new();
    let pause = MenuItem::with_id(PAUSE_ID, pause_menu_label(state.is_paused()), true, None);
    let status = MenuItem::with_id(STATUS_ID, "Status", true, None);
    let configure = MenuItem::with_id(CONFIGURE_ID, "Configure KAI", true, None);
    let config = MenuItem::with_id(CONFIG_ID, "Open configuration", true, None);
    let quit = MenuItem::with_id(QUIT_ID, "Quit KAI", true, None);

    menu.append(&pause)?;
    menu.append(&status)?;
    menu.append(&configure)?;
    menu.append(&config)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&quit)?;

    let icon = Icon::from_rgba(tray_icon_rgba().to_vec(), LOGO_SIZE, LOGO_SIZE)
        .context("failed to create KAI tray icon")?;
    let _tray = TrayIconBuilder::new()
        .with_tooltip("KAI")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("failed to create KAI tray icon")?;

    // Process events on this thread so the menu item can safely update its label.
    #[cfg(windows)]
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                handle_menu_event(event, &state, &pause);
            }
        }
    }

    #[cfg(not(windows))]
    loop {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            handle_menu_event(event, &state, &pause);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Ok(())
}

fn pause_menu_label(paused: bool) -> &'static str {
    if paused {
        "Resume KAI"
    } else {
        "Pause KAI"
    }
}

fn status_content(paused: bool, processing: bool) -> (&'static str, &'static str) {
    if paused {
        (
            "KAI | PAUSED",
            "KAI is paused.\n\nTrigger listening is off until you resume it.",
        )
    } else if processing {
        (
            "KAI | RUNNING",
            "KAI is running.\n\nTrigger listening is active, and a response is being processed.",
        )
    } else {
        (
            "KAI | RUNNING",
            "KAI is running.\n\nTrigger listening is active and ready.",
        )
    }
}

fn handle_menu_event(event: MenuEvent, state: &AppState, pause: &MenuItem) {
    match event.id.0.as_str() {
        PAUSE_ID => {
            let paused = state.toggle_paused();
            pause.set_text(pause_menu_label(paused));
            info!(
                "KAI {} from system tray",
                if paused { "paused" } else { "resumed" }
            );
        }
        STATUS_ID => show_status(state),
        CONFIGURE_ID => {
            if let Err(error) = launch_configure() {
                error!("Could not launch settings: {}", error);
            }
        }
        CONFIG_ID => {
            if let Err(error) = open_config() {
                error!("Could not open configuration: {}", error);
            }
        }
        QUIT_ID => std::process::exit(0),
        _ => {}
    }
}

#[cfg(windows)]
fn show_status(state: &AppState) {
    let paused = state.is_paused();
    let (title, message) = status_content(paused, state.is_processing());
    let icon = if paused {
        MB_ICONWARNING
    } else {
        MB_ICONINFORMATION
    };
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let message: Vec<u16> = message.encode_utf16().chain(Some(0)).collect();
    unsafe {
        MessageBoxW(null_mut(), message.as_ptr(), title.as_ptr(), MB_OK | icon);
    }
}

#[cfg(not(windows))]
fn show_status(state: &AppState) {
    info!(
        "KAI status: {}, processing: {}",
        if state.is_paused() {
            "paused"
        } else {
            "running"
        },
        state.is_processing()
    );
}

fn open_config() -> Result<()> {
    let path = SettingsService::path();
    std::process::Command::new("notepad.exe")
        .arg(path)
        .spawn()
        .context("failed to launch configuration editor")?;
    Ok(())
}

fn launch_configure() -> Result<()> {
    let executable = std::env::current_exe().context("failed to locate KAI executable")?;
    std::process::Command::new(executable)
        .arg("--configure-ui")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .context("failed to launch settings editor")?;
    Ok(())
}

const LOGO_SIZE: u32 = 64;

// Pre-rasterized at build time; no image-decoding dependency is needed at runtime.
fn tray_icon_rgba() -> &'static [u8] {
    include_bytes!("../assets/kai-manga-icon.rgba")
}

/// Creates a native Windows icon from the tray artwork for the Settings and
/// Trigger Editor title bars and taskbar buttons.
#[cfg(windows)]
pub fn create_logo_icon() -> windows_sys::Win32::UI::WindowsAndMessaging::HICON {
    use std::{ffi::c_void, ptr};

    const SIZE: i32 = LOGO_SIZE as i32;
    unsafe {
        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE,
            biHeight: -SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: (SIZE * SIZE * 4) as u32,
            ..Default::default()
        };

        let mut dib_pixels: *mut c_void = ptr::null_mut();
        let color_bitmap = CreateDIBSection(
            ptr::null_mut(),
            &bitmap_info,
            DIB_RGB_COLORS,
            &mut dib_pixels,
            ptr::null_mut(),
            0,
        );
        if color_bitmap.is_null() || dib_pixels.is_null() {
            return ptr::null_mut();
        }

        for (index, rgba) in tray_icon_rgba().chunks_exact(4).enumerate() {
            let bgra = [rgba[2], rgba[1], rgba[0], rgba[3]];
            ptr::copy_nonoverlapping(bgra.as_ptr(), dib_pixels.cast::<u8>().add(index * 4), 4);
        }

        let mask_bits = [0u8; (SIZE as usize * SIZE as usize) / 8];
        let mask_bitmap = CreateBitmap(SIZE, SIZE, 1, 1, mask_bits.as_ptr().cast::<c_void>());
        if mask_bitmap.is_null() {
            DeleteObject(color_bitmap);
            return ptr::null_mut();
        }

        let icon_info = ICONINFO {
            fIcon: 1,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(mask_bitmap);
        DeleteObject(color_bitmap);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::{handle_menu_event, pause_menu_label, status_content, tray_icon_rgba, PAUSE_ID};
    use crate::state::AppState;
    use tray_icon::menu::{MenuEvent, MenuItem};

    #[test]
    fn pause_menu_action_matches_current_state() {
        assert_eq!(pause_menu_label(false), "Pause KAI");
        assert_eq!(pause_menu_label(true), "Resume KAI");
    }

    #[test]
    fn clicking_pause_updates_state_and_action_label_in_both_directions() {
        let state = AppState::with_history_limit(10);
        let pause = MenuItem::with_id(PAUSE_ID, pause_menu_label(false), true, None);
        let event = || MenuEvent {
            id: PAUSE_ID.into(),
        };

        handle_menu_event(event(), &state, &pause);
        assert!(state.is_paused());
        assert_eq!(pause.text(), "Resume KAI");

        handle_menu_event(event(), &state, &pause);
        assert!(!state.is_paused());
        assert_eq!(pause.text(), "Pause KAI");
    }

    #[test]
    fn status_content_distinguishes_paused_ready_and_processing_states() {
        assert_eq!(status_content(true, false).0, "KAI | PAUSED");
        assert!(status_content(true, false).1.contains("listening is off"));
        assert_eq!(status_content(false, false).0, "KAI | RUNNING");
        assert!(status_content(false, false).1.contains("active and ready"));
        assert!(status_content(false, true)
            .1
            .contains("response is being processed"));
    }

    #[test]
    fn logo_asset_has_expected_dimensions_and_blue_accent() {
        let pixels = tray_icon_rgba();
        assert_eq!(
            pixels.len(),
            super::LOGO_SIZE as usize * super::LOGO_SIZE as usize * 4
        );
        assert!(pixels
            .chunks_exact(4)
            .any(|pixel| pixel[2] > pixel[0].saturating_add(70)));
    }

    #[cfg(windows)]
    #[test]
    fn native_window_icon_is_created_from_the_tray_logo() {
        use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;

        let icon = super::create_logo_icon();
        assert!(!icon.is_null());
        unsafe { DestroyIcon(icon) };
    }
}
