//! Only secrets entered into usoai are stored here. Vendor credentials are never copied.
const MAX_BYTES: usize = 2560;

fn target(account_id: &str) -> Result<String, String> {
    if !crate::accounts::valid_profile_id(account_id) {
        return Err("Invalid account ID".into());
    }
    Ok(format!("usoai/{account_id}"))
}

pub fn set(account_id: &str, value: &str) -> Result<(), String> {
    let target = target(account_id)?;
    if value.trim().is_empty() || value.len() > MAX_BYTES || value.contains('\0') {
        return Err("Enter a nonempty credential of at most 2560 bytes".into());
    }
    platform::set(&target, value)
}

pub fn read(account_id: &str) -> Result<Option<String>, String> {
    platform::read(&target(account_id)?)
}

pub fn remove(account_id: &str) -> Result<(), String> {
    platform::remove(&target(account_id)?)
}

#[cfg(windows)]
mod platform {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW,
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };

    fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() }
    fn missing(error: &windows::core::Error) -> bool { error.code().0 as u32 == 0x80070490 }

    pub fn set(target: &str, value: &str) -> Result<(), String> {
        let mut target = wide(target);
        let mut bytes = value.as_bytes().to_vec();
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            ..Default::default()
        };
        let result = unsafe { CredWriteW(&credential, 0) }
            .map_err(|_| "Unable to store the credential in Windows Credential Manager".into());
        bytes.fill(0);
        result
    }

    pub fn read(target: &str) -> Result<Option<String>, String> {
        let target = wide(target);
        let mut raw: *mut CREDENTIALW = std::ptr::null_mut();
        if let Err(error) = unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0, &mut raw) } {
            return if missing(&error) { Ok(None) } else { Err("Unable to read Windows Credential Manager".into()) };
        }
        if raw.is_null() { return Err("Windows returned an empty credential".into()); }
        let value = unsafe {
            let credential = &*raw;
            let bytes = if credential.CredentialBlobSize as usize > super::MAX_BYTES || credential.CredentialBlob.is_null() {
                None
            } else {
                Some(std::slice::from_raw_parts(credential.CredentialBlob, credential.CredentialBlobSize as usize).to_vec())
            };
            CredFree(raw.cast());
            bytes
        }.ok_or("Windows returned an invalid credential")?;
        String::from_utf8(value).map(Some).map_err(|_| "Stored credential has an invalid encoding".into())
    }

    pub fn remove(target: &str) -> Result<(), String> {
        let target = wide(target);
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, 0) } {
            Ok(()) => Ok(()),
            Err(error) if missing(&error) => Ok(()),
            Err(_) => Err("Unable to remove the credential from Windows Credential Manager".into()),
        }
    }
}

#[cfg(all(not(windows), test))]
mod platform {
    use std::{collections::HashMap, sync::{Mutex, OnceLock}};
    fn store() -> &'static Mutex<HashMap<String, String>> {
        static STORE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        STORE.get_or_init(|| Mutex::new(HashMap::new()))
    }
    pub fn set(target: &str, value: &str) -> Result<(), String> { store().lock().unwrap().insert(target.into(), value.into()); Ok(()) }
    pub fn read(target: &str) -> Result<Option<String>, String> { Ok(store().lock().unwrap().get(target).cloned()) }
    pub fn remove(target: &str) -> Result<(), String> { store().lock().unwrap().remove(target); Ok(()) }
}

#[cfg(all(not(windows), not(test)))]
mod platform {
    pub fn set(_: &str, _: &str) -> Result<(), String> { Err("Credential storage requires Windows".into()) }
    pub fn read(_: &str) -> Result<Option<String>, String> { Err("Credential storage requires Windows".into()) }
    pub fn remove(_: &str) -> Result<(), String> { Err("Credential storage requires Windows".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unscoped_targets_and_invalid_values() {
        for id in ["../vendor", "gemini:antigravity", "claude/work", "", "x\0y"] {
            assert!(target(id).is_err());
        }
        assert!(set("test-validation", " ").is_err());
        assert!(set("test-validation", &"x".repeat(MAX_BYTES + 1)).is_err());
        assert_eq!(target("codex-work").unwrap(), "usoai/codex-work");
    }

    #[test]
    fn own_secret_round_trip_and_idempotent_removal() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("test-{}-{stamp}", std::process::id());
        struct Cleanup(String);
        impl Drop for Cleanup { fn drop(&mut self) { let _ = remove(&self.0); } }
        let _cleanup = Cleanup(id.clone());
        assert!(read(&id).unwrap().is_none());
        set(&id, "fake-secret-for-storage-test").unwrap();
        assert_eq!(read(&id).unwrap().as_deref(), Some("fake-secret-for-storage-test"));
        set(&id, "replacement").unwrap();
        assert_eq!(read(&id).unwrap().as_deref(), Some("replacement"));
        remove(&id).unwrap();
        remove(&id).unwrap();
        assert!(read(&id).unwrap().is_none());
    }
}
