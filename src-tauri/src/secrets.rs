//! Provider credentials in Windows Credential Manager (per-user, DPAPI-protected by Windows).
//! Secrets never cross into the frontend and are never logged.

use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW,
    CredWriteW,
};
use windows::core::{HSTRING, PWSTR};

const AZURE_SPEECH_TARGET: &str = "VoiceDesktopAgent/AzureSpeech";

/// Azure Speech resource key plus its region. The region is stored as the credential's
/// user name; the key is the protected blob.
#[derive(Clone, PartialEq, Eq)]
pub struct AzureSpeechCredential {
    pub region: String,
    pub key: String,
}

// Deliberately hides the key so it can't leak through `{:?}` in logs.
impl std::fmt::Debug for AzureSpeechCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureSpeechCredential")
            .field("region", &self.region)
            .field("key", &"<redacted>")
            .finish()
    }
}

pub fn save_azure_speech(cred: &AzureSpeechCredential) -> windows::core::Result<()> {
    write_generic(AZURE_SPEECH_TARGET, &cred.region, cred.key.as_bytes())
}

pub fn load_azure_speech() -> windows::core::Result<Option<AzureSpeechCredential>> {
    Ok(
        read_generic(AZURE_SPEECH_TARGET)?.and_then(|(region, blob)| {
            String::from_utf8(blob)
                .ok()
                .map(|key| AzureSpeechCredential { region, key })
        }),
    )
}

pub fn delete_azure_speech() -> windows::core::Result<()> {
    delete_generic(AZURE_SPEECH_TARGET)
}

fn write_generic(target: &str, user: &str, secret: &[u8]) -> windows::core::Result<()> {
    let mut target_w: Vec<u16> = target.encode_utf16().chain([0]).collect();
    let mut user_w: Vec<u16> = user.encode_utf16().chain([0]).collect();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        UserName: PWSTR(user_w.as_mut_ptr()),
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_ptr() as *mut u8,
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0) }
}

fn read_generic(target: &str) -> windows::core::Result<Option<(String, Vec<u8>)>> {
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    match unsafe { CredReadW(&HSTRING::from(target), CRED_TYPE_GENERIC, None, &mut ptr) } {
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => return Ok(None),
        Err(e) => return Err(e),
        Ok(()) => {}
    }
    let result = unsafe {
        let c = &*ptr;
        let user = if c.UserName.is_null() {
            String::new()
        } else {
            c.UserName.to_string().unwrap_or_default()
        };
        let blob =
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
        (user, blob)
    };
    unsafe { CredFree(ptr as *const _) };
    Ok(Some(result))
}

fn delete_generic(target: &str) -> windows::core::Result<()> {
    match unsafe { CredDeleteW(&HSTRING::from(target), CRED_TYPE_GENERIC, None) } {
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_redacts_key() {
        let c = AzureSpeechCredential {
            region: "eastus".into(),
            key: "supersecret123".into(),
        };
        let s = format!("{c:?}");
        assert!(s.contains("eastus"));
        assert!(!s.contains("supersecret123"));
    }

    /// Uses a separate target so it never touches the user's real credential.
    #[test]
    fn credential_manager_round_trip() {
        let target = format!("VoiceDesktopAgent/Test/{}", std::process::id());
        write_generic(&target, "westeurope", b"test-secret").unwrap();
        let (user, blob) = read_generic(&target).unwrap().expect("stored");
        assert_eq!(user, "westeurope");
        assert_eq!(blob, b"test-secret");
        delete_generic(&target).unwrap();
        assert!(read_generic(&target).unwrap().is_none());
        // Deleting twice is fine.
        delete_generic(&target).unwrap();
    }
}
