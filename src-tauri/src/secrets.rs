//! Шифрование API-ключей в `config.json`.
//!
//! Ключи шифруются Windows DPAPI в контексте текущего пользователя. Это значит:
//!  * на диске нет открытых ключей;
//!  * скопированный на другую машину/другого пользователя `config.json`
//!    не позволит восстановить ключи (это ожидаемо и безопасно);
//!  * приложение по-прежнему получает открытый ключ в память для запросов.

const PREFIX: &str = "enc:v1:";

pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(PREFIX)
}

pub fn protect(plain: &str) -> Result<String, String> {
    if plain.is_empty() {
        return Ok(String::new());
    }
    if is_encrypted(plain) {
        return Ok(plain.to_string());
    }
    let encrypted = platform_protect(plain.as_bytes())?;
    Ok(format!("{PREFIX}{}", hex_encode(&encrypted)))
}

pub fn unprotect(value: &str) -> Result<String, String> {
    if !is_encrypted(value) {
        return Ok(value.to_string());
    }
    let hex = value.strip_prefix(PREFIX).unwrap_or(value);
    let bytes = hex_decode(hex)?;
    let decrypted = platform_unprotect(&bytes)?;
    String::from_utf8(decrypted).map_err(|e| format!("decrypted API key is not UTF-8: {e}"))
}

#[cfg(windows)]
fn platform_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let ok = unsafe {
        CryptProtectData(
            &input,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 {
        return Err(format!(
            "DPAPI CryptProtectData failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if output.pbData.is_null() || output.cbData == 0 {
        return Err("DPAPI returned an empty encrypted blob".to_string());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData as *mut std::ffi::c_void);
    }
    Ok(bytes)
}

#[cfg(windows)]
fn platform_unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    let input = CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description: *mut u16 = std::ptr::null_mut();
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            &mut description,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if !description.is_null() {
        unsafe {
            LocalFree(description as *mut std::ffi::c_void);
        }
    }
    if ok == 0 {
        return Err(format!(
            "DPAPI CryptUnprotectData failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    if output.pbData.is_null() || output.cbData == 0 {
        return Err("DPAPI returned an empty decrypted blob".to_string());
    }
    let bytes =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
    unsafe {
        LocalFree(output.pbData as *mut std::ffi::c_void);
    }
    Ok(bytes)
}

#[cfg(not(windows))]
fn platform_protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI is only available on Windows; refusing to store API keys in plaintext".to_string())
}

#[cfg(not(windows))]
fn platform_unprotect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI is only available on Windows".to_string())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn hex_decode(input: &str) -> Result<Vec<u8>, String> {
    if input.len() % 2 != 0 {
        return Err("encrypted API key has an invalid length".to_string());
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    let bytes = input.as_bytes();
    for chunk in bytes.chunks(2) {
        let hi = hex_value(chunk[0])?;
        let lo = hex_value(chunk[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_value(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("encrypted API key contains invalid characters".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let bytes = [0u8, 1, 15, 16, 255];
        assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes);
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let secret = "sk-or-v1-test-secret-123";
        let encrypted = protect(secret).expect("protect");
        assert!(encrypted.starts_with(PREFIX));
        assert!(!encrypted.contains(secret));
        assert_eq!(unprotect(&encrypted).expect("unprotect"), secret);
    }
}
