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
//!
//! 표를 갈아 끼우는 마이그레이션(`fk_off`)은 외래키를 끄고 돌린다. 외래키를 켠 채
//! `DROP TABLE` 하면 자식 행이 함께 지워지기 때문이다. 대신 **커밋 전에**
//! `foreign_key_check` 로 확인하므로 어긋나면 아무것도 적용되지 않는다.

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

struct Migration {
    version: i32,
    name: &'static str,
    sql: &'static str,
    fk_off: bool,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "001_init",
        sql: include_str!("../../migrations/001_init.sql"),
        fk_off: false,
    },
    // ledger_entry · program_alias 재생성 — 외래키를 끄고 돌린다.
    Migration {
        version: 2,
        name: "002_upload_versions",
        sql: include_str!("../../migrations/002_upload_versions.sql"),
        fk_off: true,
    },
    // 배분 · 확인 메모 표만 더한다.
    Migration {
        version: 3,
        name: "003_allocation_recon",
        sql: include_str!("../../migrations/003_allocation_recon.sql"),
        fk_off: false,
    },
    // 기본 재원 이름을 지시서 용어로 맞춘다 (사용자가 바꾼 이름은 건드리지 않는다). 표 구조 변경 없음.
    Migration {
        version: 4,
        name: "004_fund_names",
        sql: include_str!("../../migrations/004_fund_names.sql"),
        fk_off: false,
    },
    // 수동 정정(재원경정·환불·추가징수) 표. 기존 표는 건드리지 않는다.
    Migration {
        version: 5,
        name: "005_adjustment",
        sql: include_str!("../../migrations/005_adjustment.sql"),
        fk_off: false,
    },
    // 자유수강권 정책·대상자·묶음 표. 기존 표는 건드리지 않는다.
    Migration {
        version: 6,
        name: "006_voucher",
        sql: include_str!("../../migrations/006_voucher.sql"),
        fk_off: false,
    },
    // 정책에 학기 기간 칸을 더하고 지원 대상 항목 표를 만든다. 기존 행 보존.
    Migration {
        version: 7,
        name: "007_voucher_policy",
        sql: include_str!("../../migrations/007_voucher_policy.sql"),
        fk_off: false,
    },
    // 방과후이용권 대상 여부 · 학기 공백 확인 표시. 기존 표 보존.
    Migration {
        version: 8,
        name: "008_ops",
        sql: include_str!("../../migrations/008_ops.sql"),
        fk_off: false,
    },
    // 월 확정 지문 칸. 기존 확정 이력 보존.
    Migration {
        version: 9,
        name: "009_close_fingerprint",
        sql: include_str!("../../migrations/009_close_fingerprint.sql"),
        fk_off: false,
    },
];

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
        // 자료 보존이 먼저다: 직전 백업을 만들고 검증하지 못하면 마이그레이션을 시작하지 않는다 (DB 는 옛 버전 그대로).
        match backup_before_migrate(conn, from) {
            Ok(Some(name)) => log::info!("마이그레이션 전 백업: {name}"),
            Ok(None) => {}
            Err(e) => {
                return Err(AppError::new(
                    "MIGRATE_BACKUP",
                    format!("자료 구조를 갱신하기 전에 백업을 만들지 못해 갱신을 시작하지 않았습니다. 백업 폴더를 확인한 뒤 다시 실행해 주세요. ({})", e.message),
                )
                .detail(e.detail.unwrap_or_default()));
            }
        }
    }
    apply(conn, from)
}

/// 테스트용 — 특정 버전까지만 적용한다 (구버전 DB 를 흉내 낼 때).
#[cfg(test)]
pub fn run_up_to(conn: &mut Connection, to: i32) -> AppResult<()> {
    let from = current_version(conn)?;
    for m in MIGRATIONS.iter().filter(|m| m.version > from && m.version <= to) {
        if m.fk_off {
            apply_one_without_fk(conn, m)?;
        } else {
            apply_one(conn, m)?;
        }
    }
    Ok(())
}

fn apply(conn: &mut Connection, from: i32) -> AppResult<()> {
    for m in MIGRATIONS.iter().filter(|m| m.version > from) {
        if m.fk_off {
            apply_one_without_fk(conn, m)?;
        } else {
            apply_one(conn, m)?;
        }
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

/// PRAGMA foreign_keys 는 트랜잭션 안에서 듣지 않으므로 바깥에서 끄고, 끝나면 반드시 켠다.
fn apply_one_without_fk(conn: &mut Connection, m: &Migration) -> AppResult<()> {
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let result = apply_one(conn, m);
    let restored = conn.pragma_update(None, "foreign_keys", "ON");
    result?;
    restored?;
    Ok(())
}

/// 마이그레이션 직전 백업 — P5 백업 패키지(zip, 검증 포함)로 만든다. 메모리 DB 면 건너뛴다.
fn backup_before_migrate(conn: &Connection, from: i32) -> AppResult<Option<String>> {
    let Some(path) = conn.path().map(|p| p.to_string()) else {
        return Ok(None);
    };
    if path.is_empty() || path == ":memory:" {
        return Ok(None);
    }
    let db_path = std::path::Path::new(&path);
    let data_dir = db_path.parent().ok_or_else(|| AppError::new("IO", "자료 폴더를 찾지 못했습니다."))?;
    // 설정 표가 있으면 설정대로(지정 경로·암호화), 없으면 기본 폴더에 암호화 없이.
    let has_setting: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'app_setting'", [], |r| r.get(0)).unwrap_or(0);
    let info = if has_setting > 0 {
        crate::ops::backup::create(conn, data_dir, "migration", &format!("스키마 v{from} → v{} 마이그레이션 직전", latest_version()))?
    } else {
        crate::ops::backup::create_opts(conn, data_dir, "migration", &format!("스키마 v{from} → v{} 마이그레이션 직전", latest_version()), &crate::ops::backup::CreateOpts { dir: crate::ops::backup::default_dir(data_dir), encryption: "none", password: None, fail_at: None })?
    };
    Ok(Some(info.file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

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
        run(&mut c).unwrap();
        assert_eq!(current_version(&c).unwrap(), latest_version());
        let fk: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0)).unwrap();
        assert_eq!(fk, 1, "fk_off 마이그레이션 뒤에도 외래키는 다시 켜진다");
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
    fn v1_data_survives_the_v2_rebuild() {
        // v1 스키마에 학년도·프로그램·거래를 넣고 v2 로 올린다. id 와 금액이 그대로여야 한다.
        let mut c = Connection::open_in_memory().unwrap();
        c.pragma_update(None, "foreign_keys", "ON").unwrap();
        run_up_to(&mut c, 1).unwrap();
        assert_eq!(current_version(&c).unwrap(), 1);
        c.execute("INSERT INTO school_year (year, name, start_ym, end_ym, is_current) VALUES (2026, '2026학년도', '2026-03', '2027-02', 1)", []).unwrap();
        c.execute("INSERT INTO fiscal_month (school_year_id, ym, seq) VALUES (1, '2026-03', 1)", []).unwrap();
        c.execute("INSERT INTO program (school_year_id, name) VALUES (1, '코딩')", []).unwrap();
        c.execute("INSERT INTO program_alias (school_year_id, program_id, alias) VALUES (1, 1, '코딩A')", []).unwrap();
        let g3: i64 = c.query_row("SELECT id FROM fund_type WHERE code = 'g3_support'", [], |r| r.get(0)).unwrap();
        c.execute(
            "INSERT INTO ledger_entry (school_year_id, fiscal_month_id, program_id, fund_type_id, amount_item_id, flow, entry_kind, amount)
             VALUES (1, 1, 1, ?1, 1, 'collection', 'initial_collect', 70000)",
            params![g3],
        )
        .unwrap();
        run(&mut c).unwrap();
        assert_eq!(current_version(&c).unwrap(), latest_version());
        let (id, amount, fund, prog): (i64, i64, i64, i64) = c
            .query_row("SELECT id, amount, fund_type_id, program_id FROM ledger_entry", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap();
        assert_eq!((id, amount, fund, prog), (1, 70000, g3, 1), "기존 거래의 id·금액·재원 id 유지");
        let merged: Option<i64> = c.query_row("SELECT merged_into_id FROM fund_type WHERE id = ?1", [g3], |r| r.get(0)).unwrap();
        let av: i64 = c.query_row("SELECT id FROM fund_type WHERE code = 'after_voucher'", [], |r| r.get(0)).unwrap();
        assert_eq!(merged, Some(av), "초3 지원금은 방과후이용권으로 접힌다");
        let alias: String = c.query_row("SELECT alias FROM program_alias WHERE program_id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(alias, "코딩A");
        // 재생성된 표에서도 원장 불변성이 유지된다.
        let err = c.execute("UPDATE ledger_entry SET amount = 1 WHERE id = 1", []).unwrap_err();
        assert!(err.to_string().contains("LEDGER_IMMUTABLE"));
        let err = c.execute("DELETE FROM ledger_entry WHERE id = 1", []).unwrap_err();
        assert!(err.to_string().contains("LEDGER_NO_DELETE"));
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
            "school_year", "fiscal_month", "fund_type", "fund_type_alias", "amount_item", "program", "program_alias",
            "program_group", "program_group_member", "student", "upload_file", "upload_row", "upload_apply",
            "source_row", "ledger_entry", "ledger_correction", "month_close", "audit_log", "app_setting", "allocation", "recon_note", "adjustment", "adjustment_entry", "adjustment_status_log",
            "voucher_policy", "voucher_exception", "voucher_target", "voucher_target_log", "voucher_batch", "voucher_batch_item", "voucher_refund", "voucher_policy_item",
            "after_voucher_target", "after_voucher_target_log",
        ] {
            assert!(names.iter().any(|n| n == t), "표 {t} 가 없다: {names:?}");
        }
        assert!(!names.iter().any(|n| n.ends_with("_new")), "임시 표가 남아 있다");
    }
}
