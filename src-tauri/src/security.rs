//! Security hardening utilities for Eiryn:
//! - DPAPI credential encryption at rest on Windows
//! - Path traversal sanitization
//! - Secure secret storage

#[cfg(windows)]
mod dpapi {
    use std::os::raw::c_void;

    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    type CryptProtectDataFn = unsafe extern "system" fn(
        p_data_in: *mut DataBlob,
        sz_data_descr: *const u16,
        p_optional_entropy: *mut DataBlob,
        pv_reserved: *mut c_void,
        p_prompt_struct: *mut c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;

    type CryptUnprotectDataFn = unsafe extern "system" fn(
        p_data_in: *mut DataBlob,
        ppsz_data_descr: *mut *mut u16,
        p_optional_entropy: *mut DataBlob,
        pv_reserved: *mut c_void,
        p_prompt_struct: *mut c_void,
        dw_flags: u32,
        p_data_out: *mut DataBlob,
    ) -> i32;

    type LocalFreeFn = unsafe extern "system" fn(h_mem: *mut c_void) -> *mut c_void;

    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    pub fn encrypt(plaintext: &str) -> Result<String, String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }

        unsafe {
            use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            use windows::core::w;

            let h_crypt = LoadLibraryW(w!("Crypt32.dll")).map_err(|e| format!("Failed to load Crypt32.dll: {e}"))?;
            let proc_protect = GetProcAddress(h_crypt, windows::core::s!("CryptProtectData"))
                .ok_or("CryptProtectData not found")?;
            let crypt_protect: CryptProtectDataFn = std::mem::transmute(proc_protect);

            let h_kernel = LoadLibraryW(w!("Kernel32.dll")).map_err(|e| format!("Failed to load Kernel32.dll: {e}"))?;
            let proc_free = GetProcAddress(h_kernel, windows::core::s!("LocalFree"))
                .ok_or("LocalFree not found")?;
            let local_free: LocalFreeFn = std::mem::transmute(proc_free);

            let mut in_bytes = plaintext.as_bytes().to_vec();
            let mut data_in = DataBlob {
                cb_data: in_bytes.len() as u32,
                pb_data: in_bytes.as_mut_ptr(),
            };
            let mut data_out = DataBlob {
                cb_data: 0,
                pb_data: std::ptr::null_mut(),
            };

            let res = crypt_protect(
                &mut data_in,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            );

            if res == 0 || data_out.pb_data.is_null() {
                return Err("CryptProtectData failed".to_string());
            }

            let slice = std::slice::from_raw_parts(data_out.pb_data, data_out.cb_data as usize);
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(slice);
            local_free(data_out.pb_data as *mut c_void);

            Ok(format!("dpapi:{b64}"))
        }
    }

    pub fn decrypt(ciphertext: &str) -> Result<String, String> {
        if ciphertext.is_empty() {
            return Ok(String::new());
        }
        if !ciphertext.starts_with("dpapi:") {
            // Legacy unencrypted plaintext fallback
            return Ok(ciphertext.to_string());
        }

        let raw_b64 = &ciphertext["dpapi:".len()..];
        use base64::Engine;
        let mut encrypted_bytes = base64::engine::general_purpose::STANDARD
            .decode(raw_b64)
            .map_err(|e| format!("Invalid base64 in DPAPI payload: {e}"))?;

        unsafe {
            use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
            use windows::core::w;

            let h_crypt = LoadLibraryW(w!("Crypt32.dll")).map_err(|e| format!("Failed to load Crypt32.dll: {e}"))?;
            let proc_unprotect = GetProcAddress(h_crypt, windows::core::s!("CryptUnprotectData"))
                .ok_or("CryptUnprotectData not found")?;
            let crypt_unprotect: CryptUnprotectDataFn = std::mem::transmute(proc_unprotect);

            let h_kernel = LoadLibraryW(w!("Kernel32.dll")).map_err(|e| format!("Failed to load Kernel32.dll: {e}"))?;
            let proc_free = GetProcAddress(h_kernel, windows::core::s!("LocalFree"))
                .ok_or("LocalFree not found")?;
            let local_free: LocalFreeFn = std::mem::transmute(proc_free);

            let mut data_in = DataBlob {
                cb_data: encrypted_bytes.len() as u32,
                pb_data: encrypted_bytes.as_mut_ptr(),
            };
            let mut data_out = DataBlob {
                cb_data: 0,
                pb_data: std::ptr::null_mut(),
            };

            let res = crypt_unprotect(
                &mut data_in,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut data_out,
            );

            if res == 0 || data_out.pb_data.is_null() {
                return Err("CryptUnprotectData failed".to_string());
            }

            let slice = std::slice::from_raw_parts(data_out.pb_data, data_out.cb_data as usize);
            let s = String::from_utf8(slice.to_vec()).map_err(|e| format!("Invalid UTF-8: {e}"))?;
            local_free(data_out.pb_data as *mut c_void);

            Ok(s)
        }
    }
}

#[cfg(not(windows))]
mod dpapi {
    pub fn encrypt(plaintext: &str) -> Result<String, String> {
        Ok(plaintext.to_string())
    }
    pub fn decrypt(ciphertext: &str) -> Result<String, String> {
        Ok(ciphertext.to_string())
    }
}

pub fn encrypt_secret(val: &str) -> Result<String, String> {
    dpapi::encrypt(val)
}

pub fn decrypt_secret(val: &str) -> Result<String, String> {
    dpapi::decrypt(val)
}

/// Sanitize file names to avoid path traversal (e.g. '../', null bytes, slashes).
pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}
