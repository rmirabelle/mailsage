/**
 * Sign-in tokens and passwords live in Windows Credential Manager and nowhere
 * else: not in the database, not in files, not in logs. Windows encrypts each
 * entry for the signed-in Windows user.
 */
use anyhow::{anyhow, bail, Result};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_FLAGS,
    CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

/** Windows allows 2560 bytes for one entry. The secret is stored as UTF-8 to use few bytes. */
const MAX_SECRET_BYTES: usize = 2560;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/** The Credential Manager entry name of an account's secret. */
pub fn account_target(account_id: i64) -> String {
    format!("MailSage/account/{account_id}")
}

pub fn save(target: &str, secret: &str) -> Result<()> {
    if secret.len() > MAX_SECRET_BYTES {
        bail!("the secret is too long for Windows Credential Manager");
    }
    let mut target_wide = wide(target);
    let mut user_wide = wide("Mail Sage");
    let mut blob = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {
        Flags: CRED_FLAGS(0),
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_wide.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_wide.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|error| anyhow!("Windows Credential Manager did not save the secret: {error}"))
}

/** The secret, or None when there is no entry. */
pub fn read(target: &str) -> Result<Option<String>> {
    let target_wide = wide(target);
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    let result = unsafe {
        CredReadW(
            PCWSTR(target_wide.as_ptr()),
            CRED_TYPE_GENERIC,
            None,
            &mut credential,
        )
    };
    if let Err(error) = result {
        if error.code() == ERROR_NOT_FOUND.to_hresult() {
            return Ok(None);
        }
        bail!("Windows Credential Manager did not read the secret: {error}");
    }
    let secret = unsafe {
        let entry = &*credential;
        let bytes = std::slice::from_raw_parts(entry.CredentialBlob, entry.CredentialBlobSize as usize);
        let secret = String::from_utf8(bytes.to_vec());
        CredFree(credential as *const core::ffi::c_void);
        secret
    };
    Ok(Some(secret.map_err(|_| anyhow!("the stored secret is damaged"))?))
}

/** Deletes the entry. No entry is not an error. */
pub fn delete(target: &str) -> Result<()> {
    let target_wide = wide(target);
    match unsafe { CredDeleteW(PCWSTR(target_wide.as_ptr()), CRED_TYPE_GENERIC, None) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(error) => bail!("Windows Credential Manager did not delete the secret: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_reads_and_deletes_a_secret() {
        let target = "MailSage/test/secrets";
        save(target, "first value").unwrap();
        save(target, "second value é").unwrap();
        assert_eq!(read(target).unwrap().as_deref(), Some("second value é"));
        delete(target).unwrap();
        assert_eq!(read(target).unwrap(), None);
        delete(target).unwrap();
    }

    #[test]
    fn refuses_a_secret_that_is_too_long() {
        assert!(save("MailSage/test/long", &"x".repeat(MAX_SECRET_BYTES + 1)).is_err());
    }
}
