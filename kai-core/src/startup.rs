//! Per-user Windows sign-in startup integration.

use anyhow::{bail, Context, Result};
use std::os::windows::process::CommandExt;
use std::{
    ffi::OsString,
    fs, mem,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
};
use windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "KAI";
const LAUNCHER_FILE: &str = "kai-startup.vbs";
const QUIET_START_ARG: &str = "--quiet-start";

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn startup_script(executable: &Path) -> String {
    let executable = executable.to_string_lossy().replace('"', "\"\"");
    format!(
        "' KAI hidden sign-in launcher\r\nSet kaiShell = CreateObject(\"WScript.Shell\")\r\nkaiShell.Run \"\"\"\" & \"{executable}\" & \"\"\" --background {QUIET_START_ARG}\", 0, False\r\n"
    )
}

fn startup_command(script: &Path, windows_dir: &std::ffi::OsStr) -> OsString {
    let host = PathBuf::from(windows_dir)
        .join("System32")
        .join("wscript.exe");
    let mut command = OsString::from("\"");
    command.push(host.as_os_str());
    command.push("\" \"");
    command.push(script.as_os_str());
    command.push("\"");
    command
}

fn registry_error(code: u32, context: &str) -> anyhow::Error {
    anyhow::Error::new(std::io::Error::from_raw_os_error(code as i32)).context(context.to_string())
}

fn open_run_key_for_update() -> Result<HKEY> {
    let key_path = wide(RUN_KEY);
    let mut key: HKEY = std::ptr::null_mut();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
    };
    if open == 0 {
        return Ok(key);
    }
    Err(registry_error(
        open,
        "Windows sign-in settings are unavailable",
    ))
}

/// Whether KAI is registered to start for this Windows user at sign-in.
pub fn is_enabled() -> Result<bool> {
    let key_path = wide(RUN_KEY);
    let value_name = wide(VALUE_NAME);
    let mut key: HKEY = std::ptr::null_mut();
    let open = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            key_path.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if open != 0 {
        if open == ERROR_FILE_NOT_FOUND {
            return Ok(false);
        }
        return Err(registry_error(open, "Could not read KAI sign-in setting"));
    }

    let mut value_type = 0;
    let mut byte_count = 0;
    let query = unsafe {
        RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut value_type,
            std::ptr::null_mut(),
            &mut byte_count,
        )
    };
    unsafe { RegCloseKey(key) };

    match query {
        0 => Ok(value_type == REG_SZ),
        ERROR_FILE_NOT_FOUND => Ok(false),
        error => Err(registry_error(error, "Could not read KAI sign-in setting")),
    }
}

/// Adds or removes only KAI's per-user Run value; it never requires elevation.
pub fn set_enabled(enabled: bool) -> Result<()> {
    let command_data = if enabled {
        let executable = std::env::current_exe().context("Could not locate kai.exe")?;
        let launcher = crate::config::Config::config_dir().join(LAUNCHER_FILE);
        if let Some(parent) = launcher.parent() {
            fs::create_dir_all(parent).context("Could not create KAI config directory")?;
        }
        fs::write(&launcher, startup_script(&executable))
            .context("Could not write KAI's hidden startup launcher")?;
        let windows_dir = std::env::var_os("WINDIR").context("WINDIR is not set")?;
        Some(
            startup_command(&launcher, &windows_dir)
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let value_name = wide(VALUE_NAME);
    let key = open_run_key_for_update()?;

    let result = if let Some(mut data) = command_data {
        let byte_count = (data.len() * mem::size_of::<u16>()) as u32;
        unsafe {
            RegSetValueExW(
                key,
                value_name.as_ptr(),
                0,
                REG_SZ,
                data.as_mut_ptr().cast(),
                byte_count,
            )
        }
    } else {
        let result = unsafe { RegDeleteValueW(key, value_name.as_ptr()) };
        if result == 2 {
            0
        } else {
            result
        }
    };

    unsafe { RegCloseKey(key) };
    if result != 0 {
        bail!(
            "{}",
            registry_error(result, "Could not update KAI sign-in setting")
        );
    }
    if !enabled {
        let launcher = crate::config::Config::config_dir().join(LAUNCHER_FILE);
        let _ = fs::remove_file(launcher);
    }
    Ok(())
}

/// Refresh an already-enabled Run entry to use the hidden launcher.
pub fn refresh_if_enabled() -> Result<bool> {
    if is_enabled()? {
        set_enabled(true)?;
        let windows_dir = std::env::var_os("WINDIR").context("WINDIR is not set")?;
        let script = crate::config::Config::config_dir().join(LAUNCHER_FILE);
        Command::new(
            PathBuf::from(windows_dir)
                .join("System32")
                .join("wscript.exe"),
        )
        .arg(script)
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .spawn()
        .context("Could not restart KAI through the hidden startup launcher")?;
        return Ok(true);
    }
    Ok(false)
}

/// Detach the opt-in sign-in instance from its automatically-created console.
pub fn detach_console() {
    unsafe { windows_sys::Win32::System::Console::FreeConsole() };
}

#[cfg(test)]
mod tests {
    use super::{startup_command, startup_script};
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn startup_command_quotes_executable_paths_and_requests_background_mode() {
        assert_eq!(
            startup_command(
                Path::new(r"C:\Users\Test\AppData\Roaming\kai\kai-startup.vbs"),
                OsStr::new(r"C:\Windows"),
            ),
            r#""C:\Windows\System32\wscript.exe" "C:\Users\Test\AppData\Roaming\kai\kai-startup.vbs""#
        );
        assert!(startup_script(Path::new(r"C:\Program Files\KAI\kai.exe"))
            .contains(r#"kaiShell.Run """" & "C:\Program Files\KAI\kai.exe" & """ --background --quiet-start", 0, False"#));
    }
}
