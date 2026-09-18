// ============================================================
// listener.rs — Global keyboard listener (Windows WH_KEYBOARD_LL)
//
// Uses SetWindowsHookEx(WH_KEYBOARD_LL) directly via the
// windows-sys crate. Runs a proper Win32 message pump (GetMessage)
// on a dedicated OS thread — this is required for the low-level
// keyboard hook to fire correctly.
//
// No admin privileges required for WH_KEYBOARD_LL.
// ============================================================

use log::{debug, error, info};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc as std_mpsc;
use tokio::sync::mpsc::UnboundedSender;

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// A key event sent from the OS hook into the async trigger engine.
#[derive(Debug, Clone)]
pub enum KeyEvent {
    Char(char),
    Enter,
    Escape,
    Backspace,
    Alt,
    Other,
}

// Thread-local sender — the hook callback is a plain C function pointer,
// so we communicate via a global atomic pointer to our sender.
static SENDER: AtomicPtr<std_mpsc::SyncSender<KeyEvent>> =
    AtomicPtr::new(std::ptr::null_mut());

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

/// Starts the global keyboard listener.
/// Must be called from a dedicated OS thread (not an async task).
pub fn start_listener(tx: UnboundedSender<KeyEvent>) {
    // Bridge: std SyncSender (used from hook callback) → tokio UnboundedSender
    let (internal_tx, internal_rx) = std_mpsc::sync_channel::<KeyEvent>(256);

    // Store sender pointer for the hook callback
    let boxed = Box::new(internal_tx);
    SENDER.store(Box::into_raw(boxed), Ordering::SeqCst);

    // Bridge thread: relay from std channel → tokio channel
    std::thread::spawn(move || {
        for event in internal_rx {
            if tx.send(event).is_err() {
                break; // Main app shutting down
            }
        }
    });

    unsafe {
        // Force creation of a message queue for this thread
        let mut msg = std::mem::zeroed::<MSG>();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);

        // Get module instance for current process
        let h_mod = GetModuleHandleW(std::ptr::null());

        // Install the low-level keyboard hook.
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), h_mod, 0);

        if hook == std::ptr::null_mut() {
            error!("Failed to install keyboard hook! Error code: {}", 
                windows_sys::Win32::Foundation::GetLastError());
            error!("This should not require admin privileges. Check antivirus.");
            return;
        }

        info!("Keyboard hook installed successfully (handle: {:?}, module: {:?})", hook, h_mod);

        // Run the Win32 message pump — REQUIRED for WH_KEYBOARD_LL to fire.
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        UnhookWindowsHookEx(hook);
        info!("Keyboard hook removed");
    }
}

/// Win32 low-level keyboard hook callback.
/// Called by Windows for every keypress, system-wide.
unsafe extern "system" fn keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    eprintln!("[HOOK] Called n_code={} w_param={:#x}", n_code, w_param);
    // HC_ACTION (0) means we should process the message
    if n_code == HC_ACTION as i32 {
        let msg_type = w_param as u32;
        if msg_type == WM_KEYDOWN || msg_type == WM_SYSKEYDOWN {
            let kb = &*(l_param as *const KBDLLHOOKSTRUCT);
            let vk_code = kb.vkCode;

            let event = vk_to_key_event(vk_code);
            eprintln!("[HOOK KEY] vk={:#04x} → {:?}", vk_code, event);

            // Send the event (non-blocking — drop if buffer full)
            let sender_ptr = SENDER.load(Ordering::SeqCst);
            if !sender_ptr.is_null() {
                let _ = (*sender_ptr).try_send(event);
            }
        }
    }

    // MUST call next hook in chain — never swallow events
    CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param)
}

/// Maps a Windows Virtual Key code to our KeyEvent type.
fn vk_to_key_event(vk: u32) -> KeyEvent {
    match vk {
        // Alt key (VK_MENU = 0x12, VK_LMENU = 0xA4, VK_RMENU = 0xA5)
        vk if vk == VK_MENU as u32 || vk == VK_LMENU as u32 || vk == VK_RMENU as u32 => KeyEvent::Alt,

        // Enter
        vk if vk == VK_RETURN as u32 => KeyEvent::Enter,

        // Escape
        vk if vk == VK_ESCAPE as u32 => KeyEvent::Escape,

        // Backspace
        vk if vk == VK_BACK as u32 => KeyEvent::Backspace,

        // Letters A–Z (VK codes 0x41–0x5A)
        0x41 => KeyEvent::Char('a'),
        0x42 => KeyEvent::Char('b'),
        0x43 => KeyEvent::Char('c'),
        0x44 => KeyEvent::Char('d'),
        0x45 => KeyEvent::Char('e'),
        0x46 => KeyEvent::Char('f'),
        0x47 => KeyEvent::Char('g'),
        0x48 => KeyEvent::Char('h'),
        0x49 => KeyEvent::Char('i'),
        0x4A => KeyEvent::Char('j'),
        0x4B => KeyEvent::Char('k'),
        0x4C => KeyEvent::Char('l'),
        0x4D => KeyEvent::Char('m'),
        0x4E => KeyEvent::Char('n'),
        0x4F => KeyEvent::Char('o'),
        0x50 => KeyEvent::Char('p'),
        0x51 => KeyEvent::Char('q'),
        0x52 => KeyEvent::Char('r'),
        0x53 => KeyEvent::Char('s'),
        0x54 => KeyEvent::Char('t'),
        0x55 => KeyEvent::Char('u'),
        0x56 => KeyEvent::Char('v'),
        0x57 => KeyEvent::Char('w'),
        0x58 => KeyEvent::Char('x'),
        0x59 => KeyEvent::Char('y'),
        0x5A => KeyEvent::Char('z'),

        // Digits 0–9 (VK codes 0x30–0x39)
        0x30 => KeyEvent::Char('0'),
        0x31 => KeyEvent::Char('1'),
        0x32 => KeyEvent::Char('2'),
        0x33 => KeyEvent::Char('3'),
        0x34 => KeyEvent::Char('4'),
        0x35 => KeyEvent::Char('5'),
        0x36 => KeyEvent::Char('6'),
        0x37 => KeyEvent::Char('7'),
        0x38 => KeyEvent::Char('8'),
        0x39 => KeyEvent::Char('9'),

        // Space
        vk if vk == VK_SPACE as u32 => KeyEvent::Char(' '),

        // Semicolon (VK_OEM_1 = 0xBA)
        0xBA => KeyEvent::Char(';'),

        // Other punctuation
        0xBC => KeyEvent::Char(','),
        0xBE => KeyEvent::Char('.'),
        0xBF => KeyEvent::Char('/'),
        0xDB => KeyEvent::Char('['),
        0xDD => KeyEvent::Char(']'),
        0xDC => KeyEvent::Char('\\'),
        0xDE => KeyEvent::Char('\''),
        0xBD => KeyEvent::Char('-'),
        0xBB => KeyEvent::Char('='),
        0xC0 => KeyEvent::Char('`'),

        // Numpad
        vk if vk == VK_NUMPAD0 as u32 => KeyEvent::Char('0'),
        vk if vk == VK_NUMPAD1 as u32 => KeyEvent::Char('1'),
        vk if vk == VK_NUMPAD2 as u32 => KeyEvent::Char('2'),
        vk if vk == VK_NUMPAD3 as u32 => KeyEvent::Char('3'),
        vk if vk == VK_NUMPAD4 as u32 => KeyEvent::Char('4'),
        vk if vk == VK_NUMPAD5 as u32 => KeyEvent::Char('5'),
        vk if vk == VK_NUMPAD6 as u32 => KeyEvent::Char('6'),
        vk if vk == VK_NUMPAD7 as u32 => KeyEvent::Char('7'),
        vk if vk == VK_NUMPAD8 as u32 => KeyEvent::Char('8'),
        vk if vk == VK_NUMPAD9 as u32 => KeyEvent::Char('9'),

        // Everything else (arrows, F-keys, modifiers, etc.)
        _ => KeyEvent::Other,
    }
}
