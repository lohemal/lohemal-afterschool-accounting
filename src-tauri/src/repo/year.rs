//! 학년도 · 회계월.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::months;
use crate::error::{AppError, AppResult};
use crate::model::{FiscalMonth, SchoolYear};
use crate::repo::audit;

fn map_year(r: &Row) -> rusqlite::Result<SchoolYear> {
    Ok(SchoolYear {
        id: r.get(0)?,
        year: r.get(1)?,
        name: r.get(2)?,
        start_ym: r.get(3)?,
        end_ym: r.get(4)?,
        is_current: r.get::<_, i64>(5)? == 1,
        created_at: r.get(6)?,
    })
}

const YEAR_COLS: &str = "id, year, name, start_ym, end_ym, is_current, created_at";

pub fn list(conn: &Connection) -> AppResult<Vec<SchoolYear>> {
    let mut st = conn.prepare(&format!(
        "SELECT {YEAR_COLS} FROM school_year ORDER BY year DESC"
    ))?;
    let rows = st.query_map([], map_year)?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<SchoolYear> {
    conn.query_row(
        &format!("SELECT {YEAR_COLS} FROM school_year WHERE id = ?1"),
        [id],
        map_year,
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("학년도를 찾지 못했습니다."))
}

pub fn current(conn: &Connection) -> AppResult<Option<SchoolYear>> {
    Ok(conn
        .query_row(
            &format!("SELECT {YEAR_COLS} FROM school_year WHERE is_current = 1"),
            [],
            map_year,
        )
        .optional()?)
}

/// 학년도와 12개 회계월을 함께 만든다. 첫 학년도는 자동으로 현재 학년도가 된다.
pub fn create(conn: &Connection, year: i32) -> AppResult<SchoolYear> {
    if !(2000..=2100).contains(&year) {
        return Err(AppError::invalid("학년도는 2000~2100 사이여야 합니다."));
    }
    let has_current: i64 =
        conn.query_row("SELECT count(*) FROM school_year WHERE is_current = 1", [], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO school_year (year, name, start_ym, end_ym, is_current)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            year,
            months::year_name(year),
            months::start_ym(year),
            months::end_ym(year),
            if has_current == 0 { 1 } else { 0 }
        ],
    )?;
    let id = conn.last_insert_rowid();
    for (seq, ym) in months::months_of(year) {
        conn.execute(
            "INSERT INTO fiscal_month (school_year_id, ym, seq) VALUES (?1, ?2, ?3)",
            params![id, ym, seq],
        )?;
    }
    audit::write(
        conn,
        "year.create",
        "school_year",
        Some(id),
        &format!("{} 생성 (회계월 12개)", months::year_name(year)),
        serde_json::json!({ "year": year }),
    )?;
    get(conn, id)
}

pub fn set_current(conn: &Connection, id: i64) -> AppResult<SchoolYear> {
    let y = get(conn, id)?;
    conn.execute("UPDATE school_year SET is_current = 0 WHERE is_current = 1", [])?;
    conn.execute("UPDATE school_year SET is_current = 1 WHERE id = ?1", [id])?;
    audit::write(
        conn,
        "year.set_current",
        "school_year",
        Some(id),
        &format!("현재 학년도를 {} 로 바꿈", y.name),
        serde_json::json!({}),
    )?;
    get(conn, id)
}

/// 학년도의 회계월 12개. 합계는 ledger_entry 에서 그때그때 계산한다.
pub fn months(conn: &Connection, school_year_id: i64) -> AppResult<Vec<FiscalMonth>> {
    let mut st = conn.prepare(
        "SELECT m.id, m.school_year_id, m.ym, m.seq, m.status,
                (SELECT count(*) FROM ledger_entry e WHERE e.fiscal_month_id = m.id AND e.state = 'active'),
                (SELECT coalesce(sum(e.amount), 0) FROM ledger_entry e
                   WHERE e.fiscal_month_id = m.id AND e.state = 'active' AND e.flow = 'collection'),
                (SELECT coalesce(sum(e.amount), 0) FROM ledger_entry e
                   WHERE e.fiscal_month_id = m.id AND e.state = 'active' AND e.flow = 'expenditure'),
                (SELECT max(c.created_at) FROM month_close c
                   WHERE c.fiscal_month_id = m.id AND c.action = 'close'),
                (SELECT count(*) FROM ledger_entry e
                   WHERE e.fiscal_month_id = m.id
                     AND e.created_at > coalesce((SELECT max(c.created_at) FROM month_close c
                                                   WHERE c.fiscal_month_id = m.id AND c.action = 'close'), ''))
           FROM fiscal_month m
          WHERE m.school_year_id = ?1
          ORDER BY m.seq",
    )?;
    let rows = st.query_map([school_year_id], |r| {
        let ym: String = r.get(2)?;
        let status: String = r.get(4)?;
        let last_closed_at: Option<String> = r.get(8)?;
        let after: i64 = r.get(9)?;
        Ok(FiscalMonth {
            id: r.get(0)?,
            school_year_id: r.get(1)?,
            label: months::label(&ym),
            ym,
            seq: r.get(3)?,
            changed_after_close: status == "closed" && last_closed_at.is_some() && after > 0,
            status,
            entry_count: r.get(5)?,
            collection_total: r.get(6)?,
            expenditure_total: r.get(7)?,
            last_closed_at,
        })
    })?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}

pub fn month_get(conn: &Connection, school_year_id: i64, id: i64) -> AppResult<FiscalMonth> {
    months(conn, school_year_id)?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::not_found("회계월을 찾지 못했습니다."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn creating_a_year_makes_twelve_months_march_to_february() {
        let db = Db::open_memory().unwrap();
        let y = db.write(|tx| create(tx, 2026)).unwrap();
        assert_eq!(y.name, "2026학년도");
        assert_eq!(y.start_ym, "2026-03");
        assert_eq!(y.end_ym, "2027-02");
        assert!(y.is_current, "첫 학년도는 자동으로 현재 학년도");

        let ms = db.read(|c| months(c, y.id)).unwrap();
        assert_eq!(ms.len(), 12);
        assert_eq!(ms[0].ym, "2026-03");
        assert_eq!(ms[0].label, "3월");
        assert_eq!(ms[11].ym, "2027-02");
        assert_eq!(ms[11].seq, 12);
        assert!(ms.iter().all(|m| m.entry_count == 0 && m.collection_total == 0));
        assert!(ms.iter().all(|m| m.status == "open" && !m.changed_after_close));
    }

    #[test]
    fn duplicate_year_is_a_conflict() {
        let db = Db::open_memory().unwrap();
        db.write(|tx| create(tx, 2026)).unwrap();
        let err = db.write(|tx| create(tx, 2026)).unwrap_err();
        assert_eq!(err.code, "CONFLICT");
        assert_eq!(err.message, "같은 학년도가 이미 있습니다.");
    }

    #[test]
    fn only_one_current_year() {
        let db = Db::open_memory().unwrap();
        let a = db.write(|tx| create(tx, 2026)).unwrap();
        let b = db.write(|tx| create(tx, 2027)).unwrap();
        assert!(a.is_current && !b.is_current);
        db.write(|tx| set_current(tx, b.id)).unwrap();
        let cur = db.read(current).unwrap().unwrap();
        assert_eq!(cur.id, b.id);
        let n: i64 = db
            .read(|c| Ok(c.query_row("SELECT count(*) FROM school_year WHERE is_current = 1", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 1);
        // 변경 이력이 남는다.
        let log = db.read(|c| audit::list(c, 10)).unwrap();
        assert!(log.iter().any(|l| l.kind == "year.set_current"));
        assert!(log.iter().any(|l| l.kind == "year.create"));
    }

    #[test]
    fn out_of_range_year_is_rejected() {
        let db = Db::open_memory().unwrap();
        assert_eq!(db.write(|tx| create(tx, 1999)).unwrap_err().code, "INVALID");
    }
}
