//! SHA-256 — 파일 중복 감지(upload_file.sha256)와 원본 행 해시에 쓴다.

use sha2::{Digest, Sha256};

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn sha256_file(path: &std::path::Path) -> std::io::Result<(String, u64)> {
    let bytes = std::fs::read(path)?;
    Ok((sha256_hex(&bytes), bytes.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_hex(b"").len(), 64);
    }
}
