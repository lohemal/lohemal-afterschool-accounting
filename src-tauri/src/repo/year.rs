//! 학년도 · 회계월.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::months;
use crate::error::{AppError, AppResult};
use crate::model::{FiscalMonth, MonthChanges, SchoolYear};
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
    let mut out = out?;
    // 확정된 달은 지문으로 변경 여부를 다시 판정한다 (옛 확정은 시각 비교 결과 유지)
    for m in out.iter_mut().filter(|m| m.status == "closed") {
        if let Ok(ch) = month_changes(conn, m.id) {
            if ch.has_fingerprint {
                m.changed_after_close = ch.changed;
            }
        }
    }
    Ok(out)
}

pub fn month_is_closed(conn: &Connection, fiscal_month_id: i64) -> AppResult<bool> {
    let status: Option<String> = conn
        .query_row("SELECT status FROM fiscal_month WHERE id = ?1", [fiscal_month_id], |r| r.get(0))
        .optional()?;
    match status {
        Some(s) => Ok(s == "closed"),
        None => Err(AppError::not_found("회계월을 찾지 못했습니다.")),
    }
}

pub fn month_label_of(conn: &Connection, fiscal_month_id: i64) -> AppResult<String> {
    let ym: Option<String> = conn
        .query_row("SELECT ym FROM fiscal_month WHERE id = ?1", [fiscal_month_id], |r| r.get(0))
        .optional()?;
    Ok(ym.map(|y| format!("{} ({y})", months::label(&y))).unwrap_or_default())
}

/// 확정 대상 자료의 정규화 지문 — 살아 있는/취소된 원장 거래 전부(id·상태·단계·금액·분류·처리일), 배분, 정정 상태를
/// id 순으로 고정 형식으로 이어 붙여 SHA-256. 시각은 넣지 않는다(같은 초 변경도 잡힌다).
pub fn month_fingerprint(conn: &Connection, fiscal_month_id: i64) -> AppResult<(String, serde_json::Value)> {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    let mut entries: Vec<(i64, String, String)> = Vec::new();
    {
        let mut st = conn.prepare(
            "SELECT id, state, stage, amount, coalesce(fund_type_id, 0), coalesce(program_id, 0), coalesce(program_group_id, 0), coalesce(student_id, 0), amount_item_id, flow, entry_kind, coalesce(processed_on, ''), coalesce(transfer_group_id, 0)
               FROM ledger_entry WHERE fiscal_month_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map([fiscal_month_id], |r| {
            Ok((
                r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, i64>(6)?, r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?, r.get::<_, String>(9)?, r.get::<_, String>(10)?, r.get::<_, String>(11)?, r.get::<_, i64>(12)?,
            ))
        })?;
        for row in rows {
            let (id, state, stage, amount, fund, prog, grp, stu, item, flow, kind, processed, tg) = row?;
            h.update(format!("e|{id}|{state}|{stage}|{amount}|{fund}|{prog}|{grp}|{stu}|{item}|{flow}|{kind}|{processed}|{tg}\n").as_bytes());
            entries.push((id, state, stage));
        }
    }
    let mut allocs: Vec<(i64, String)> = Vec::new();
    {
        let mut st = conn.prepare("SELECT a.id, a.entry_id, a.program_id, a.amount, a.status FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id WHERE e.fiscal_month_id = ?1 ORDER BY a.id")?;
        let rows = st.query_map([fiscal_month_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?)))?;
        for row in rows {
            let (id, e, p, a, s) = row?;
            h.update(format!("a|{id}|{e}|{p}|{a}|{s}\n").as_bytes());
            allocs.push((id, s));
        }
    }
    let mut adjs: Vec<(i64, String)> = Vec::new();
    {
        let mut st = conn.prepare("SELECT id, status, coalesce(processed_on, ''), amount FROM adjustment WHERE fiscal_month_id = ?1 ORDER BY id")?;
        let rows = st.query_map([fiscal_month_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)))?;
        for row in rows {
            let (id, s, p, a) = row?;
            h.update(format!("j|{id}|{s}|{p}|{a}\n").as_bytes());
            adjs.push((id, s));
        }
    }
    let snapshot = serde_json::json!({ "entries": entries, "allocations": allocs, "adjustments": adjs });
    Ok((hex::encode(h.finalize()), snapshot))
}

/// 마지막 확정 이후 무엇이 달라졌는지 — 지문이 다르면 스냅샷과 지금을 비교해 거래·정정·배분 단위로 가려낸다.
pub fn month_changes(conn: &Connection, fiscal_month_id: i64) -> AppResult<MonthChanges> {
    let status: String = conn.query_row("SELECT status FROM fiscal_month WHERE id = ?1", [fiscal_month_id], |r| r.get(0)).optional()?.ok_or_else(|| AppError::not_found("회계월을 찾지 못했습니다."))?;
    let last: Option<(String, String, String)> = conn
        .query_row("SELECT created_at, fingerprint, snapshot_json FROM month_close WHERE fiscal_month_id = ?1 AND action = 'close' ORDER BY id DESC LIMIT 1", [fiscal_month_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    let mut out = MonthChanges { fiscal_month_id, status: status.clone(), closed_at: None, has_fingerprint: false, changed: false, new_entry_ids: vec![], changed_entry_ids: vec![], adjustment_changes: vec![], allocation_changed: false, summary: String::new() };
    let Some((closed_at, fp, snap)) = last else {
        out.summary = "확정 이력이 없습니다.".into();
        return Ok(out);
    };
    out.closed_at = Some(closed_at.clone());
    if fp.is_empty() {
        // 옛 확정(지문 없음): 시각 비교로만 판단
        let after: i64 = conn.query_row("SELECT count(*) FROM ledger_entry WHERE fiscal_month_id = ?1 AND created_at > ?2", params![fiscal_month_id, closed_at], |r| r.get(0))?;
        out.changed = after > 0;
        out.summary = if out.changed { format!("확정 뒤 새 거래 {after}건 (옛 확정 — 시각 비교)") } else { "변경 없음 (옛 확정 — 시각 비교)".into() };
        return Ok(out);
    }
    out.has_fingerprint = true;
    let (now_fp, now_snap) = month_fingerprint(conn, fiscal_month_id)?;
    if now_fp == fp {
        out.summary = "확정 당시와 같습니다.".into();
        return Ok(out);
    }
    out.changed = true;
    let old: serde_json::Value = serde_json::from_str(&snap).unwrap_or(serde_json::json!({}));
    let old_entries: std::collections::BTreeMap<i64, (String, String)> = old["entries"].as_array().map(|a| a.iter().filter_map(|x| Some((x[0].as_i64()?, (x[1].as_str()?.to_string(), x[2].as_str()?.to_string())))).collect()).unwrap_or_default();
    for x in now_snap["entries"].as_array().into_iter().flatten() {
        let (id, st, sg) = (x[0].as_i64().unwrap_or(0), x[1].as_str().unwrap_or(""), x[2].as_str().unwrap_or(""));
        match old_entries.get(&id) {
            None => out.new_entry_ids.push(id),
            Some((os, og)) if os != st || og != sg => out.changed_entry_ids.push(id),
            _ => {}
        }
    }
    let old_adjs: std::collections::BTreeMap<i64, String> = old["adjustments"].as_array().map(|a| a.iter().filter_map(|x| Some((x[0].as_i64()?, x[1].as_str()?.to_string()))).collect()).unwrap_or_default();
    for x in now_snap["adjustments"].as_array().into_iter().flatten() {
        let (id, st) = (x[0].as_i64().unwrap_or(0), x[1].as_str().unwrap_or("").to_string());
        match old_adjs.get(&id) {
            None => out.adjustment_changes.push((id, String::new(), st)),
            Some(os) if *os != st => out.adjustment_changes.push((id, os.clone(), st)),
            _ => {}
        }
    }
    out.allocation_changed = old["allocations"] != now_snap["allocations"];
    let mut parts = Vec::new();
    if !out.new_entry_ids.is_empty() {
        parts.push(format!("새 거래 {}건", out.new_entry_ids.len()));
    }
    if !out.changed_entry_ids.is_empty() {
        parts.push(format!("상태가 바뀐 거래 {}건", out.changed_entry_ids.len()));
    }
    if !out.adjustment_changes.is_empty() {
        parts.push(format!("정정 변경 {}건", out.adjustment_changes.len()));
    }
    if out.allocation_changed {
        parts.push("배분 변경".into());
    }
    if parts.is_empty() {
        parts.push("거래 내용(금액·분류·처리일) 변경".into());
    }
    out.summary = format!("확정 뒤 변경: {}", parts.join(" · "));
    Ok(out)
}

/// 월 확정 — 그 시점의 합계와 확정 대상 자료의 지문을 남긴다. 확정된 달의 업로드 자료는 재개 전까지 바꿀 수 없다.
pub fn month_close(conn: &Connection, fiscal_month_id: i64, note: &str) -> AppResult<()> {
    if month_is_closed(conn, fiscal_month_id)? {
        return Err(AppError::invalid("이미 확정된 달입니다."));
    }
    let s = crate::repo::ledger::month_summary(conn, fiscal_month_id)?;
    let totals = serde_json::json!({
        "entryCount": s.rows.iter().map(|r| r.entry_count).sum::<i64>(),
        "collectionTotal": s.collection_total,
        "expenditureTotal": s.expenditure_total,
    });
    let (fp, snapshot) = month_fingerprint(conn, fiscal_month_id)?;
    conn.execute("UPDATE fiscal_month SET status = 'closed' WHERE id = ?1", [fiscal_month_id])?;
    conn.execute(
        "INSERT INTO month_close (fiscal_month_id, action, totals_json, note, fingerprint, snapshot_json) VALUES (?1, 'close', ?2, ?3, ?4, ?5)",
        params![fiscal_month_id, totals.to_string(), note.trim(), fp, snapshot.to_string()],
    )?;
    audit::write(conn, "month.close", "fiscal_month", Some(fiscal_month_id), &format!("{} 확정", month_label_of(conn, fiscal_month_id)?), serde_json::json!({}))?;
    Ok(())
}

pub fn month_reopen(conn: &Connection, fiscal_month_id: i64, note: &str) -> AppResult<()> {
    if !month_is_closed(conn, fiscal_month_id)? {
        return Err(AppError::invalid("확정되지 않은 달입니다."));
    }
    let n = note.trim();
    if n.is_empty() {
        return Err(AppError::invalid("재개 사유를 입력해 주세요."));
    }
    conn.execute("UPDATE fiscal_month SET status = 'open' WHERE id = ?1", [fiscal_month_id])?;
    conn.execute(
        "INSERT INTO month_close (fiscal_month_id, action, note) VALUES (?1, 'reopen', ?2)",
        params![fiscal_month_id, n],
    )?;
    audit::write(conn, "month.reopen", "fiscal_month", Some(fiscal_month_id), &format!("{} 재개: {n}", month_label_of(conn, fiscal_month_id)?), serde_json::json!({}))?;
    Ok(())
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
