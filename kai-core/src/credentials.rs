//! OS credential-store boundary.

#[cfg(windows)]
use std::ptr;

/// Read a generic secret from Windows Credential Manager.
#[cfg(windows)]
pub fn read_secret(target: &str) -> Option<String> {
    use windows_sys::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    let target_wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let mut credential: *mut CREDENTIALW = ptr::null_mut();
    let success = unsafe { CredReadW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
    if success == 0 || credential.is_null() {
        return None;
    }

    let result = unsafe {
        let item = &*credential;
        if item.CredentialBlob.is_null() || item.CredentialBlobSize == 0 {
            None
        } else {
            let bytes =
                std::slice::from_raw_parts(item.CredentialBlob, item.CredentialBlobSize as usize);
            String::from_utf8(bytes.to_vec()).ok()
        }
    };
    unsafe { CredFree(credential as *mut _) };
    result
}

#[cfg(not(windows))]
pub fn read_secret(_target: &str) -> Option<String> {
    None
}

#[cfg(windows)]
pub fn write_secret(target: &str, secret: &str) -> anyhow::Result<()> {
    use anyhow::bail;
    use windows_sys::Win32::Security::Credentials::{
        CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    let target_wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    let user_wide: Vec<u16> = "KAI".encode_utf16().chain(Some(0)).collect();
    let secret_bytes = secret.as_bytes();
    let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = target_wide.as_ptr() as *mut u16;
    credential.UserName = user_wide.as_ptr() as *mut u16;
    credential.CredentialBlobSize = secret_bytes.len() as u32;
    credential.CredentialBlob = secret_bytes.as_ptr() as *mut u8;
    // For generic credentials, Windows stores this under the current user's
    // Credential Manager profile while preserving it across logon sessions.
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;

    if unsafe { CredWriteW(&mut credential, 0) } == 0 {
        bail!("Windows Credential Manager rejected the credential");
    }
    Ok(())
}

#[cfg(windows)]
pub fn delete_secret(target: &str) -> anyhow::Result<()> {
    use windows_sys::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

    let target_wide: Vec<u16> = target.encode_utf16().chain(Some(0)).collect();
    if unsafe { CredDeleteW(target_wide.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(1168) {
            return Err(error.into());
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn write_secret(_target: &str, _secret: &str) -> anyhow::Result<()> {
    anyhow::bail!("secure credential storage is not implemented for this operating system")
}

#[cfg(not(windows))]
pub fn delete_secret(_target: &str) -> anyhow::Result<()> {
    anyhow::bail!("secure credential storage is not implemented for this operating system")
}
