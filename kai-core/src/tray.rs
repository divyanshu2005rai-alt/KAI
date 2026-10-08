//! System-tray controls for the desktop application.
//!
//! The tray is an adapter over `AppState`; it does not own trigger, provider,
//! or injection behavior. A future settings window can use the same state and
//! configuration boundary.

use std::sync::Arc;

use anyhow::{Context, Result};
use log::{error, info};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIconBuilder};

use crate::settings::SettingsService;
use crate::state::AppState;

#[cfg(windows)]
use std::ptr::null_mut;

#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, TranslateMessage, MSG,
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
    let pause = MenuItem::with_id(PAUSE_ID, "Pause / Resume", true, None);
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

    let icon =
        Icon::from_rgba(tray_icon_rgba(), 16, 16).context("failed to create KAI tray icon")?;
    let _tray = TrayIconBuilder::new()
        .with_tooltip("KAI")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("failed to create KAI tray icon")?;

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        PAUSE_ID => {
            let paused = state.toggle_paused();
            info!(
                "KAI {} from system tray",
                if paused { "paused" } else { "resumed" }
            );
        }
        STATUS_ID => info!(
            "KAI status: {}, processing: {}",
            if state.is_paused() {
                "paused"
            } else {
                "active"
            },
            state.is_processing()
        ),
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
    }));

    // Windows tray menus are delivered through the tray window's message
    // queue. Keep this thread pumping messages so clicks reach MenuEvent.
    #[cfg(windows)]
    unsafe {
        let mut message: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    #[cfg(not(windows))]
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }

    Ok(())
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
        .arg("--configure")
        .spawn()
        .context("failed to launch settings editor")?;
    Ok(())
}

fn tray_icon_rgba() -> Vec<u8> {
    // Simple blue KAI marker; keeping the icon generated avoids a binary asset
    // dependency until the product branding is finalized.
    let mut pixels = Vec::with_capacity(16 * 16 * 4);
    for y in 0..16 {
        for x in 0..16 {
            let border = x == 0 || y == 0 || x == 15 || y == 15;
            let blue = if border { 180 } else { 255 };
            pixels.extend_from_slice(&[32, 96, blue, 255]);
        }
    }
    pixels
}
