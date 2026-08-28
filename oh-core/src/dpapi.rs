//! Windows DPAPI 加解密：用于在本机安全保存 API Key（仅当前用户/本机可解密）。
//! 附带：加密级随机数（BCryptGenRandom），用于本地 sidecar 会话令牌。

use base64::{engine::general_purpose::STANDARD as B64, Engine};

/// 生成加密级随机十六进制字符串（用于本地网关/记忆服务的会话令牌）。
pub fn random_hex(byte_len: usize) -> Option<String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::Cryptography::{
            BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        };
        let mut buf = vec![0u8; byte_len];
        let status = unsafe {
            BCryptGenRandom(
                std::ptr::null_mut(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                BCRYPT_USE_SYSTEM_PREFERRED_RNG,
            )
        };
        if status != 0 {
            return None;
        }
        let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
        Some(hex)
    }
    #[cfg(not(windows))]
    {
        let _ = byte_len;
        None
    }
}

/// 使用 Windows DPAPI 加密（用户作用域）。
pub fn encrypt(plain: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::{
            CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        let bytes = plain.as_bytes();
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptProtectData(
                &in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI 加密失败".to_string());
        }
        let out = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let b64 = B64.encode(out);
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        Ok(b64)
    }
    #[cfg(not(windows))]
    {
        // 非 Windows（开发）降级：明文 base64（仅测试用途）
        Ok(B64.encode(plain.as_bytes()))
    }
}

/// 使用 Windows DPAPI 解密。
pub fn decrypt(b64: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Cryptography::{
            CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
        };

        let bytes = B64.decode(b64).map_err(|_| "密文格式错误".to_string())?;
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: bytes.len() as u32,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        let ok = unsafe {
            CryptUnprotectData(
                &in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err("DPAPI 解密失败（可能不是本机/本用户加密的数据）".to_string());
        }
        let out = unsafe { std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize) };
        let s = String::from_utf8_lossy(out).to_string();
        unsafe {
            LocalFree(out_blob.pbData as _);
        }
        Ok(s)
    }
    #[cfg(not(windows))]
    {
        let bytes = B64.decode(b64).map_err(|_| "密文格式错误".to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let key = "sk-test-1234567890";
        let enc = encrypt(key).expect("encrypt");
        assert_ne!(enc, key);
        let dec = decrypt(&enc).expect("decrypt");
        assert_eq!(dec, key);
    }
}
