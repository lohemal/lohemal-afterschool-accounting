//! 변경 이력. 사람이 읽는 한 줄 요약만 남기고 **개인정보·금액은 넣지 않는다.**

use rusqlite::{params, Connection};

use crate::error::AppResult;
use crate::model::AuditLog;

pub fn write(
    conn: &Connection,
    kind: &str,
    target_table: &str,
    target_id: Option<i64>,
    summary: &str,
    detail: serde_json::Value,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO audit_log (kind, target_table, target_id, summary, detail_json)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, target_table, target_id, summary, detail.to_string()],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(conn: &Connection, limit: i64) -> AppResult<Vec<AuditLog>> {
    let mut st = conn.prepare(
        "SELECT id, at, kind, target_table, target_id, summary
           FROM audit_log ORDER BY id DESC LIMIT ?1",
    )?;
    let rows = st.query_map([limit], |r| {
        Ok(AuditLog {
            id: r.get(0)?,
            at: r.get(1)?,
            kind: r.get(2)?,
            target_table: r.get(3)?,
            target_id: r.get(4)?,
            summary: r.get(5)?,
        })
    })?;
    let out: Result<Vec<_>, _> = rows.collect();
    Ok(out?)
}
