//! 마이그레이션 러너.
//!
//! `PRAGMA user_version` 을 스키마 버전으로 쓰고, 아래 목록을 번호순으로 적용한다.
//! 적용 전에 기존 자료를 SQLite 백업 API 로 `backups/` 에 떠 둔다 — 파일 복사는
//! 아직 WAL 에만 있는 내용을 놓친다.
//!
//! 새 마이그레이션 추가 방법
//!   1. `migrations/00N_설명.sql` 파일 생성
//!   2. 아래 MIGRATIONS 배열에 한 줄 추가
//! 기존 파일은 절대 수정하지 않는다 (이미 적용된 사용자가 있으므로).
//! 단, **첫 릴리스 전**에는 001 을 직접 고치되 연습용 DB 파일을 지우고 다시 만든다.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

struct Migration {
    version: i32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "001_init",
    sql: include_str!("../../migrations/001_init.sql"),
}];

pub fn latest_version() -> i32 {
    MIGRATIONS.iter().map(|m| m.version).max().unwrap_or(0)
}

pub fn current_version(conn: &Connection) -> rusqlite::Result<i32> {
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
}

pub fn run(conn: &mut Connection) -> AppResult<()> {
    let from = current_version(conn)?;
    let to = latest_version();
    if from > to {
        return Err(AppError::new(
            "MIGRATE",
            "이 자료 파일은 더 새로운 버전의 프로그램으로 만들어졌습니다. 프로그램을 업데이트해 주세요.",
        )
        .detail(format!("user_version {from} > {to}")));
    }
    if from == to {
        return Ok(());
    }
    if from > 0 {
        match backup_before_migrate(conn, from) {
            Ok(Some(name)) => log::info!("마이그레이션 전 백업: {name}"),
            Ok(None) => {}
            Err(e) => log::warn!("마이그레이션 전 백업 실패(계속 진행): {e}"),
        }
    }
    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        apply_one(conn, m)?;
        log::info!("마이그레이션 적용: {}", m.name);
    }
    Ok(())
}

fn apply_one(conn: &mut Connection, m: &Migration) -> AppResult<()> {
    let tx = conn.transaction()?;
    tx.execute_batch(m.sql).map_err(|e| {
        AppError::new("MIGRATE", "자료 구조를 갱신하지 못했습니다.")
            .detail(format!("{} :: {e}", m.name))
    })?;
    // 커밋하기 전에 외래키가 어긋나지 않았는지 본다.
    let broken = {
        let mut st = tx.prepare("PRAGMA foreign_key_check")?;
        let n = st.query_map([], |_| Ok(()))?.count();
        n
    };
    if broken > 0 {
        return Err(AppError::new(
            "MIGRATE",
            "자료 구조를 갱신한 뒤 연결이 어긋나 갱신을 되돌렸습니다.",
        )
        .detail(format!("{} :: foreign_key_check {broken}건", m.name)));
    }
    tx.pragma_update(None, "user_version", m.version)?;
    tx.commit()?;
    Ok(())
}

/// 마이그레이션 직전 백업. 메모리 DB 면 건너뛴다.
fn backup_before_migrate(conn: &Connection, from: i32) -> AppResult<Option<String>> {
    let Some(path) = conn.path().map(|p| p.to_string()) else {
        return Ok(None);
    };
    if path.is_empty() || path == ":memory:" {
        return Ok(None);
    }
    let db_path = std::path::Path::new(&path);
    let dir = db_path
        .parent()
        .ok_or_else(|| AppError::new("IO", "자료 폴더를 찾지 못했습니다."))?
        .join("backups");
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "before-migrate-v{from}-{}.db",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let dest = dir.join(&name);
    let mut out = Connection::open(&dest)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut out)?;
    backup.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
    Ok(Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        run(&mut c).unwrap();
        c
    }

    #[test]
    fn applies_to_latest_and_is_idempotent() {
        let mut c = fresh();
        assert_eq!(current_version(&c).unwrap(), latest_version());
        // 두 번 돌려도 아무 일도 하지 않는다.
        run(&mut c).unwrap();
        assert_eq!(current_version(&c).unwrap(), latest_version());
    }

    #[test]
    fn seeds_builtin_fund_types_and_amount_items() {
        let c = fresh();
        let funds: i64 = c
            .query_row("SELECT count(*) FROM fund_type WHERE is_builtin = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(funds, 7);
        let items: i64 = c
            .query_row("SELECT count(*) FROM amount_item WHERE is_builtin = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(items, 6);
        // 초3 수익자 부담금은 수익자의 하위 구분이다.
        let parent: String = c
            .query_row(
                "SELECT p.code FROM fund_type f JOIN fund_type p ON p.id = f.parent_id WHERE f.code = 'g3_excess'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent, "beneficiary");
    }

    #[test]
    fn newer_db_is_refused() {
        let c = fresh();
        c.pragma_update(None, "user_version", latest_version() + 5).unwrap();
        let mut c = c;
        let err = run(&mut c).unwrap_err();
        assert_eq!(err.code, "MIGRATE");
    }

    #[test]
    fn all_expected_tables_exist() {
        let c = fresh();
        let mut st = c
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let names: Vec<String> = st
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for t in [
            "school_year",
            "fiscal_month",
            "fund_type",
            "amount_item",
            "program",
            "program_alias",
            "student",
            "upload_file",
            "source_row",
            "ledger_entry",
            "ledger_correction",
            "month_close",
            "audit_log",
            "app_setting",
        ] {
            assert!(names.iter().any(|n| n == t), "표 {t} 가 없다: {names:?}");
        }
    }
}
