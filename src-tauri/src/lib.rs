//! 방과후 회계 검증 매니저 — 앱 진입점.
//!
//! 계층
//!   `commands`  얇은 IPC 껍데기 (로직 없음)
//!   `repo`      SQL
//!   `domain`    업무 규칙 (DB 의존 없음)
//!   `db`        연결·마이그레이션
//!
//! 업무 자료는 실행 파일과 떨어진 곳(`%APPDATA%\kr.school.asaccounting`)에 둔다.
//! 학생·회계 자료는 이 컴퓨터 밖으로 나가지 않는다.

pub mod commands;
pub mod db;
pub mod domain;
pub mod error;
pub mod excel;
pub mod logging;
pub mod model;
pub mod repo;

use tauri::Manager;

use crate::db::Db;

pub const DB_FILE: &str = "asaccounting.db";

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // 개발·시험용: 환경변수가 있으면 실사용 자료 폴더 대신 그 폴더를 쓴다 (연습용 DB).
            let dir = match std::env::var("ASACCOUNTING_DATA_DIR") {
                Ok(d) if !d.trim().is_empty() => std::path::PathBuf::from(d),
                _ => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&dir)?;
            logging::init(&dir.join("logs"));
            log::info!(
                "방과후 회계 검증 매니저 {} 시작 · 자료 폴더 {}",
                env!("CARGO_PKG_VERSION"),
                dir.display()
            );
            let db = Db::open(&dir.join(DB_FILE)).map_err(|e| {
                log::error!("자료 파일 열기 실패: {} ({:?})", e.message, e.detail);
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("{} ({})", e.message, e.detail.unwrap_or_default()),
                )
            })?;
            // 반영 도중 앱이 꺼져 DB 에 없는 보관 파일이 남았으면 치운다.
            if let Err(e) = db.read(|c| repo::upload::cleanup_orphans(c, &dir)) {
                log::warn!("보관 파일 정리 실패(계속 진행): {}", e.message);
            }
            app.manage(db);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::bootstrap,
            commands::app::setting_get,
            commands::app::setting_set,
            commands::app::audit_list,
            commands::app::open_data_folder,
            commands::year::year_list,
            commands::year::year_create,
            commands::year::year_set_current,
            commands::year::month_list,
            commands::catalog::catalog_list,
            commands::catalog::fund_type_save,
            commands::catalog::amount_item_save,
            commands::ledger::month_summary,
            commands::ledger::program_list,
            commands::upload::upload_inspect,
            commands::upload::upload_preview,
            commands::upload::upload_commit,
            commands::upload::upload_list,
            commands::upload::upload_detail,
            commands::upload::upload_impact,
            commands::upload::upload_cancel,
            commands::upload::upload_restore,
            commands::upload::upload_kinds,
            commands::upload::program_map_save,
            commands::upload::program_group_list,
            commands::upload::program_alias_delete,
            commands::upload::fund_alias_list,
            commands::upload::fund_alias_save,
            commands::upload::fund_alias_delete,
            commands::upload::month_close,
            commands::upload::month_reopen,
            commands::recon::allocation_board,
            commands::recon::allocation_auto,
            commands::recon::allocation_manual,
            commands::recon::allocation_void,
            commands::recon::recon_month,
            commands::recon::recon_detail,
            commands::recon::recon_note_save,
            commands::recon::source_row_view,
            commands::recon::open_stored_file,
            commands::adjust::adjustment_preview,
            commands::adjust::adjustment_create,
            commands::adjust::adjustment_list,
            commands::adjust::adjustment_set_status,
            commands::adjust::adjustment_cancel,
            commands::adjust::student_list,
            commands::adjust::entry_picks,
        ])
        .run(tauri::generate_context!())
        .expect("앱을 시작하지 못했습니다");
}
