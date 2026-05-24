// Windows DPAPI wrappers for at-rest protection of small secrets in settings.json.
// Ciphertext is base64-encoded and prefixed with PREFIX so we can tell it apart from
// legacy plaintext and auto-migrate on the next save.

pub const PREFIX: &str = "dpapi:v1:";

#[cfg(windows)]
pub fn protect(plaintext: &str) -> Option<String> {
    use base64::Engine;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{CryptProtectData, CRYPT_INTEGER_BLOB};

    let bytes = plaintext.as_bytes();
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &mut input,
            None,
            None,
            None,
            None,
            0,
            &mut output,
        )
    };
    if ok.is_err() || output.pbData.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let encoded = base64::engine::general_purpose::STANDARD.encode(slice);
    unsafe {
        let _ = LocalFree(windows::Win32::Foundation::HLOCAL(output.pbData as *mut _));
    }
    Some(format!("{PREFIX}{encoded}"))
}

#[cfg(windows)]
pub fn unprotect(value: &str) -> Option<String> {
    use base64::Engine;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let payload = value.strip_prefix(PREFIX)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let mut input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            0,
            &mut output,
        )
    };
    if ok.is_err() || output.pbData.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let decoded = String::from_utf8(slice.to_vec()).ok();
    unsafe {
        let _ = LocalFree(windows::Win32::Foundation::HLOCAL(output.pbData as *mut _));
    }
    decoded
}

#[cfg(not(windows))]
pub fn protect(_plaintext: &str) -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn unprotect(_value: &str) -> Option<String> {
    None
}

pub fn is_protected(value: &str) -> bool {
    value.starts_with(PREFIX)
}
