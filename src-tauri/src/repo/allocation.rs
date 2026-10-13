//! 프로그램별 징수액 배분.
//!
//! 징수명단(기안용)처럼 프로그램 열이 없는 양식에서 온 징수 거래는 프로그램이 NULL(미배분)이다.
//! 프로그램별 수강생명단(`roster_link` 업로드)의 정규화 행으로 그 거래의 귀속 프로그램을 정한다.
//!
//! 자동 배분은 근거가 분명할 때만 한다.
//!   · 같은 학생(학년·반·번호·이름이 모두 같은 `student_id`)이 참고 명단에서 **한 프로그램**뿐이면
//!     거래 전액을 그 프로그램에 (auto_single).
//!   · 여러 프로그램이면, 그 학생·항목의 징수 거래가 **하나**이고 참고 명단의 프로그램별 금액 합이
//!     그 거래 금액과 **정확히 같을 때만** 참고 금액대로 나눈다 (auto_split).
//!   · 그 밖(동명이인·학생 식별 불가·금액 불일치·재원 여러 개)은 자동으로 하지 않고 사용자 확인 대상.
//! 배분은 원장 금액을 바꾸지 않는다. 합은 원 거래를 넘지 못한다(트리거). 남는 금액은 '미배분'으로 둔다.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{audit, year};

/// 거래 하나에 배분된 합 (살아 있는 것만).
pub fn allocated_sum(conn: &Connection, entry_id: i64) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(amount), 0) FROM allocation WHERE entry_id = ?1 AND status = 'active'",
        [entry_id],
        |r| r.get(0),
    )?)
}

fn month_of_entry(conn: &Connection, entry_id: i64) -> AppResult<(i64, i64, i64, Option<i64>, Option<i64>, String)> {
    conn.query_row(
        "SELECT e.fiscal_month_id, e.school_year_id, e.amount, e.program_id, e.student_id, e.state
           FROM ledger_entry e WHERE e.id = ?1",
        [entry_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("거래를 찾지 못했습니다."))
}

fn guard(conn: &Connection, entry_id: i64) -> AppResult<(i64, i64, i64)> {
    let (month, year_id, amount, program, _student, state) = month_of_entry(conn, entry_id)?;
    if state != "active" {
        return Err(AppError::invalid("취소되었거나 대체된 거래에는 배분할 수 없습니다."));
    }
    if program.is_some() {
        return Err(AppError::invalid("이미 프로그램이 있는 거래는 배분 대상이 아닙니다."));
    }
    if year::month_is_closed(conn, month)? {
        return Err(AppError::invalid("확정된 달의 배분은 바꿀 수 없습니다. 월별 정산에서 재개한 뒤 바꿔 주세요."));
    }
    Ok((month, year_id, amount))
}

fn insert(
    conn: &Connection,
    entry_id: i64,
    program_id: i64,
    amount: i64,
    method: &str,
    source_upload_id: Option<i64>,
    source_row_id: Option<i64>,
    confirmed: bool,
    reason: &str,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO allocation (entry_id, program_id, amount, method, source_upload_id, source_row_id, confirmed, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![entry_id, program_id, amount, method, source_upload_id, source_row_id, confirmed as i64, reason],
    )
    .map_err(|e| {
        let text = e.to_string();
        if text.contains("ALLOCATION_OVER") {
            AppError::invalid("배분 합이 원 거래 금액을 넘거나 부호가 다릅니다. 기존 자료는 그대로입니다.").detail(text)
        } else {
            AppError::from(e)
        }
    })?;
    Ok(conn.last_insert_rowid())
}

/// 수동 배분 — 사용자가 확인한 금액을 프로그램별로 넣는다. 전부 또는 일부.
pub fn manual(conn: &Connection, entry_id: i64, parts: &[(i64, i64)], reason: &str) -> AppResult<Vec<i64>> {
    let (_, _, amount) = guard(conn, entry_id)?;
    if parts.is_empty() {
        return Err(AppError::invalid("배분할 프로그램과 금액을 입력해 주세요."));
    }
    let already = allocated_sum(conn, entry_id)?;
    let total: i64 = parts.iter().map(|p| p.1).try_fold(0i64, |a, b| a.checked_add(b))
        .ok_or_else(|| AppError::new("OVERFLOW", "금액 합계가 표현 범위를 넘었습니다."))?;
    if parts.iter().any(|p| p.1 == 0) {
        return Err(AppError::invalid("0원은 배분할 수 없습니다."));
    }
    if (already + total).abs() > amount.abs() || parts.iter().any(|p| (p.1 > 0) != (amount > 0)) {
        return Err(AppError::invalid(format!(
            "배분 합({})이 원 거래 금액({})을 넘습니다. 기존 자료는 그대로입니다.",
            crate::domain::money::format_won(already + total),
            crate::domain::money::format_won(amount)
        )));
    }
    let mut ids = Vec::new();
    for (program_id, amt) in parts {
        ids.push(insert(conn, entry_id, *program_id, *amt, "manual", None, None, true, reason)?);
    }
    audit::write(conn, "allocation.manual", "ledger_entry", Some(entry_id), &format!("수동 배분 {}건", parts.len()), serde_json::json!({}))?;
    Ok(ids)
}

/// 배분 취소. 거래 자체는 그대로다.
pub fn void(conn: &Connection, allocation_id: i64, reason: &str) -> AppResult<()> {
    let entry_id: Option<i64> = conn
        .query_row("SELECT entry_id FROM allocation WHERE id = ?1 AND status = 'active'", [allocation_id], |r| r.get(0))
        .optional()?;
    let Some(entry_id) = entry_id else { return Err(AppError::not_found("살아 있는 배분을 찾지 못했습니다.")) };
    let (month, _, _, _, _, _) = month_of_entry(conn, entry_id)?;
    if year::month_is_closed(conn, month)? {
        return Err(AppError::invalid("확정된 달의 배분은 바꿀 수 없습니다."));
    }
    let r = reason.trim();
    if r.is_empty() {
        return Err(AppError::invalid("취소 사유를 입력해 주세요."));
    }
    conn.execute(
        "UPDATE allocation SET status = 'voided', voided_at = datetime('now', 'localtime'), void_reason = ?2 WHERE id = ?1",
        params![allocation_id, r],
    )?;
    audit::write(conn, "allocation.void", "allocation", Some(allocation_id), &format!("배분 취소: {r}"), serde_json::json!({}))?;
    Ok(())
}

/// 거래들이 취소될 때 그 배분도 함께 접는다 (파일 취소·교체·복원).
pub fn void_for_entries(conn: &Connection, entry_ids: &[i64], reason: &str) -> AppResult<usize> {
    let mut n = 0;
    for id in entry_ids {
        n += conn.execute(
            "UPDATE allocation SET status = 'voided', voided_at = datetime('now', 'localtime'), void_reason = ?2
              WHERE entry_id = ?1 AND status = 'active'",
            params![id, reason],
        )?;
    }
    Ok(n)
}

// ── 배분 대상 조회 ──────────────────────────────────────────────────────

/// 참고 명단(roster_link·snapshot 등 적용 중인 파일)의 정규화 행 — 학생 → (프로그램, 항목, 금액).
#[derive(Debug, Clone)]
struct RosterRow {
    upload_id: i64,
    row_id: i64,
    student_id: Option<i64>,
    student_name: String,
    program_id: i64,
    program_name: String,
    item_code: String,
    amount: i64,
}

fn roster_rows(conn: &Connection, fiscal_month_id: i64) -> AppResult<Vec<RosterRow>> {
    let mut st = conn.prepare(
        "SELECT u.upload_file_id, u.id, u.student_id, u.student_name, u.program_id, p.name, u.amount_item_code, u.amount
           FROM upload_row u
           JOIN upload_file f ON f.id = u.upload_file_id
           JOIN program p ON p.id = u.program_id
          WHERE f.fiscal_month_id = ?1 AND f.status = 'applied' AND f.data_kind = 'roster_link'
            AND u.program_id IS NOT NULL AND u.status <> 'error'
            AND (u.entry_kind = '' OR u.entry_kind = 'initial_collect')
          ORDER BY u.id",
    )?;
    let rows = st.query_map([fiscal_month_id], |r| {
        Ok(RosterRow {
            upload_id: r.get(0)?,
            row_id: r.get(1)?,
            student_id: r.get(2)?,
            student_name: r.get(3)?,
            program_id: r.get(4)?,
            program_name: r.get(5)?,
            item_code: r.get(6)?,
            amount: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 미배분 징수 거래(살아 있고 프로그램이 없는 initial_collect)와 배분 현황.
pub fn pending(conn: &Connection, fiscal_month_id: i64) -> AppResult<AllocationBoard> {
    let roster = roster_rows(conn, fiscal_month_id)?;
    let roster_uploads: i64 = conn.query_row(
        "SELECT count(*) FROM upload_file WHERE fiscal_month_id = ?1 AND status = 'applied' AND data_kind = 'roster_link'",
        [fiscal_month_id],
        |r| r.get(0),
    )?;
    let mut by_student: HashMap<i64, Vec<&RosterRow>> = HashMap::new();
    let mut by_name: HashMap<String, Vec<&RosterRow>> = HashMap::new();
    for r in &roster {
        if let Some(s) = r.student_id {
            by_student.entry(s).or_default().push(r);
        }
        if !r.student_name.is_empty() {
            by_name.entry(r.student_name.clone()).or_default().push(r);
        }
    }
    let mut st = conn.prepare(
        "SELECT e.id, e.amount, e.student_id, coalesce(s.grade, 0), coalesce(s.class_name, ''), coalesce(s.student_no, ''), coalesce(s.name, ''),
                coalesce(ef.name, '미확정'), i.code, i.name, e.entry_kind, f.file_name, f.version_no
           FROM ledger_entry e
           LEFT JOIN student s ON s.id = e.student_id
           LEFT JOIN fund_type f0 ON f0.id = e.fund_type_id
           LEFT JOIN fund_type ef ON ef.id = coalesce(f0.merged_into_id, f0.id)
           JOIN amount_item i ON i.id = e.amount_item_id
           LEFT JOIN upload_file f ON f.id = e.upload_file_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active' AND e.flow = 'collection'
            AND e.program_id IS NULL AND e.program_group_id IS NULL
          ORDER BY s.grade, s.class_name, s.student_no, s.name, i.sort_order, e.id",
    )?;
    let mut items: Vec<PendingEntry> = Vec::new();
    let mut unallocated_total = 0i64;
    let mut allocated_total = 0i64;
    let mut by_item: BTreeMap<String, i64> = BTreeMap::new();
    let mut auto_possible = 0i64;
    let mut need_user = 0i64;
    let mut no_student = 0i64;
    let rows = st.query_map([fiscal_month_id], |r| {
        Ok((
            r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, i64>(3)?, r.get::<_, String>(4)?,
            r.get::<_, String>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?, r.get::<_, String>(8)?, r.get::<_, String>(9)?,
            r.get::<_, String>(10)?, r.get::<_, Option<String>>(11)?, r.get::<_, Option<i64>>(12)?,
        ))
    })?;
    let rows: Vec<_> = rows.collect::<Result<_, _>>()?;
    for (id, amount, student_id, grade, class_name, student_no, name, fund_name, item_code, item_name, kind, file_name, version) in rows {
        let allocated = allocated_sum(conn, id)?;
        let remaining = amount - allocated;
        let allocs = list_for_entry(conn, id)?;
        let mut candidates: Vec<AllocCandidate> = Vec::new();
        let mut exact = false;
        if let Some(s) = student_id {
            if let Some(rs) = by_student.get(&s) {
                exact = true;
                for r in rs {
                    candidates.push(AllocCandidate {
                        program_id: r.program_id,
                        program_name: r.program_name.clone(),
                        item_code: r.item_code.clone(),
                        amount: r.amount,
                        source_upload_id: r.upload_id,
                        source_row_id: r.row_id,
                        same_student: true,
                    });
                }
            }
        }
        if candidates.is_empty() && !name.is_empty() {
            if let Some(rs) = by_name.get(&name) {
                for r in rs {
                    candidates.push(AllocCandidate {
                        program_id: r.program_id,
                        program_name: r.program_name.clone(),
                        item_code: r.item_code.clone(),
                        amount: r.amount,
                        source_upload_id: r.upload_id,
                        source_row_id: r.row_id,
                        same_student: false,
                    });
                }
            }
        }
        let plan = if remaining == 0 {
            "done"
        } else if student_id.is_none() {
            "no_student"
        } else if !exact {
            if candidates.is_empty() { "no_roster" } else { "name_only" }
        } else {
            match auto_plan(conn, fiscal_month_id, amount, remaining, &item_code, &candidates, student_id, &kind)? {
                Some(_) => "auto",
                None => "manual",
            }
        };
        match plan {
            "auto" => auto_possible += 1,
            "manual" | "name_only" | "no_roster" => need_user += 1,
            "no_student" => no_student += 1,
            _ => {}
        }
        if remaining != 0 {
            unallocated_total += remaining;
            *by_item.entry(item_name.clone()).or_default() += remaining;
        }
        allocated_total += allocated;
        items.push(PendingEntry {
            entry_id: id,
            entry_kind: kind,
            student: if name.is_empty() { String::new() } else { format!("{grade}학년 {class_name}반 {student_no}번 {name}") },
            student_id,
            fund_name,
            item_code,
            item_name,
            amount,
            allocated,
            remaining,
            plan: plan.into(),
            candidates,
            allocations: allocs,
            file_name: file_name.unwrap_or_default(),
            version_no: version.unwrap_or(0),
        });
    }
    Ok(AllocationBoard {
        fiscal_month_id,
        roster_upload_count: roster_uploads,
        entries: items,
        unallocated_total,
        allocated_total,
        auto_possible,
        need_user,
        no_student,
        by_item: by_item.into_iter().map(|(name, total)| SummaryLine { name, count: 0, total }).collect(),
    })
}

pub fn list_for_entry(conn: &Connection, entry_id: i64) -> AppResult<Vec<Allocation>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.entry_id, a.program_id, p.name, a.amount, a.method, a.status, a.confirmed, a.reason, a.created_at, a.source_row_id
           FROM allocation a JOIN program p ON p.id = a.program_id WHERE a.entry_id = ?1 ORDER BY a.id",
    )?;
    let rows = st.query_map([entry_id], |r| {
        Ok(Allocation {
            id: r.get(0)?,
            entry_id: r.get(1)?,
            program_id: r.get(2)?,
            program_name: r.get(3)?,
            amount: r.get(4)?,
            method: r.get(5)?,
            status: r.get(6)?,
            confirmed: r.get::<_, i64>(7)? == 1,
            reason: r.get(8)?,
            created_at: r.get(9)?,
            source_row_id: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 자동 배분 계획. 근거가 모자라면 None.
fn auto_plan(
    conn: &Connection,
    fiscal_month_id: i64,
    amount: i64,
    remaining: i64,
    item_code: &str,
    candidates: &[AllocCandidate],
    student_id: Option<i64>,
    kind: &str,
) -> AppResult<Option<Vec<(i64, i64, i64, i64)>>> {
    if kind != "initial_collect" || remaining != amount {
        return Ok(None); // 이미 일부 배분된 거래는 사람이 본다
    }
    let same: Vec<&AllocCandidate> = candidates.iter().filter(|c| c.same_student).collect();
    if same.is_empty() {
        return Ok(None);
    }
    let mut programs: BTreeMap<i64, (i64, i64, i64)> = BTreeMap::new(); // program → (금액 합, upload, row)
    for c in &same {
        if !c.item_code.is_empty() && c.item_code != item_code {
            continue;
        }
        let e = programs.entry(c.program_id).or_insert((0, c.source_upload_id, c.source_row_id));
        e.0 += c.amount;
    }
    if programs.is_empty() {
        // 금액 없는 명단(1학기 명단)이면 항목 구분 없이 프로그램만 본다.
        for c in &same {
            programs.entry(c.program_id).or_insert((0, c.source_upload_id, c.source_row_id));
        }
    }
    if programs.len() == 1 {
        let (pid, (_, up, row)) = programs.into_iter().next().unwrap();
        return Ok(Some(vec![(pid, amount, up, row)]));
    }
    // 여러 프로그램: 이 학생·항목의 징수 거래가 하나뿐이고 참고 금액 합이 정확히 같을 때만.
    let Some(sid) = student_id else { return Ok(None) };
    let n_entries: i64 = conn.query_row(
        "SELECT count(*) FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id
          WHERE e.student_id = ?1 AND e.state = 'active' AND e.flow = 'collection' AND e.entry_kind = 'initial_collect'
            AND e.program_id IS NULL AND i.code = ?2 AND e.fiscal_month_id = ?3",
        params![sid, item_code, fiscal_month_id],
        |r| r.get(0),
    )?;
    if n_entries != 1 {
        return Ok(None);
    }
    let sum: i64 = programs.values().map(|v| v.0).sum();
    if sum != amount || programs.values().any(|v| v.0 == 0) {
        return Ok(None);
    }
    Ok(Some(programs.into_iter().map(|(pid, (amt, up, row))| (pid, amt, up, row)).collect()))
}

/// 자동 배분 실행 — 근거가 분명한 거래만. 결과: (배분한 거래 수, 배분 줄 수, 남은 거래 수).
pub fn auto_allocate(conn: &Connection, fiscal_month_id: i64) -> AppResult<AutoAllocateResult> {
    if year::month_is_closed(conn, fiscal_month_id)? {
        return Err(AppError::invalid("확정된 달의 배분은 바꿀 수 없습니다."));
    }
    let board = pending(conn, fiscal_month_id)?;
    let mut entries_done = 0;
    let mut lines = 0;
    let mut skipped = 0;
    for e in &board.entries {
        if e.plan != "auto" {
            if e.remaining != 0 {
                skipped += 1;
            }
            continue;
        }
        let Some(plan) = auto_plan(conn, fiscal_month_id, e.amount, e.remaining, &e.item_code, &e.candidates, e.student_id, &e.entry_kind)? else {
            skipped += 1;
            continue;
        };
        let method = if plan.len() == 1 { "auto_single" } else { "auto_split" };
        let roster_total: i64 = e.candidates.iter().filter(|c| c.same_student && (c.item_code.is_empty() || c.item_code == e.item_code)).map(|c| c.amount).sum();
        let reason = if plan.len() == 1 && roster_total != 0 && roster_total != e.amount {
            format!("학생의 유일한 프로그램. 참고 명단 금액({})과 징수 금액({})이 다름", crate::domain::money::format_won(roster_total), crate::domain::money::format_won(e.amount))
        } else if plan.len() == 1 {
            "학생의 유일한 프로그램".into()
        } else {
            "참고 명단의 프로그램별 금액 합이 징수 금액과 일치".into()
        };
        for (pid, amt, up, row) in plan {
            insert(conn, e.entry_id, pid, amt, method, Some(up), Some(row), false, &reason)?;
            lines += 1;
        }
        entries_done += 1;
    }
    if entries_done > 0 {
        audit::write(conn, "allocation.auto", "fiscal_month", Some(fiscal_month_id), &format!("자동 배분 {entries_done}건 (줄 {lines}), 보류 {skipped}건"), serde_json::json!({}))?;
    }
    Ok(AutoAllocateResult { entries_allocated: entries_done, lines, skipped })
}

#[cfg(test)]
#[path = "allocation_tests.rs"]
mod tests;
