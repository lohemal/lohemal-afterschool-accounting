//! `#[tauri::command]` 계층. **로직을 두지 않는다** — 입력을 받아 repo/domain 에 넘기고
//! 결과를 돌려줄 뿐이다.

pub mod adjust;
pub mod app;
pub mod catalog;
pub mod ledger;
pub mod link;
pub mod ops;
pub mod recon;
pub mod upload;
pub mod year;
pub mod voucher;
