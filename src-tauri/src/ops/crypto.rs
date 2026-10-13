//! 백업 암호화 — 검증된 라이브러리만 쓴다. 알고리즘을 직접 구현하지 않는다.
//!
//! 파일 형식 `.asbk` (버전 1):
//!   `ASBK1\n` + 헤더 JSON 한 줄 + `\n` + AES-256-GCM 암호문(헤더 JSON 을 추가 인증 데이터로 묶는다)
//!   헤더에는 알고리즘·KDF 매개변수·salt·nonce·(DPAPI 모드면 감싼 키)만 들어간다. 학생·학교 정보는 없다.
//!
//! 두 가지 모드
//!   · dpapi    현재 Windows 계정용 — 임의의 32바이트 내용 키를 만들어 zip 을 암호화하고, 그 키를 DPAPI(사용자 범위)로 감싼다.
//!              비밀번호 입력이 없어 자동 백업에 맞다. 다른 PC·다른 계정에서는 풀 수 없다.
//!   · password 다른 PC 이전용 — Argon2id(비밀번호, salt) 로 키를 유도한다. 비밀번호는 어디에도 저장·기록하지 않는다.
//! 복호화 실패(비밀번호 오류·손상)는 인증 태그 검증 실패로 드러나며 같은 메시지로 알린다 — 비밀번호를 메시지에 넣지 않는다.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::{AppError, AppResult};

pub const MAGIC: &[u8] = b"ASBK1\n";
pub const FORMAT_VERSION: u32 = 1;
const DPAPI_ENTROPY: &[u8] = b"kr.school.asaccounting.backup.v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Header {
    pub format_version: u32,
    /// dpapi · password
    pub mode: String,
    pub cipher: String,
    pub nonce_b64: String,
    /// password 모드
    pub kdf: Option<String>,
    pub salt_b64: Option<String>,
    pub argon2_m_kib: Option<u32>,
    pub argon2_t: Option<u32>,
    pub argon2_p: Option<u32>,
    /// dpapi 모드 — DPAPI 로 감싼 내용 키
    pub wrapped_key_b64: Option<String>,
    pub created_at: String,
    pub app_version: String,
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn fail(what: &str) -> AppError {
    AppError::new("CRYPTO", what.to_string())
}

/// 비밀번호를 메시지·로그에 넣지 않는 공통 오류.
pub fn wrong_password_or_damaged() -> AppError {
    AppError::new("DECRYPT", "비밀번호가 틀리거나 암호화된 백업 파일이 손상되었습니다. 복원할 수 없습니다.")
}

pub fn is_encrypted_file(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

pub fn is_encrypted_path(path: &std::path::Path) -> bool {
    path.extension().map(|e| e == "asbk").unwrap_or(false)
}

// ── DPAPI (Windows) ─────────────────────────────────────────────────────────

#[cfg(windows)]
fn dpapi(data: &[u8], protect: bool) -> AppResult<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let mut input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut entropy = CRYPT_INTEGER_BLOB { cbData: DPAPI_ENTROPY.len() as u32, pbData: DPAPI_ENTROPY.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if protect {
            CryptProtectData(&mut input, std::ptr::null(), &mut entropy, std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&mut input, std::ptr::null_mut(), &mut entropy, std::ptr::null_mut(), std::ptr::null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return Err(if protect { fail("Windows 계정 보호(DPAPI)로 키를 감싸지 못했습니다.") } else { AppError::new("DECRYPT", "이 백업은 다른 Windows 계정(또는 다른 PC)에서 만든 '현재 계정용' 백업이라 풀 수 없습니다. 비밀번호 백업으로 옮겨 주세요.") });
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
    unsafe {
        LocalFree(out.pbData as _);
    }
    Ok(v)
}

#[cfg(not(windows))]
fn dpapi(_data: &[u8], _protect: bool) -> AppResult<Vec<u8>> {
    Err(fail("현재 계정용 보호(DPAPI)는 Windows 에서만 됩니다."))
}

// ── 키 유도 ────────────────────────────────────────────────────────────────

const ARGON_M_KIB: u32 = 64 * 1024;
const ARGON_T: u32 = 3;
const ARGON_P: u32 = 1;

fn derive_key(password: &str, salt: &[u8], m_kib: u32, t: u32, p: u32) -> AppResult<Zeroizing<[u8; 32]>> {
    let params = Params::new(m_kib, t, p, Some(32)).map_err(|e| fail(&format!("키 유도 설정 오류: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0u8; 32]);
    argon.hash_password_into(password.as_bytes(), salt, key.as_mut()).map_err(|e| fail(&format!("키 유도 실패: {e}")))?;
    Ok(key)
}

fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut b = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}

/// 암호화 방식 선택.
pub enum Mode<'a> {
    Dpapi,
    Password(&'a str),
}

/// zip 바이트 → .asbk 바이트.
pub fn encrypt(plain: &[u8], mode: Mode) -> AppResult<Vec<u8>> {
    let nonce = random_bytes::<12>();
    let (key, header) = match mode {
        Mode::Dpapi => {
            let key = Zeroizing::new(random_bytes::<32>());
            let wrapped = dpapi(key.as_ref(), true)?;
            (
                key,
                Header { format_version: FORMAT_VERSION, mode: "dpapi".into(), cipher: "AES-256-GCM".into(), nonce_b64: b64().encode(nonce), kdf: None, salt_b64: None, argon2_m_kib: None, argon2_t: None, argon2_p: None, wrapped_key_b64: Some(b64().encode(wrapped)), created_at: crate::ops::history::now(), app_version: env!("CARGO_PKG_VERSION").into() },
            )
        }
        Mode::Password(pw) => {
            if pw.chars().count() < 8 {
                return Err(AppError::invalid("백업 비밀번호는 8자 이상이어야 합니다."));
            }
            let salt = random_bytes::<16>();
            let key = derive_key(pw, &salt, ARGON_M_KIB, ARGON_T, ARGON_P)?;
            (
                key,
                Header { format_version: FORMAT_VERSION, mode: "password".into(), cipher: "AES-256-GCM".into(), nonce_b64: b64().encode(nonce), kdf: Some("argon2id".into()), salt_b64: Some(b64().encode(salt)), argon2_m_kib: Some(ARGON_M_KIB), argon2_t: Some(ARGON_T), argon2_p: Some(ARGON_P), wrapped_key_b64: None, created_at: crate::ops::history::now(), app_version: env!("CARGO_PKG_VERSION").into() },
            )
        }
    };
    let header_json = serde_json::to_string(&header).map_err(|e| fail(&format!("헤더 직렬화 실패: {e}")))?;
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let ct = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: header_json.as_bytes() }).map_err(|_| fail("암호화에 실패했습니다."))?;
    let mut out = Vec::with_capacity(MAGIC.len() + header_json.len() + 1 + ct.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(header_json.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn read_header(bytes: &[u8]) -> AppResult<(Header, usize)> {
    if !bytes.starts_with(MAGIC) {
        return Err(fail("암호화된 백업 형식이 아닙니다."));
    }
    let rest = &bytes[MAGIC.len()..];
    let nl = rest.iter().position(|b| *b == b'\n').ok_or_else(|| fail("암호화된 백업의 헤더가 손상되었습니다."))?;
    let header: Header = serde_json::from_slice(&rest[..nl]).map_err(|_| fail("암호화된 백업의 헤더를 읽지 못했습니다."))?;
    if header.format_version > FORMAT_VERSION {
        return Err(fail(&format!("지원하지 않는 암호화 백업 형식(v{})입니다. 프로그램을 업데이트해 주세요.", header.format_version)));
    }
    Ok((header, MAGIC.len() + nl + 1))
}

/// .asbk 바이트 → zip 바이트. 비밀번호 모드인데 비밀번호가 없으면 NEED_PASSWORD.
pub fn decrypt(bytes: &[u8], password: Option<&str>) -> AppResult<Zeroizing<Vec<u8>>> {
    let (header, off) = read_header(bytes)?;
    let header_json = &bytes[MAGIC.len()..off - 1];
    let nonce = b64().decode(&header.nonce_b64).map_err(|_| fail("헤더(nonce)가 손상되었습니다."))?;
    let key: Zeroizing<[u8; 32]> = match header.mode.as_str() {
        "dpapi" => {
            let wrapped = b64().decode(header.wrapped_key_b64.as_deref().unwrap_or("")).map_err(|_| fail("헤더(키)가 손상되었습니다."))?;
            let raw = Zeroizing::new(dpapi(&wrapped, false)?);
            let mut k = Zeroizing::new([0u8; 32]);
            if raw.len() != 32 {
                return Err(wrong_password_or_damaged());
            }
            k.copy_from_slice(&raw);
            k
        }
        "password" => {
            let Some(pw) = password else { return Err(AppError::new("NEED_PASSWORD", "이 백업은 비밀번호로 암호화되어 있습니다. 비밀번호를 입력해 주세요.")) };
            let salt = b64().decode(header.salt_b64.as_deref().unwrap_or("")).map_err(|_| fail("헤더(salt)가 손상되었습니다."))?;
            derive_key(pw, &salt, header.argon2_m_kib.unwrap_or(ARGON_M_KIB), header.argon2_t.unwrap_or(ARGON_T), header.argon2_p.unwrap_or(ARGON_P))?
        }
        _ => return Err(fail("알 수 없는 암호화 방식입니다.")),
    };
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key.as_ref()));
    let plain = cipher.decrypt(Nonce::from_slice(&nonce), Payload { msg: &bytes[off..], aad: header_json }).map_err(|_| wrong_password_or_damaged())?;
    Ok(Zeroizing::new(plain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip_and_wrong_password() {
        let data = b"zip-bytes-here".repeat(100);
        let enc = encrypt(&data, Mode::Password("correct horse battery")).unwrap();
        assert!(is_encrypted_file(&enc));
        let (h, _) = read_header(&enc).unwrap();
        assert_eq!((h.mode.as_str(), h.kdf.as_deref()), ("password", Some("argon2id")));
        assert!(!String::from_utf8_lossy(&enc).contains("correct horse"), "비밀번호가 파일에 없다");
        assert_eq!(&*decrypt(&enc, Some("correct horse battery")).unwrap(), &data);
        let e = decrypt(&enc, Some("wrong password!")).unwrap_err();
        assert_eq!(e.code, "DECRYPT");
        assert!(!e.message.contains("wrong"), "오류 메시지에 비밀번호가 없다");
        assert_eq!(decrypt(&enc, None).unwrap_err().code, "NEED_PASSWORD");
        assert_eq!(encrypt(&data, Mode::Password("short")).unwrap_err().code, "INVALID");
        // 손상
        let mut bad = enc.clone();
        let n = bad.len();
        bad[n - 5] ^= 0x01;
        assert_eq!(decrypt(&bad, Some("correct horse battery")).unwrap_err().code, "DECRYPT");
        // 헤더 변조(AAD)
        let mut bad2 = enc.clone();
        let pos = MAGIC.len() + 5;
        bad2[pos] = if bad2[pos] == b'a' { b'b' } else { b'a' };
        assert!(decrypt(&bad2, Some("correct horse battery")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_round_trip_for_current_account() {
        let data = b"account-bound".to_vec();
        let enc = encrypt(&data, Mode::Dpapi).unwrap();
        let (h, _) = read_header(&enc).unwrap();
        assert_eq!(h.mode, "dpapi");
        assert!(h.wrapped_key_b64.is_some() && h.salt_b64.is_none());
        assert_eq!(&*decrypt(&enc, None).unwrap(), &data);
        let mut bad = enc.clone();
        let n = bad.len();
        bad[n - 1] ^= 0xFF;
        assert_eq!(decrypt(&bad, None).unwrap_err().code, "DECRYPT");
    }
}
