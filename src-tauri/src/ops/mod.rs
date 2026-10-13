//! 운영 작업 — 백업 · 복원 · 작업 이력 · 내보내기. 회계 계산이 아니라 자료 보존과 복구를 다룬다.
//!
//! 작업 이력(`ops-history.jsonl`)은 DB 밖 파일이다. 과거 DB 를 복원해도 복원 작업 자체의 기록은 남는다.

pub mod backup;
pub mod crypto;
pub mod export;
pub mod history;
pub mod restore;

#[cfg(test)]
#[path = "ops_tests.rs"]
mod tests;
