//! 자유수강권 관리 — 정책 · 대상자 · 전입생 · 사용액 · 소급 지원 계산 · 일괄 재원경정 · 환불.
//!
//! 원칙
//!   · 사용액·잔액은 저장하지 않고 원장(`ledger_entry`, 실제 처리·살아 있는 거래)에서 그때그때 계산한다.
//!   · 원장에 쓰는 모든 것은 P3 `repo::adjustment` 를 거친다 (재원경정 = fund_transfer, 환불 = refund_return).
//!     이 모듈은 어떤 거래도 직접 만들거나 고치지 않는다.
//!   · 지원액을 자동으로 확정하지 않는다. 미리보기 → 담당자 선택 → 승인 묶음 등록. 묶음 안에서 하나라도 막히면 전부 되돌린다.
//!   · 환불 완료는 '실제 수납 확인' 없이는 등록되지 않는다. 환불 자체가 자동으로 완료되지 않는다.
//!   · 한도 초과분은 지원하지 않는다. 계산보다 실제 사용액이 많으면 '확인 대상' 으로만 보여 준다 (자동 되돌림 없음).
//!   · 전입생의 이전 학교 사용액은 담당자가 확인하기 전까지 '확인 필요' 이고, 0 원으로 자동 확정하지 않는다.

use std::collections::BTreeMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::money::format_won as w;
use crate::domain::months;
use crate::domain::voucher::{self as engine, default_semesters, Charge, Decision, LimitSpec};
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{adjustment, audit, catalog, recon, year};

pub fn basis_label(b: &str) -> &'static str {
    match b {
        "annual" => "연간 한도",
        "semester" => "학기별 한도",
        "unlimited" => "제한 없음",
        _ => "알 수 없음",
    }
}

pub fn target_status_label(s: &str) -> &'static str {
    match s {
        "pending" => "확인 전",
        "confirmed" => "대상자 확정",
        "ended" => "지원 종료",
        "cancelled" => "대상 취소",
        _ => "알 수 없음",
    }
}

pub fn prior_status_label(s: &str) -> &'static str {
    match s {
        "confirmed" => "확인됨",
        _ => "이전 학교 사용액 확인 필요",
    }
}

pub fn refund_status_label(s: &str) -> &'static str {
    match s {
        "none" => "미처리",
        "planned" => "처리 예정",
        "partial" => "일부 완료",
        "complete" => "처리 완료",
        "cancelled" => "취소",
        _ => "알 수 없음",
    }
}

pub fn batch_kind_label(k: &str) -> &'static str {
    match k {
        "retro" => "소급 지원 경정",
        "monthly" => "당월 사용액 경정",
        _ => "알 수 없음",
    }
}

fn usage_state_label(s: &str) -> &'static str {
    match s {
        "ok" => "일치",
        "retro" => "경정 필요",
        "over" => "확인 대상 (계산보다 많이 사용)",
        "decide" => "배분 결정 필요",
        "pending" => "처리 예정 경정 있음",
        "outside" => "적용 기간 밖",
        "excluded" => "지원 대상 항목 아님",
        _ => "",
    }
}

fn check_date(label: &str, d: &Option<String>) -> AppResult<()> {
    if let Some(d) = d {
        if !d.trim().is_empty() && chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").is_err() {
            return Err(AppError::invalid(format!("{label}은(는) YYYY-MM-DD 형식이어야 합니다.")));
        }
    }
    Ok(())
}

fn blank(d: &Option<String>) -> Option<String> {
    d.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn student_label(conn: &Connection, id: i64) -> AppResult<String> {
    Ok(conn
        .query_row("SELECT grade || '학년 ' || class_name || '반 ' || student_no || '번 ' || name FROM student WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .unwrap_or_default())
}

fn ensure_student(conn: &Connection, school_year_id: i64, student_id: i64) -> AppResult<()> {
    let n: i64 = conn.query_row("SELECT count(*) FROM student WHERE id = ?1 AND school_year_id = ?2", params![student_id, school_year_id], |r| r.get(0))?;
    if n == 0 {
        return Err(AppError::invalid("학생이 이 학년도에 없습니다."));
    }
    Ok(())
}

fn validate_limits(basis: &str, annual: Option<i64>, s1: Option<i64>, s2: Option<i64>) -> AppResult<()> {
    if !engine::is_valid_basis(basis) {
        return Err(AppError::invalid("한도 기준은 연간 · 학기별 · 제한 없음 중 하나여야 합니다."));
    }
    for (label, v) in [("연간 한도", annual), ("1학기 한도", s1), ("2학기 한도", s2)] {
        if let Some(v) = v {
            if v < 0 {
                return Err(AppError::invalid(format!("{label}은(는) 0원 이상이어야 합니다.")));
            }
        }
    }
    match basis {
        "annual" if annual.is_none() => Err(AppError::invalid("연간 한도 금액을 입력해 주세요.")),
        "semester" if s1.is_none() || s2.is_none() => Err(AppError::invalid("1학기·2학기 한도 금액을 모두 입력해 주세요.")),
        _ => Ok(()),
    }
}

fn limit_label(basis: &str, annual: Option<i64>, s1: Option<i64>, s2: Option<i64>, carry: bool) -> String {
    match basis {
        "annual" => format!("연간 {}원", w(annual.unwrap_or(0))),
        "semester" => format!("1학기 {}원 · 2학기 {}원{}", w(s1.unwrap_or(0)), w(s2.unwrap_or(0)), if carry { " · 잔액 이월" } else { " · 이월 없음" }),
        _ => "제한 없음".into(),
    }
}

// ── 정책 ─────────────────────────────────────────────────────────────────

const POLICY_COLS: &str = "id, school_year_id, basis, annual_limit, semester1_limit, semester2_limit, carry_over, note, created_at, superseded_at, semester1_from, semester1_to, semester2_from, semester2_to";

fn map_policy(r: &rusqlite::Row) -> rusqlite::Result<VoucherPolicy> {
    let basis: String = r.get(2)?;
    let d: [Option<String>; 4] = [r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?];
    Ok(VoucherPolicy {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        basis_label: basis_label(&basis).into(),
        basis,
        annual_limit: r.get(3)?,
        semester1_limit: r.get(4)?,
        semester2_limit: r.get(5)?,
        carry_over: r.get::<_, i64>(6)? == 1,
        semester_default: d.iter().all(|x| x.is_none()),
        semester1_from: d[0].clone().unwrap_or_default(),
        semester1_to: d[1].clone().unwrap_or_default(),
        semester2_from: d[2].clone().unwrap_or_default(),
        semester2_to: d[3].clone().unwrap_or_default(),
        uncovered_months: vec![],
        eligible_item_ids: vec![],
        eligible_item_names: vec![],
        all_items_eligible: true,
        note: r.get(7)?,
        created_at: r.get(8)?,
        superseded_at: r.get(9)?,
    })
}

/// 기본 학기 기간: 1학기 3/1~8/31 · 2학기 9/1~다음 해 2월 말일.
pub fn default_semester_dates(year: i32) -> [String; 4] {
    let feb_end = chrono::NaiveDate::from_ymd_opt(year + 1, 3, 1)
        .and_then(|d| d.pred_opt())
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("{}-02-28", year + 1));
    [format!("{year}-03-01"), format!("{year}-08-31"), format!("{year}-09-01"), feb_end]
}

/// 회계월(차례순)마다 학기 번호. 날짜는 달 단위로 비교한다. 어느 학기에도 들지 않으면 0.
fn semesters_for(months: &[MonthRef], d: &[String; 4]) -> Vec<i32> {
    let ym = |s: &str| s.get(0..7).unwrap_or("").to_string();
    let (a, b, c, e) = (ym(&d[0]), ym(&d[1]), ym(&d[2]), ym(&d[3]));
    months
        .iter()
        .map(|m| {
            if !a.is_empty() && m.ym >= a && m.ym <= b {
                1
            } else if !c.is_empty() && m.ym >= c && m.ym <= e {
                2
            } else {
                0
            }
        })
        .collect()
}

fn month_labels(months: &[MonthRef], pick: impl Fn(usize) -> bool) -> Vec<String> {
    months.iter().enumerate().filter(|(i, _)| pick(*i)).map(|(_, m)| format!("{} ({})", months::label(&m.ym), m.ym)).collect()
}

/// 정책 행에 기본값·학기 공백·지원 대상 항목을 채운다.
fn fill_policy(conn: &Connection, p: &mut VoucherPolicy) -> AppResult<()> {
    let y = year::get(conn, p.school_year_id)?;
    let def = default_semester_dates(y.year);
    for (i, slot) in [&mut p.semester1_from, &mut p.semester1_to, &mut p.semester2_from, &mut p.semester2_to].into_iter().enumerate() {
        if slot.is_empty() {
            *slot = def[i].clone();
        }
    }
    let months = months_of_year(conn, p.school_year_id)?;
    let sems = semesters_for(&months, &[p.semester1_from.clone(), p.semester1_to.clone(), p.semester2_from.clone(), p.semester2_to.clone()]);
    p.uncovered_months = month_labels(&months, |i| sems[i] == 0);
    let mut st = conn.prepare("SELECT i.id, i.name FROM voucher_policy_item x JOIN amount_item i ON i.id = x.amount_item_id WHERE x.policy_id = ?1 AND x.eligible = 1 ORDER BY i.sort_order")?;
    let rows: Vec<(i64, String)> = st.query_map([p.id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let any: i64 = conn.query_row("SELECT count(*) FROM voucher_policy_item WHERE policy_id = ?1", [p.id], |r| r.get(0))?;
    if any == 0 {
        p.all_items_eligible = true;
        p.eligible_item_ids = vec![];
        p.eligible_item_names = catalog::amount_items(conn)?.iter().filter(|i| i.is_active).map(|i| i.name.clone()).collect();
    } else {
        p.all_items_eligible = false;
        p.eligible_item_ids = rows.iter().map(|x| x.0).collect();
        p.eligible_item_names = rows.into_iter().map(|x| x.1).collect();
    }
    Ok(())
}

pub fn policy_get(conn: &Connection, id: i64) -> AppResult<VoucherPolicy> {
    let mut p = conn.query_row(&format!("SELECT {POLICY_COLS} FROM voucher_policy WHERE id = ?1"), [id], map_policy)?;
    fill_policy(conn, &mut p)?;
    Ok(p)
}

pub fn policy_current(conn: &Connection, school_year_id: i64) -> AppResult<Option<VoucherPolicy>> {
    let p = conn
        .query_row(&format!("SELECT {POLICY_COLS} FROM voucher_policy WHERE school_year_id = ?1 AND superseded_at IS NULL"), [school_year_id], map_policy)
        .optional()?;
    match p {
        Some(mut p) => {
            fill_policy(conn, &mut p)?;
            Ok(Some(p))
        }
        None => Ok(None),
    }
}

pub fn policy_history(conn: &Connection, school_year_id: i64) -> AppResult<Vec<VoucherPolicy>> {
    let mut st = conn.prepare(&format!("SELECT {POLICY_COLS} FROM voucher_policy WHERE school_year_id = ?1 ORDER BY id DESC"))?;
    let mut rows: Vec<VoucherPolicy> = st.query_map([school_year_id], map_policy)?.collect::<Result<_, _>>()?;
    for p in rows.iter_mut() {
        fill_policy(conn, p)?;
    }
    Ok(rows)
}

fn resolve_dates(conn: &Connection, input: &VoucherPolicyInput) -> AppResult<([String; 4], [Option<String>; 4])> {
    let y = year::get(conn, input.school_year_id)?;
    let def = default_semester_dates(y.year);
    let given = [blank(&input.semester1_from), blank(&input.semester1_to), blank(&input.semester2_from), blank(&input.semester2_to)];
    let mut eff = def.clone();
    for i in 0..4 {
        if let Some(v) = &given[i] {
            eff[i] = v.clone();
        }
    }
    Ok((eff, given))
}

fn validate_dates(conn: &Connection, school_year_id: i64, d: &[String; 4]) -> Vec<String> {
    let mut out = Vec::new();
    for (label, v) in [("1학기 시작일", &d[0]), ("1학기 종료일", &d[1]), ("2학기 시작일", &d[2]), ("2학기 종료일", &d[3])] {
        if chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").is_err() {
            out.push(format!("{label}은(는) YYYY-MM-DD 형식이어야 합니다."));
        }
    }
    if !out.is_empty() {
        return out;
    }
    if d[0] > d[1] {
        out.push("1학기 시작일이 종료일보다 늦습니다.".into());
    }
    if d[2] > d[3] {
        out.push("2학기 시작일이 종료일보다 늦습니다.".into());
    }
    if !(d[1] < d[2] || d[3] < d[0]) {
        out.push("1학기와 2학기 기간이 겹칩니다.".into());
    }
    if let Ok(y) = year::get(conn, school_year_id) {
        for (label, v) in [("1학기 시작일", &d[0]), ("1학기 종료일", &d[1]), ("2학기 시작일", &d[2]), ("2학기 종료일", &d[3])] {
            let ym = &v[0..7];
            if ym < y.start_ym.as_str() || ym > y.end_ym.as_str() {
                out.push(format!("{label}이(가) 학년도 범위({} ~ {}) 밖입니다.", y.start_ym, y.end_ym));
            }
        }
    }
    out
}

/// 저장 전 미리보기 — 차단 사유, 학기 공백, 지원 대상에서 빠지는 항목에 이미 등록된 경정.
pub fn policy_preview(conn: &Connection, input: &VoucherPolicyInput) -> AppResult<VoucherPolicyPreview> {
    let mut blocked = Vec::new();
    let mut warnings = Vec::new();
    if let Err(e) = validate_limits(&input.basis, input.annual_limit, input.semester1_limit, input.semester2_limit) {
        blocked.push(e.message);
    }
    year::get(conn, input.school_year_id)?;
    let (eff, _) = resolve_dates(conn, input)?;
    blocked.extend(validate_dates(conn, input.school_year_id, &eff));
    let months = months_of_year(conn, input.school_year_id)?;
    let uncovered = if blocked.is_empty() { month_labels(&months, |i| semesters_for(&months, &eff)[i] == 0) } else { vec![] };
    if input.basis == "semester" && !uncovered.is_empty() {
        warnings.push(format!("{} 은(는) 어느 학기에도 속하지 않아 미지원 기간으로 봅니다 (어느 학기에도 임의로 넣지 않습니다).", uncovered.join(", ")));
    }
    let items = catalog::amount_items(conn)?;
    let mut eligible_item_names: Vec<String> = Vec::new();
    let mut affected_count = 0;
    let mut affected_total = 0;
    match &input.eligible_item_ids {
        None => eligible_item_names = items.iter().filter(|i| i.is_active).map(|i| i.name.clone()).collect(),
        Some(ids) => {
            if ids.is_empty() {
                blocked.push("지원 대상 항목을 하나 이상 골라 주세요.".into());
            }
            for id in ids {
                match items.iter().find(|i| i.id == *id) {
                    Some(i) => eligible_item_names.push(i.name.clone()),
                    None => blocked.push(format!("금액 항목 #{id} 이(가) 없습니다.")),
                }
            }
            let idx = recon::FundIndex::load(conn)?;
            let free = free_fund_id(conn, &idx)?;
            let mut st = conn.prepare(
                "SELECT amount_item_id, to_fund_id, amount FROM adjustment WHERE school_year_id = ?1 AND kind = 'fund_transfer' AND status <> 'cancelled' AND student_id IS NOT NULL",
            )?;
            let rows: Vec<(i64, i64, i64)> = st.query_map([input.school_year_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
            for (item, to, amount) in rows {
                if idx.effective(to) == free && !ids.contains(&item) {
                    affected_count += 1;
                    affected_total += amount;
                }
            }
            if affected_count > 0 {
                warnings.push(format!(
                    "지원 대상에서 빠지는 항목에 이미 등록된 자유수강권 경정 {}건 {}원이 있습니다. 완료된 경정은 자동으로 취소·수정하지 않습니다 — 필요하면 회계 정정에서 담당자가 처리합니다.",
                    affected_count, w(affected_total)
                ));
            }
        }
    }
    if policy_current(conn, input.school_year_id)?.is_some() {
        warnings.push("정책을 바꾸면 이후 사용액 계산은 새 정책으로 다시 계산됩니다. 이미 등록된 경정·환불은 바뀌지 않고, 이전 정책은 이력으로 남습니다.".into());
    }
    Ok(VoucherPolicyPreview {
        semester1_from: eff[0].clone(),
        semester1_to: eff[1].clone(),
        semester2_from: eff[2].clone(),
        semester2_to: eff[3].clone(),
        uncovered_months: uncovered,
        eligible_item_names,
        affected_transfer_count: affected_count,
        affected_transfer_total: affected_total,
        warnings,
        blocked,
    })
}

/// 정책 저장 — 이전 정책은 닫고(superseded_at) 새 행을 넣는다. 학기 기간·지원 대상 항목은 정책 판과 함께 남는다.
pub fn policy_save(tx: &Connection, input: &VoucherPolicyInput) -> AppResult<VoucherPolicy> {
    let pv = policy_preview(tx, input)?;
    if !pv.blocked.is_empty() {
        return Err(AppError::invalid(pv.blocked.join(" / ")));
    }
    let (_, given) = resolve_dates(tx, input)?;
    let (annual, s1, s2, carry) = match input.basis.as_str() {
        "annual" => (input.annual_limit, None, None, false),
        "semester" => (None, input.semester1_limit, input.semester2_limit, input.carry_over),
        _ => (None, None, None, false),
    };
    let prev = policy_current(tx, input.school_year_id)?;
    tx.execute(
        "UPDATE voucher_policy SET superseded_at = datetime('now', 'localtime') WHERE school_year_id = ?1 AND superseded_at IS NULL",
        [input.school_year_id],
    )?;
    tx.execute(
        "INSERT INTO voucher_policy (school_year_id, basis, annual_limit, semester1_limit, semester2_limit, carry_over, note, semester1_from, semester1_to, semester2_from, semester2_to)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![input.school_year_id, input.basis, annual, s1, s2, carry as i64, input.note.trim(), given[0], given[1], given[2], given[3]],
    )?;
    let id = tx.last_insert_rowid();
    if let Some(ids) = &input.eligible_item_ids {
        for item in catalog::amount_items(tx)? {
            tx.execute("INSERT INTO voucher_policy_item (policy_id, amount_item_id, eligible) VALUES (?1, ?2, ?3)", params![id, item.id, ids.contains(&item.id) as i64])?;
        }
    }
    audit::write(
        tx,
        "voucher.policy",
        "voucher_policy",
        Some(id),
        &format!("자유수강권 한도 정책 {}: {} · 지원 항목 {}", if prev.is_some() { "변경" } else { "등록" }, limit_label(&input.basis, annual, s1, s2, carry), pv.eligible_item_names.join("·")),
        serde_json::json!({ "previous": prev.map(|p| p.id), "semesters": [pv.semester1_from, pv.semester1_to, pv.semester2_from, pv.semester2_to] }),
    )?;
    policy_get(tx, id)
}

// ── 학생 예외 ─────────────────────────────────────────────────────────────

const EXC_COLS: &str = "x.id, x.school_year_id, x.student_id, s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, x.basis, x.annual_limit, x.semester1_limit, x.semester2_limit, x.carry_over, x.reason, x.created_at, x.superseded_at";

fn map_exception(r: &rusqlite::Row) -> rusqlite::Result<VoucherException> {
    let basis: String = r.get(4)?;
    Ok(VoucherException {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        student_id: r.get(2)?,
        student: r.get(3)?,
        basis_label: basis_label(&basis).into(),
        basis,
        annual_limit: r.get(5)?,
        semester1_limit: r.get(6)?,
        semester2_limit: r.get(7)?,
        carry_over: r.get::<_, i64>(8)? == 1,
        reason: r.get(9)?,
        created_at: r.get(10)?,
        superseded_at: r.get(11)?,
    })
}

pub fn exception_list(conn: &Connection, school_year_id: i64, include_history: bool) -> AppResult<Vec<VoucherException>> {
    let mut st = conn.prepare(&format!(
        "SELECT {EXC_COLS} FROM voucher_exception x JOIN student s ON s.id = x.student_id
          WHERE x.school_year_id = ?1 AND (?2 = 1 OR x.superseded_at IS NULL) ORDER BY x.superseded_at IS NOT NULL, s.grade, s.class_name, s.student_no, x.id DESC"
    ))?;
    let rows: Vec<VoucherException> = st.query_map(params![school_year_id, include_history as i64], map_exception)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

fn exception_current(conn: &Connection, student_id: i64) -> AppResult<Option<VoucherException>> {
    Ok(conn
        .query_row(&format!("SELECT {EXC_COLS} FROM voucher_exception x JOIN student s ON s.id = x.student_id WHERE x.student_id = ?1 AND x.superseded_at IS NULL"), [student_id], map_exception)
        .optional()?)
}

pub fn exception_save(tx: &Connection, input: &VoucherExceptionInput) -> AppResult<VoucherException> {
    validate_limits(&input.basis, input.annual_limit, input.semester1_limit, input.semester2_limit)?;
    if input.reason.trim().is_empty() {
        return Err(AppError::invalid("예외 사유를 입력해 주세요."));
    }
    ensure_student(tx, input.school_year_id, input.student_id)?;
    let (annual, s1, s2, carry) = match input.basis.as_str() {
        "annual" => (input.annual_limit, None, None, false),
        "semester" => (None, input.semester1_limit, input.semester2_limit, input.carry_over),
        _ => (None, None, None, false),
    };
    tx.execute("UPDATE voucher_exception SET superseded_at = datetime('now', 'localtime') WHERE student_id = ?1 AND superseded_at IS NULL", [input.student_id])?;
    tx.execute(
        "INSERT INTO voucher_exception (school_year_id, student_id, basis, annual_limit, semester1_limit, semester2_limit, carry_over, reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![input.school_year_id, input.student_id, input.basis, annual, s1, s2, carry as i64, input.reason.trim()],
    )?;
    let id = tx.last_insert_rowid();
    audit::write(tx, "voucher.exception", "voucher_exception", Some(id), &format!("자유수강권 학생 예외 한도 등록: {}", limit_label(&input.basis, annual, s1, s2, carry)), serde_json::json!({}))?;
    Ok(tx.query_row(&format!("SELECT {EXC_COLS} FROM voucher_exception x JOIN student s ON s.id = x.student_id WHERE x.id = ?1"), [id], map_exception)?)
}

/// 예외 해제 — 행을 닫기만 한다. 그 뒤로는 학년도 정책이 적용된다.
pub fn exception_end(tx: &Connection, id: i64, reason: &str) -> AppResult<()> {
    if reason.trim().is_empty() {
        return Err(AppError::invalid("해제 사유를 입력해 주세요."));
    }
    let n = tx.execute("UPDATE voucher_exception SET superseded_at = datetime('now', 'localtime') WHERE id = ?1 AND superseded_at IS NULL", [id])?;
    if n == 0 {
        return Err(AppError::not_found("살아 있는 예외를 찾지 못했습니다."));
    }
    audit::write(tx, "voucher.exception_end", "voucher_exception", Some(id), &format!("자유수강권 학생 예외 해제: {}", reason.trim()), serde_json::json!({}))?;
    Ok(())
}

// ── 대상자 ─────────────────────────────────────────────────────────────────

const TARGET_COLS: &str = "t.id, t.school_year_id, t.student_id, s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, t.status, t.confirmed_on, t.apply_from, t.apply_to,
    t.transfer_in, t.transfer_in_on, t.prior_used, t.prior_status, t.prior_note, t.note, t.created_at, t.updated_at,
    EXISTS (SELECT 1 FROM voucher_exception x WHERE x.student_id = t.student_id AND x.superseded_at IS NULL)";

fn map_target(r: &rusqlite::Row) -> rusqlite::Result<VoucherTarget> {
    let status: String = r.get(4)?;
    let transfer_in = r.get::<_, i64>(8)? == 1;
    let prior_used: Option<i64> = r.get(10)?;
    let prior_status: String = r.get(11)?;
    Ok(VoucherTarget {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        student_id: r.get(2)?,
        student: r.get(3)?,
        status_label: target_status_label(&status).into(),
        status,
        confirmed_on: r.get(5)?,
        apply_from: r.get(6)?,
        apply_to: r.get(7)?,
        transfer_in,
        transfer_in_on: r.get(9)?,
        prior_used,
        prior_needs_confirm: transfer_in && (prior_status != "confirmed" || prior_used.is_none()),
        prior_status_label: prior_status_label(&prior_status).into(),
        prior_status,
        prior_note: r.get(12)?,
        note: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        has_exception: r.get::<_, i64>(16)? == 1,
        logs: Vec::new(),
    })
}

fn fill_logs(conn: &Connection, t: &mut VoucherTarget) -> AppResult<()> {
    let mut st = conn.prepare("SELECT action, from_status, to_status, detail, at FROM voucher_target_log WHERE target_id = ?1 ORDER BY id")?;
    t.logs = st
        .query_map([t.id], |r| Ok(VoucherTargetLog { action: r.get(0)?, from_status: r.get(1)?, to_status: r.get(2)?, detail: r.get(3)?, at: r.get(4)? }))?
        .collect::<Result<_, _>>()?;
    Ok(())
}

pub fn target_list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<VoucherTarget>> {
    let mut st = conn.prepare(&format!("SELECT {TARGET_COLS} FROM voucher_target t JOIN student s ON s.id = t.student_id WHERE t.school_year_id = ?1 ORDER BY s.grade, s.class_name, s.student_no, s.name"))?;
    let mut rows: Vec<VoucherTarget> = st.query_map([school_year_id], map_target)?.collect::<Result<_, _>>()?;
    for t in rows.iter_mut() {
        fill_logs(conn, t)?;
    }
    Ok(rows)
}

pub fn target_get(conn: &Connection, id: i64) -> AppResult<VoucherTarget> {
    let mut t = conn
        .query_row(&format!("SELECT {TARGET_COLS} FROM voucher_target t JOIN student s ON s.id = t.student_id WHERE t.id = ?1"), [id], map_target)
        .optional()?
        .ok_or_else(|| AppError::not_found("대상자를 찾지 못했습니다."))?;
    fill_logs(conn, &mut t)?;
    Ok(t)
}

fn target_of_student(conn: &Connection, school_year_id: i64, student_id: i64) -> AppResult<Option<VoucherTarget>> {
    let t = conn
        .query_row(&format!("SELECT {TARGET_COLS} FROM voucher_target t JOIN student s ON s.id = t.student_id WHERE t.school_year_id = ?1 AND t.student_id = ?2"), params![school_year_id, student_id], map_target)
        .optional()?;
    Ok(t)
}

fn ym_in_year(conn: &Connection, school_year_id: i64, date: &str) -> AppResult<bool> {
    let y = year::get(conn, school_year_id)?;
    let ym = &date[0..7];
    Ok(ym >= y.start_ym.as_str() && ym <= y.end_ym.as_str())
}

/// 대상자 등록·수정 (학생당 한 행). 상태·날짜 변경은 모두 이력에 남는다. 지원액은 여기서 확정되지 않는다.
pub fn target_save(tx: &Connection, input: &VoucherTargetInput) -> AppResult<VoucherTarget> {
    if !matches!(input.status.as_str(), "pending" | "confirmed" | "ended" | "cancelled") {
        return Err(AppError::invalid("대상자 상태가 올바르지 않습니다."));
    }
    ensure_student(tx, input.school_year_id, input.student_id)?;
    check_date("확정일", &input.confirmed_on)?;
    check_date("적용 시작일", &input.apply_from)?;
    check_date("적용 종료일", &input.apply_to)?;
    check_date("전입일", &input.transfer_in_on)?;
    let confirmed_on = blank(&input.confirmed_on);
    let apply_from = blank(&input.apply_from);
    let apply_to = blank(&input.apply_to);
    let transfer_in_on = if input.transfer_in { blank(&input.transfer_in_on) } else { None };
    if input.status == "confirmed" && (confirmed_on.is_none() || apply_from.is_none()) {
        return Err(AppError::invalid("대상자 확정에는 확정일과 적용 시작일이 필요합니다."));
    }
    if input.status == "ended" && (apply_from.is_none() || apply_to.is_none()) {
        return Err(AppError::invalid("지원 종료에는 적용 시작일과 적용 종료일이 필요합니다."));
    }
    if let (Some(f), Some(t)) = (&apply_from, &apply_to) {
        if f > t {
            return Err(AppError::invalid("적용 종료일이 적용 시작일보다 앞섭니다."));
        }
    }
    for (label, d) in [("적용 시작일", &apply_from), ("적용 종료일", &apply_to), ("전입일", &transfer_in_on)] {
        if let Some(d) = d {
            if !ym_in_year(tx, input.school_year_id, d)? {
                return Err(AppError::invalid(format!("{label}이(가) 이 학년도 범위 밖입니다.")));
            }
        }
    }
    if input.transfer_in && transfer_in_on.is_none() {
        return Err(AppError::invalid("전입생은 전입일이 필요합니다."));
    }
    let existing = target_of_student(tx, input.school_year_id, input.student_id)?;
    let id = match existing {
        None => {
            tx.execute(
                "INSERT INTO voucher_target (school_year_id, student_id, status, confirmed_on, apply_from, apply_to, transfer_in, transfer_in_on, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![input.school_year_id, input.student_id, input.status, confirmed_on, apply_from, apply_to, input.transfer_in as i64, transfer_in_on, input.note.trim()],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO voucher_target_log (target_id, action, from_status, to_status, detail) VALUES (?1, 'register', '', ?2, ?3)",
                params![id, input.status, format!("등록{}{}", apply_from.as_ref().map(|d| format!(" · 적용 시작 {d}")).unwrap_or_default(), if input.transfer_in { " · 전입생" } else { "" })],
            )?;
            id
        }
        Some(old) => {
            if old.status == input.status && old.confirmed_on == confirmed_on && old.apply_from == apply_from && old.apply_to == apply_to && old.transfer_in == input.transfer_in && old.transfer_in_on == transfer_in_on && old.note == input.note.trim() {
                return Err(AppError::invalid("바뀐 내용이 없습니다."));
            }
            if old.status != input.status && input.reason.trim().is_empty() {
                return Err(AppError::invalid("상태를 바꾸려면 사유를 입력해 주세요."));
            }
            tx.execute(
                "UPDATE voucher_target SET status = ?2, confirmed_on = ?3, apply_from = ?4, apply_to = ?5, transfer_in = ?6, transfer_in_on = ?7, note = ?8,
                        prior_used = CASE WHEN ?6 = 1 THEN prior_used ELSE NULL END,
                        prior_status = CASE WHEN ?6 = 1 THEN prior_status ELSE 'unconfirmed' END,
                        updated_at = datetime('now', 'localtime') WHERE id = ?1",
                params![old.id, input.status, confirmed_on, apply_from, apply_to, input.transfer_in as i64, transfer_in_on, input.note.trim()],
            )?;
            let mut changes: Vec<String> = Vec::new();
            if old.apply_from != apply_from || old.apply_to != apply_to || old.confirmed_on != confirmed_on {
                changes.push(format!(
                    "확정일 {} · 적용 {} ~ {}",
                    confirmed_on.clone().unwrap_or_else(|| "—".into()),
                    apply_from.clone().unwrap_or_else(|| "—".into()),
                    apply_to.clone().unwrap_or_else(|| "계속".into())
                ));
            }
            if old.transfer_in != input.transfer_in || old.transfer_in_on != transfer_in_on {
                changes.push(if input.transfer_in { format!("전입생 (전입일 {})", transfer_in_on.clone().unwrap_or_default()) } else { "전입생 해제".into() });
            }
            if !input.reason.trim().is_empty() {
                changes.push(format!("사유: {}", input.reason.trim()));
            }
            let action = if old.status != input.status { "status" } else { "dates" };
            tx.execute(
                "INSERT INTO voucher_target_log (target_id, action, from_status, to_status, detail) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![old.id, action, old.status, input.status, changes.join(" / ")],
            )?;
            old.id
        }
    };
    audit::write(tx, "voucher.target", "voucher_target", Some(id), &format!("자유수강권 대상자 {} ({})", target_status_label(&input.status), if input.transfer_in { "전입생" } else { "재학생" }), serde_json::json!({}))?;
    target_get(tx, id)
}

/// 전입생의 이전 학교 사용액 입력·확인. 0 원도 담당자가 명시해야 확인된다.
pub fn target_prior_set(tx: &Connection, input: &VoucherPriorInput) -> AppResult<VoucherTarget> {
    let t = target_get(tx, input.target_id)?;
    if !t.transfer_in {
        return Err(AppError::invalid("전입생이 아닌 대상자에는 이전 학교 사용액을 입력하지 않습니다."));
    }
    if let Some(v) = input.prior_used {
        if v < 0 {
            return Err(AppError::invalid("이전 학교 사용액은 0원 이상이어야 합니다."));
        }
    }
    if input.confirmed && input.prior_used.is_none() {
        return Err(AppError::invalid("확인 처리하려면 이전 학교 사용액(0원이면 0)을 입력해야 합니다. 비워 둔 채 확인할 수 없습니다."));
    }
    let status = if input.confirmed { "confirmed" } else { "unconfirmed" };
    tx.execute(
        "UPDATE voucher_target SET prior_used = ?2, prior_status = ?3, prior_note = ?4, updated_at = datetime('now', 'localtime') WHERE id = ?1",
        params![t.id, input.prior_used, status, input.note.trim()],
    )?;
    tx.execute(
        "INSERT INTO voucher_target_log (target_id, action, from_status, to_status, detail) VALUES (?1, 'prior', ?2, ?3, ?4)",
        params![
            t.id,
            t.prior_status,
            status,
            format!("이전 학교 사용액 {}{}", input.prior_used.map(|v| format!("{}원", w(v))).unwrap_or_else(|| "미입력".into()), if input.note.trim().is_empty() { String::new() } else { format!(" · {}", input.note.trim()) })
        ],
    )?;
    audit::write(tx, "voucher.prior", "voucher_target", Some(t.id), &format!("전입생 이전 학교 사용액 {}", prior_status_label(status)), serde_json::json!({}))?;
    target_get(tx, t.id)
}

/// 학생 직접 등록 (명단 파일에 없는 전입생 등). 네 가지가 모두 같으면 기존 학생.
pub fn student_create(tx: &Connection, input: &StudentCreateInput) -> AppResult<StudentRow> {
    if input.name.trim().is_empty() {
        return Err(AppError::invalid("이름을 입력해 주세요."));
    }
    if !(1..=6).contains(&input.grade) {
        return Err(AppError::invalid("학년은 1~6 이어야 합니다."));
    }
    year::get(tx, input.school_year_id)?;
    let key = crate::excel::parse::StudentKey { grade: Some(input.grade), class_name: input.class_name.trim().into(), student_no: input.student_no.trim().into(), name: input.name.trim().into() };
    let id = crate::repo::student::upsert(tx, input.school_year_id, &key)?.ok_or_else(|| AppError::invalid("학생을 등록하지 못했습니다."))?;
    audit::write(tx, "student.create", "student", Some(id), "학생 직접 등록", serde_json::json!({}))?;
    Ok(StudentRow { id, label: student_label(tx, id)? })
}

// ── 사용액 계산 ─────────────────────────────────────────────────────────────

struct MonthRef {
    id: i64,
    seq: i32,
    ym: String,
    closed: bool,
}

fn months_of_year(conn: &Connection, school_year_id: i64) -> AppResult<Vec<MonthRef>> {
    Ok(year::months(conn, school_year_id)?.into_iter().map(|m| MonthRef { id: m.id, seq: m.seq, ym: m.ym, closed: m.status == "closed" }).collect())
}

fn seq_of_date(months: &[MonthRef], date: &str) -> Option<i32> {
    let ym = date.get(0..7)?;
    months.iter().find(|m| m.ym == ym).map(|m| m.seq)
}

/// 한 칸: 월 × 프로그램 × 항목의 재원별 징수 기준액 (살아 있고 실제 처리된 징수·재원경정 + 배분 귀속).
#[derive(Debug, Clone)]
struct Cell {
    month_id: i64,
    seq: i32,
    ym: String,
    program_id: i64,
    program_name: String,
    item_id: i64,
    item_name: String,
    by_fund: BTreeMap<i64, i64>,
}

fn load_cells(conn: &Connection, school_year_id: i64, student_id: i64, idx: &recon::FundIndex) -> AppResult<(Vec<Cell>, i64)> {
    let mut st = conn.prepare(
        "SELECT m.id, m.seq, m.ym, coalesce(e.program_id, a.program_id), coalesce(p.name, ''), e.amount_item_id, i.name, e.fund_type_id, coalesce(a.amount, e.amount)
           FROM ledger_entry e
           JOIN fiscal_month m ON m.id = e.fiscal_month_id
           JOIN amount_item i ON i.id = e.amount_item_id
           LEFT JOIN allocation a ON a.entry_id = e.id AND a.status = 'active' AND e.program_id IS NULL
           LEFT JOIN program p ON p.id = coalesce(e.program_id, a.program_id)
          WHERE e.school_year_id = ?1 AND e.student_id = ?2 AND e.state = 'active' AND e.stage = 'actual'
            AND e.flow IN ('collection', 'transfer') AND e.fund_type_id IS NOT NULL
          ORDER BY m.seq, e.program_id, e.amount_item_id",
    )?;
    let rows: Vec<(i64, i32, String, Option<i64>, String, i64, String, i64, i64)> = st
        .query_map(params![school_year_id, student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)))?
        .collect::<Result<_, _>>()?;
    let mut cells: Vec<Cell> = Vec::new();
    let mut unallocated = 0;
    for (mid, seq, ym, pid, pname, item, iname, fund, amount) in rows {
        let Some(pid) = pid else {
            unallocated += amount;
            continue;
        };
        let f = idx.effective(fund);
        match cells.iter_mut().find(|c| c.month_id == mid && c.program_id == pid && c.item_id == item) {
            Some(c) => *c.by_fund.entry(f).or_default() += amount,
            None => {
                let mut by = BTreeMap::new();
                by.insert(f, amount);
                cells.push(Cell { month_id: mid, seq, ym, program_id: pid, program_name: pname, item_id: item, item_name: iname, by_fund: by });
            }
        }
    }
    Ok((cells, unallocated))
}

/// 처리 예정(planned) 상태로 등록된 →자유수강권 재원경정 (칸별 합).
fn pending_transfers(conn: &Connection, school_year_id: i64, student_id: i64, idx: &recon::FundIndex, free: i64) -> AppResult<BTreeMap<(i64, i64, i64), i64>> {
    let mut st = conn.prepare(
        "SELECT fiscal_month_id, program_id, amount_item_id, to_fund_id, amount FROM adjustment
          WHERE school_year_id = ?1 AND student_id = ?2 AND kind = 'fund_transfer' AND status = 'planned'",
    )?;
    let rows: Vec<(i64, i64, i64, i64, i64)> = st.query_map(params![school_year_id, student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
    let mut out = BTreeMap::new();
    for (m, p, i, to, a) in rows {
        if idx.effective(to) == free {
            *out.entry((m, p, i)).or_default() += a;
        }
    }
    Ok(out)
}

struct Limits {
    spec: LimitSpec,
    source: String,
    label: String,
    /// None = 모든 항목 지원
    eligible: Option<Vec<i64>>,
    eligible_names: Vec<String>,
    semester_label: String,
    uncovered: Vec<String>,
}

fn limits_for(conn: &Connection, school_year_id: i64, student_id: i64) -> AppResult<Option<Limits>> {
    let pol = policy_current(conn, school_year_id)?;
    let exc = exception_current(conn, student_id)?;
    if pol.is_none() && exc.is_none() {
        return Ok(None);
    }
    let months = months_of_year(conn, school_year_id)?;
    let (semesters, eligible, eligible_names, semester_label, uncovered) = match &pol {
        Some(p) => (
            semesters_for(&months, &[p.semester1_from.clone(), p.semester1_to.clone(), p.semester2_from.clone(), p.semester2_to.clone()]),
            if p.all_items_eligible { None } else { Some(p.eligible_item_ids.clone()) },
            p.eligible_item_names.clone(),
            format!("1학기 {} ~ {} · 2학기 {} ~ {}", p.semester1_from, p.semester1_to, p.semester2_from, p.semester2_to),
            p.uncovered_months.clone(),
        ),
        None => (default_semesters(), None, catalog::amount_items(conn)?.iter().filter(|i| i.is_active).map(|i| i.name.clone()).collect(), "기본 학기 구분".into(), vec![]),
    };
    let (basis, annual, s1, s2, carry, source) = match (&exc, &pol) {
        (Some(x), _) => (x.basis.clone(), x.annual_limit, x.semester1_limit, x.semester2_limit, x.carry_over, "학생 예외"),
        (None, Some(p)) => (p.basis.clone(), p.annual_limit, p.semester1_limit, p.semester2_limit, p.carry_over, "학년도 정책"),
        _ => unreachable!(),
    };
    Ok(Some(Limits {
        label: limit_label(&basis, annual, s1, s2, carry),
        spec: LimitSpec { basis, annual, s1, s2, carry_over: carry, semesters },
        source: source.into(),
        eligible,
        eligible_names,
        semester_label,
        uncovered,
    }))
}

fn free_fund_id(conn: &Connection, idx: &recon::FundIndex) -> AppResult<i64> {
    Ok(idx.effective(catalog::fund_type_by_code(conn, "free_voucher")?.id))
}

fn beneficiary_root(conn: &Connection, idx: &recon::FundIndex) -> AppResult<i64> {
    Ok(idx.root(catalog::fund_type_by_code(conn, "beneficiary")?.id))
}

/// 학생 한 명의 사용액 현황. 계산은 저장하지 않는다.
pub fn usage(conn: &Connection, school_year_id: i64, student_id: i64, decisions: &[VoucherDecision]) -> AppResult<VoucherUsage> {
    ensure_student(conn, school_year_id, student_id)?;
    let idx = recon::FundIndex::load(conn)?;
    let free = free_fund_id(conn, &idx)?;
    let ben_root = beneficiary_root(conn, &idx)?;
    let months = months_of_year(conn, school_year_id)?;
    let target = target_of_student(conn, school_year_id, student_id)?;
    let (cells, unallocated) = load_cells(conn, school_year_id, student_id, &idx)?;
    let pending = pending_transfers(conn, school_year_id, student_id, &idx, free)?;
    let mut out = VoucherUsage {
        student_id,
        student: student_label(conn, student_id)?,
        target_id: target.as_ref().map(|t| t.id),
        target_status: target.as_ref().map(|t| t.status.clone()).unwrap_or_default(),
        target_status_label: target.as_ref().map(|t| t.status_label.clone()).unwrap_or_else(|| "대상자 아님".into()),
        apply_from: target.as_ref().and_then(|t| t.apply_from.clone()),
        apply_to: target.as_ref().and_then(|t| t.apply_to.clone()),
        basis: String::new(),
        basis_label: String::new(),
        limit_source: String::new(),
        limit_label: String::new(),
        limit_total: None,
        prior_used: target.as_ref().and_then(|t| t.prior_used),
        used_expected: 0,
        used_actual: 0,
        voucher_total: 0,
        g3x_total: 0,
        general_total: 0,
        dual: false,
        eligible_item_names: vec![],
        semester_label: String::new(),
        pending_total: 0,
        remaining: None,
        remaining_s1: None,
        remaining_s2: None,
        exhausted_ym: None,
        retro_total: 0,
        over_total: 0,
        needs_decision: false,
        decision_months: vec![],
        rows: vec![],
        blocked: vec![],
        warnings: vec![],
        notes: vec![],
    };
    if unallocated != 0 {
        out.warnings.push(format!("프로그램에 배분되지 않은 징수액 {}원은 계산에서 빠졌습니다. 징수액 배분을 먼저 확정해 주세요.", w(unallocated)));
    }
    let g3x = idx.effective(catalog::fund_type_by_code(conn, "g3_excess")?.id);
    let av = idx.effective(catalog::fund_type_by_code(conn, "after_voucher")?.id);
    let limits = limits_for(conn, school_year_id, student_id)?;
    if let Some(l) = &limits {
        out.basis = l.spec.basis.clone();
        out.basis_label = basis_label(&l.spec.basis).into();
        out.limit_source = l.source.clone();
        out.limit_label = l.label.clone();
        out.eligible_item_names = l.eligible_names.clone();
        out.semester_label = l.semester_label.clone();
        if l.spec.basis == "semester" && !l.uncovered.is_empty() {
            out.notes.push(format!("{} 은(는) 어느 학기에도 속하지 않는 미지원 기간입니다.", l.uncovered.join(", ")));
        }
    } else {
        out.blocked.push("지원 한도 정책이 없습니다. '지원 한도 설정'에서 학년도 정책을 먼저 등록해 주세요.".into());
    }
    // 적용 기간
    let mut window: Option<(i32, i32)> = None;
    let mut prior_used = 0;
    let mut prior_seq = 1;
    match &target {
        None => out.blocked.push("자유수강권 대상자로 등록되지 않은 학생입니다.".into()),
        Some(t) => match t.status.as_str() {
            "pending" => out.blocked.push("대상자 확인 전입니다. 확정 뒤에 사용액을 계산합니다.".into()),
            "cancelled" => out.blocked.push("대상 취소된 학생입니다.".into()),
            _ => {
                let from = t.apply_from.as_deref().and_then(|d| seq_of_date(&months, d));
                let to = match &t.apply_to {
                    Some(d) => seq_of_date(&months, d),
                    None => Some(12),
                };
                match (from, to) {
                    (Some(f), Some(tt)) => window = Some((f, tt)),
                    _ => out.blocked.push("적용 시작일(또는 종료일)이 비어 있거나 학년도 밖입니다.".into()),
                }
                if t.transfer_in {
                    if t.prior_needs_confirm {
                        out.blocked.push("전입생의 이전 학교 사용액 확인 필요 — '전입생 관리'에서 금액을 확인해야 사용액을 계산합니다. (0원으로 가정하지 않습니다)".into());
                    } else {
                        prior_used = t.prior_used.unwrap_or(0);
                        prior_seq = t.transfer_in_on.as_deref().and_then(|d| seq_of_date(&months, d)).or(from).unwrap_or(1);
                    }
                }
            }
        },
    }
    // 실제 사용액은 차단과 상관없이 보여 준다 (원장 사실).
    let eligible_item = |item: i64| limits.as_ref().and_then(|l| l.eligible.as_ref()).map(|ids| ids.contains(&item)).unwrap_or(true);
    let mut rows: Vec<VoucherUsageRow> = Vec::new();
    let plan = match (&limits, window) {
        (Some(l), Some(win)) if out.blocked.is_empty() => {
            // 지원 대상 수강료 = 자유수강권 + 수익자 계열(초3 초과금·일반 수익자). 방과후이용권·맞춤형지원 등 다른 지원 재원이 낸 몫은
            // 한도를 쓰지 않고 자유수강권으로 바꾸지도 않는다. 지원 대상이 아닌 항목은 계산에서 뺀다.
            let charges: Vec<Charge> = cells
                .iter()
                .filter(|c| eligible_item(c.item_id))
                .map(|c| Charge { seq: c.seq, program_id: c.program_id, item_id: c.item_id, tuition: c.by_fund.iter().filter(|(f, _)| **f == free || idx.root(**f) == ben_root).map(|x| x.1).sum() })
                .collect();
            let ds: Vec<Decision> = decisions
                .iter()
                .filter(|d| d.student_id == student_id)
                .filter_map(|d| months.iter().find(|m| m.id == d.fiscal_month_id).map(|m| Decision { seq: m.seq, program_id: d.program_id, item_id: d.amount_item_id, free_amount: d.free_amount }))
                .collect();
            Some(engine::plan(&l.spec, prior_used, prior_seq, win, &charges, &ds))
        }
        _ => None,
    };
    let semesters: Vec<i32> = limits.as_ref().map(|l| l.spec.semesters.clone()).unwrap_or_else(default_semesters);
    for c in &cells {
        let tuition: i64 = c.by_fund.values().sum();
        let free_actual = c.by_fund.get(&free).copied().unwrap_or(0);
        let g3x_actual = c.by_fund.get(&g3x).copied().unwrap_or(0);
        let general_actual: i64 = c.by_fund.iter().filter(|(f, _)| **f != free && **f != g3x && idx.root(**f) == ben_root).map(|x| x.1).sum();
        let beneficiary_actual = g3x_actual + general_actual;
        let voucher_actual = c.by_fund.get(&av).copied().unwrap_or(0);
        let other_actual = tuition - free_actual - beneficiary_actual - voucher_actual;
        let funds: Vec<VoucherFundPart> = c.by_fund.iter().map(|(f, a)| VoucherFundPart { fund_id: *f, fund_name: idx.name(*f), amount: *a }).collect();
        let pend = pending.get(&(c.month_id, c.program_id, c.item_id)).copied().unwrap_or(0);
        let in_window = window.map(|(f, t)| c.seq >= f && c.seq <= t).unwrap_or(false);
        let elig = eligible_item(c.item_id);
        let pr = plan.as_ref().and_then(|p| p.rows.iter().find(|r| r.seq == c.seq && r.program_id == c.program_id && r.item_id == c.item_id));
        let free_expected = pr.map(|r| r.free_expected).unwrap_or(0);
        let delta = free_expected - free_actual - pend;
        let state = if !elig {
            "excluded"
        } else if plan.is_none() {
            if free_actual > 0 && !in_window && window.is_some() { "over" } else { "outside" }
        } else if !in_window {
            if free_actual > 0 { "over" } else { "outside" }
        } else if pr.map(|r| r.needs_decision).unwrap_or(false) {
            "decide"
        } else if delta > 0 {
            "retro"
        } else if delta < 0 {
            "over"
        } else if pend > 0 {
            "pending"
        } else {
            "ok"
        };
        // 전환 우선순위: 초3 초과금 → 일반 수익자. 이용권 지원액은 전환 대상이 아니다.
        let (retro_g3x, retro_general) = if state == "retro" {
            let a = delta.min(g3x_actual.max(0));
            (a, (delta - a).min(general_actual.max(0)).max(0))
        } else {
            (0, 0)
        };
        rows.push(VoucherUsageRow {
            fiscal_month_id: c.month_id,
            ym: c.ym.clone(),
            month_label: format!("{} ({})", months::label(&c.ym), c.ym),
            seq: c.seq,
            program_id: c.program_id,
            program_name: c.program_name.clone(),
            amount_item_id: c.item_id,
            item_name: c.item_name.clone(),
            tuition,
            free_actual,
            beneficiary_actual,
            g3x_actual,
            general_actual,
            voucher_actual,
            other_actual,
            funds,
            eligible_item: elig,
            semester: semesters.get((c.seq - 1).max(0) as usize).copied().unwrap_or(0),
            free_expected,
            beneficiary_expected: ((free_actual + beneficiary_actual).max(0) - free_expected).max(0),
            pending_transfer: pend,
            delta,
            remaining_before: pr.and_then(|r| r.remaining_before),
            remaining_after: pr.and_then(|r| r.remaining_after),
            state: state.into(),
            state_label: usage_state_label(state).into(),
            in_window,
            month_closed: months.iter().find(|m| m.id == c.month_id).map(|m| m.closed).unwrap_or(false),
            retro_g3x,
            retro_general,
        });
    }
    rows.sort_by_key(|r| (r.seq, r.program_name.clone(), r.amount_item_id));
    out.used_actual = rows.iter().map(|r| r.free_actual).sum();
    out.voucher_total = rows.iter().map(|r| r.voucher_actual).sum();
    out.g3x_total = rows.iter().map(|r| r.g3x_actual).sum();
    out.general_total = rows.iter().map(|r| r.general_actual).sum();
    out.dual = out.voucher_total > 0;
    out.pending_total = rows.iter().map(|r| r.pending_transfer).sum();
    out.used_expected = rows.iter().map(|r| r.free_expected).sum();
    out.retro_total = rows.iter().filter(|r| r.state == "retro").map(|r| r.delta).sum();
    out.over_total = rows.iter().filter(|r| r.state == "over").map(|r| -r.delta).sum();
    out.rows = rows;
    if let Some(p) = &plan {
        out.limit_total = p.limit_total;
        out.remaining = p.remaining_end;
        out.remaining_s1 = p.remaining_s1;
        out.remaining_s2 = p.remaining_s2;
        out.exhausted_ym = p.exhausted_seq.and_then(|s| months.iter().find(|m| m.seq == s).map(|m| m.ym.clone()));
        out.needs_decision = !p.needs_decision_seqs.is_empty();
        out.decision_months = p.needs_decision_seqs.iter().filter_map(|s| months.iter().find(|m| m.seq == *s).map(|m| format!("{} ({})", months::label(&m.ym), m.ym))).collect();
        out.notes = p.notes.clone();
        if out.needs_decision {
            out.warnings.push(format!("{} 에 여러 프로그램이 있는데 남은 한도가 모자랍니다. 프로그램별 자유수강권 배분을 정해야 경정 대상이 됩니다 (자료에 수강 시작일이 없어 자동으로 정하지 않습니다).", out.decision_months.join(", ")));
        }
        if out.over_total > 0 {
            out.warnings.push(format!("계산 지원액보다 실제 자유수강권 사용액이 {}원 많습니다. 한도 초과분은 자동으로 되돌리지 않으니 원본 회계자료로 확인해 주세요.", w(out.over_total)));
        }
    }
    Ok(out)
}

/// 대상자 전원의 사용액 현황.
pub fn usage_all(conn: &Connection, school_year_id: i64) -> AppResult<Vec<VoucherUsage>> {
    let targets = target_list(conn, school_year_id)?;
    let mut out = Vec::with_capacity(targets.len());
    for t in targets {
        out.push(usage(conn, school_year_id, t.student_id, &[])?);
    }
    Ok(out)
}

// ── 소급 지원 계산 · 일괄 경정 ─────────────────────────────────────────────

fn proposal_state_label(s: &str) -> &'static str {
    match s {
        "normal" => "정상 (등록 가능)",
        "check" => "재원 확인 필요",
        "blocked" => "차단",
        "decide" => "배분 결정 필요",
        "pending" => "처리 예정 경정 있음",
        "over" => "확인 대상",
        "short" => "수익자 부담분 부족",
        _ => "",
    }
}

/// 소급(또는 당월) 경정 미리보기 — 학생별 사용액을 계산해 '계산 지원액 − 실제 자유수강권' 만큼 수익자 계열 → 자유수강권 경정을 제안한다.
/// 우선순위: 초3 초과금 → 일반 수익자. 방과후이용권 중복 대상자의 일반 수익자 금액은 '재원 확인 필요'로 두고 담당자 확인 뒤에만 등록한다.
/// 이미 자유수강권인 금액은 빼고, 처리 예정 경정은 중복으로 보지 않도록 뺀다. 등록은 하지 않는다.
pub fn retro_preview(conn: &Connection, school_year_id: i64, student_ids: &[i64], decisions: &[VoucherDecision]) -> AppResult<VoucherRetroPreview> {
    let idx = recon::FundIndex::load(conn)?;
    let free = free_fund_id(conn, &idx)?;
    let ben_root = beneficiary_root(conn, &idx)?;
    let g3x = idx.effective(catalog::fund_type_by_code(conn, "g3_excess")?.id);
    let ids: Vec<i64> = if student_ids.is_empty() {
        target_list(conn, school_year_id)?.into_iter().filter(|t| matches!(t.status.as_str(), "confirmed" | "ended")).map(|t| t.student_id).collect()
    } else {
        student_ids.to_vec()
    };
    let mut usages = Vec::new();
    let mut proposals: Vec<VoucherProposal> = Vec::new();
    for sid in ids {
        let u = usage(conn, school_year_id, sid, decisions)?;
        for r in &u.rows {
            let base = VoucherProposal {
                key: format!("{}-{}-{}-{}", sid, r.fiscal_month_id, r.program_id, r.amount_item_id),
                student_id: sid,
                student: u.student.clone(),
                fiscal_month_id: r.fiscal_month_id,
                month_label: r.month_label.clone(),
                program_id: r.program_id,
                program_name: r.program_name.clone(),
                amount_item_id: r.amount_item_id,
                item_name: r.item_name.clone(),
                from_fund_id: None,
                from_fund_name: String::new(),
                amount: 0,
                state: String::new(),
                state_label: String::new(),
                voucher_actual: r.voucher_actual,
                g3x_actual: r.g3x_actual,
                general_actual: r.general_actual,
                free_actual: r.free_actual,
                remaining_after: 0,
                limit_after: r.remaining_after,
                reasons: vec![],
            };
            match r.state.as_str() {
                "retro" => {
                    let mut left = r.delta;
                    // 우선순위: 초3 초과금 먼저, 그 다음 일반 수익자(정렬 순)
                    let mut parts: Vec<&VoucherFundPart> = r.funds.iter().filter(|f| f.fund_id != free && idx.root(f.fund_id) == ben_root && f.amount > 0).collect();
                    parts.sort_by_key(|f| (if f.fund_id == g3x { 0 } else { 1 }, idx.sort_key(f.fund_id)));
                    for f in parts {
                        if left <= 0 {
                            break;
                        }
                        let amount = left.min(f.amount);
                        let input = AdjustmentInput {
                            school_year_id,
                            fiscal_month_id: r.fiscal_month_id,
                            kind: "fund_transfer".into(),
                            program_id: r.program_id,
                            student_id: Some(sid),
                            from_fund_id: Some(f.fund_id),
                            to_fund_id: Some(free),
                            fund_type_id: None,
                            amount_item_id: r.amount_item_id,
                            amount,
                            decided_on: None,
                            processed_on: None,
                            status: "planned".into(),
                            reason: "자유수강권 소급 지원 (미리보기)".into(),
                            related_entry_id: None,
                            related_adjustment_id: None,
                            confirm_duplicate: false,
                        };
                        let p = adjustment::preview(conn, &input)?;
                        let (state, mut reasons) = if p.blocked.is_empty() { ("normal", vec![]) } else { ("blocked", p.blocked.clone()) };
                        let state = if state == "normal" && f.fund_id != g3x && u.dual {
                            reasons.push("방과후이용권 중복 대상자의 일반 수익자 금액입니다. 같은 학생의 지원 대상 수강료이고 이용권·다른 지원 재원과 겹치지 않는지 확인한 뒤 '재원 확인'을 켜야 등록됩니다.".into());
                            "check"
                        } else {
                            state
                        };
                        if f.fund_id == g3x && u.dual {
                            reasons.push("방과후이용권 중복 대상자 — 초3 초과금을 우선 지원합니다.".into());
                        }
                        proposals.push(VoucherProposal {
                            key: format!("{}-{}", base.key, f.fund_id),
                            from_fund_id: Some(f.fund_id),
                            from_fund_name: f.fund_name.clone(),
                            amount,
                            state: state.into(),
                            state_label: proposal_state_label(state).into(),
                            remaining_after: f.amount - amount,
                            reasons,
                            ..base.clone()
                        });
                        left -= amount;
                    }
                    if left > 0 {
                        let others: Vec<String> = r.funds.iter().filter(|f| f.fund_id != free && idx.root(f.fund_id) != ben_root).map(|f| format!("{} {}원", f.fund_name, w(f.amount))).collect();
                        proposals.push(VoucherProposal {
                            key: format!("{}-short", base.key),
                            amount: left,
                            state: "short".into(),
                            state_label: proposal_state_label("short").into(),
                            reasons: vec![format!(
                                "계산 지원액 중 {}원은 수익자 부담분이 없어 경정할 수 없습니다{}. 방과후이용권 등 다른 지원 재원을 자유수강권으로 바꾸지 않습니다.",
                                w(left),
                                if others.is_empty() { String::new() } else { format!(" (다른 재원: {})", others.join(", ")) }
                            )],
                            ..base.clone()
                        });
                    }
                }
                "decide" => proposals.push(VoucherProposal { amount: r.free_expected - r.free_actual - r.pending_transfer, state: "decide".into(), state_label: proposal_state_label("decide").into(), reasons: vec!["같은 달 여러 프로그램에 한도가 모자랍니다. 프로그램별 배분을 정한 뒤 다시 계산해 주세요.".into()], ..base.clone() }),
                "pending" => proposals.push(VoucherProposal { amount: r.pending_transfer, state: "pending".into(), state_label: proposal_state_label("pending").into(), reasons: vec![format!("처리 예정 재원경정 {}원이 이미 등록되어 있습니다. 회계 정정 이력에서 완료 처리해 주세요 (중복 등록하지 않습니다).", w(r.pending_transfer))], ..base.clone() }),
                "over" => proposals.push(VoucherProposal { amount: -r.delta, state: "over".into(), state_label: proposal_state_label("over").into(), reasons: vec![format!("실제 자유수강권 사용액이 계산 지원액보다 {}원 많습니다. 한도 초과분은 자동으로 되돌리지 않습니다 — 원본 회계자료로 확인하세요.", w(-r.delta))], ..base.clone() }),
                _ => {}
            }
        }
        usages.push(u);
    }
    let normal_count = proposals.iter().filter(|p| p.state == "normal").count() as i64;
    let normal_total = proposals.iter().filter(|p| p.state == "normal").map(|p| p.amount).sum();
    let check_count = proposals.iter().filter(|p| p.state == "check").count() as i64;
    let check_total = proposals.iter().filter(|p| p.state == "check").map(|p| p.amount).sum();
    let blocked_count = proposals.iter().filter(|p| p.state != "normal" && p.state != "check").count() as i64;
    Ok(VoucherRetroPreview { student_count: usages.len() as i64, usages, proposals, normal_count, normal_total, check_count, check_total, blocked_count })
}

/// 승인 묶음 등록/// 승인 묶음 등록 — 담당자가 고른 행만, 등록 직전에 다시 계산·검증한다. 하나라도 어긋나면 전부 되돌린다 (조용히 건너뛰지 않는다).
pub fn apply(tx: &Connection, input: &VoucherApplyInput) -> AppResult<VoucherBatch> {
    if !matches!(input.kind.as_str(), "retro" | "monthly") {
        return Err(AppError::invalid("처리 종류가 올바르지 않습니다."));
    }
    if !matches!(input.status.as_str(), "planned" | "done") {
        return Err(AppError::invalid("처리 상태는 처리 예정 또는 처리 완료여야 합니다."));
    }
    if input.rows.is_empty() {
        return Err(AppError::invalid("등록할 행이 없습니다."));
    }
    if input.reason.trim().is_empty() {
        return Err(AppError::invalid("사유를 입력해 주세요."));
    }
    check_date("결정일", &input.decided_on)?;
    check_date("처리일", &input.processed_on)?;
    if input.status == "done" && blank(&input.processed_on).is_none() {
        return Err(AppError::invalid("처리 완료로 등록하려면 실제 처리일이 필요합니다."));
    }
    let idx = recon::FundIndex::load(tx)?;
    let free = free_fund_id(tx, &idx)?;
    let ben_root = beneficiary_root(tx, &idx)?;
    let g3x = idx.effective(catalog::fund_type_by_code(tx, "g3_excess")?.id);
    // 재검증: 학생별 사용액을 지금 다시 계산해 각 행이 '경정 필요' 금액 안에 있는지 본다.
    let mut by_student: BTreeMap<i64, Vec<&VoucherApplyRow>> = BTreeMap::new();
    for r in &input.rows {
        by_student.entry(r.student_id).or_default().push(r);
    }
    let mut errors: Vec<String> = Vec::new();
    for (sid, rows) in &by_student {
        let u = usage(tx, input.school_year_id, *sid, &input.decisions)?;
        if !u.blocked.is_empty() {
            errors.push(format!("{}: {}", u.student, u.blocked.join(" / ")));
            continue;
        }
        let mut used: BTreeMap<(i64, i64, i64), i64> = BTreeMap::new();
        let mut used_fund: BTreeMap<(i64, i64, i64, i64), i64> = BTreeMap::new();
        let mut taken_g3x: BTreeMap<(i64, i64, i64), i64> = BTreeMap::new();
        let mut taken_general: BTreeMap<(i64, i64, i64), i64> = BTreeMap::new();
        for r in rows {
            if r.amount <= 0 {
                errors.push(format!("{}: 금액은 0원보다 커야 합니다.", u.student));
                continue;
            }
            let Some(cell) = u.rows.iter().find(|x| x.fiscal_month_id == r.fiscal_month_id && x.program_id == r.program_id && x.amount_item_id == r.amount_item_id) else {
                errors.push(format!("{}: 해당 월·프로그램·항목의 징수 기준액이 없습니다.", u.student));
                continue;
            };
            if cell.state != "retro" {
                errors.push(format!("{} · {} · {}: 지금 상태가 '{}' 이어서 경정 대상이 아닙니다.", u.student, cell.month_label, cell.program_name, cell.state_label));
                continue;
            }
            let key = (r.fiscal_month_id, r.program_id, r.amount_item_id);
            let acc = used.entry(key).or_default();
            if *acc + r.amount > cell.delta {
                errors.push(format!("{} · {} · {}: 경정 금액 합 {}원이 계산 지원액 차이 {}원을 넘습니다 (한도 초과 지원 불가).", u.student, cell.month_label, cell.program_name, w(*acc + r.amount), w(cell.delta)));
                continue;
            }
            *acc += r.amount;
            let Some(part) = cell.funds.iter().find(|f| f.fund_id == idx.effective(r.from_fund_id)) else {
                errors.push(format!("{} · {} · {}: 변경 전 재원에 징수 기준액이 없습니다.", u.student, cell.month_label, cell.program_name));
                continue;
            };
            if part.fund_id == free || idx.root(part.fund_id) != ben_root {
                errors.push(format!("{} · {} · {}: 수익자 부담 재원만 자유수강권으로 경정합니다 ('{}' 불가).", u.student, cell.month_label, cell.program_name, part.fund_name));
                continue;
            }
            let fk = (r.fiscal_month_id, r.program_id, r.amount_item_id, part.fund_id);
            let facc = used_fund.entry(fk).or_default();
            if *facc + r.amount > part.amount {
                errors.push(format!("{} · {} · {}: '{}' 기준액 {}원보다 큰 금액을 경정할 수 없습니다.", u.student, cell.month_label, cell.program_name, part.fund_name, w(part.amount)));
                continue;
            }
            *facc += r.amount;
            if part.fund_id == g3x {
                *taken_g3x.entry(key).or_default() += r.amount;
            } else {
                if u.dual && !r.confirmed {
                    errors.push(format!("{} · {} · {}: 방과후이용권 중복 대상자의 일반 수익자 금액은 담당자가 '재원 확인'을 켜야 경정합니다.", u.student, cell.month_label, cell.program_name));
                    continue;
                }
                *taken_general.entry(key).or_default() += r.amount;
            }
        }
        // 우선순위: 초3 초과금을 다 쓰기 전에 일반 수익자를 전환하지 않는다.
        for (key, gen) in &taken_general {
            if *gen <= 0 {
                continue;
            }
            let Some(cell) = u.rows.iter().find(|x| x.fiscal_month_id == key.0 && x.program_id == key.1 && x.amount_item_id == key.2) else { continue };
            let need_g3x = cell.delta.min(cell.g3x_actual.max(0));
            if taken_g3x.get(key).copied().unwrap_or(0) < need_g3x {
                errors.push(format!("{} · {} · {}: 초3 초과금 {}원을 먼저 자유수강권으로 지원해야 합니다 (우선순위).", u.student, cell.month_label, cell.program_name, w(need_g3x)));
            }
        }
    }
    if !errors.is_empty() {
        return Err(AppError::invalid(format!("재검증에서 막혀 아무것도 등록하지 않았습니다: {}", errors.join(" // "))));
    }
    // 등록 — P3 adjustment::create 가 다시 검증(확정월·중복·음수 잔액)하고 막히면 오류 → 트랜잭션 전체 되돌림.
    let mut adjustment_ids: Vec<(i64, &VoucherApplyRow)> = Vec::new();
    for r in &input.rows {
        let a = adjustment::create(
            tx,
            &AdjustmentInput {
                school_year_id: input.school_year_id,
                fiscal_month_id: r.fiscal_month_id,
                kind: "fund_transfer".into(),
                program_id: r.program_id,
                student_id: Some(r.student_id),
                from_fund_id: Some(r.from_fund_id),
                to_fund_id: Some(free),
                fund_type_id: None,
                amount_item_id: r.amount_item_id,
                amount: r.amount,
                decided_on: blank(&input.decided_on),
                processed_on: blank(&input.processed_on),
                status: input.status.clone(),
                reason: format!("[자유수강권 {}] {}", batch_kind_label(&input.kind), input.reason.trim()),
                related_entry_id: None,
                related_adjustment_id: None,
                confirm_duplicate: false,
            },
        )
        .map_err(|e| AppError::invalid(format!("등록 중 막혀 전부 되돌렸습니다: {}", e.message)))?;
        adjustment_ids.push((a.id, r));
    }
    let total: i64 = input.rows.iter().map(|r| r.amount).sum();
    let single_student = if by_student.len() == 1 { by_student.keys().next().copied() } else { None };
    tx.execute(
        "INSERT INTO voucher_batch (school_year_id, kind, student_id, reason, item_count, total_amount) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![input.school_year_id, input.kind, single_student, input.reason.trim(), input.rows.len() as i64, total],
    )?;
    let batch_id = tx.last_insert_rowid();
    for (aid, r) in &adjustment_ids {
        tx.execute(
            "INSERT INTO voucher_batch_item (batch_id, adjustment_id, student_id, fiscal_month_id, program_id, amount) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![batch_id, aid, r.student_id, r.fiscal_month_id, r.program_id, r.amount],
        )?;
    }
    // 검산: 묶음 합 = 등록된 경정 합, 재원 이동 쌍의 합 0
    let (n, sum, bal): (i64, i64, i64) = tx.query_row(
        "SELECT count(*), coalesce(sum(a.amount), 0), coalesce((SELECT sum(e.amount) FROM ledger_entry e JOIN adjustment_entry ae ON ae.entry_id = e.id JOIN voucher_batch_item bi ON bi.adjustment_id = ae.adjustment_id WHERE bi.batch_id = ?1), 0)
           FROM voucher_batch_item bi JOIN adjustment a ON a.id = bi.adjustment_id WHERE bi.batch_id = ?1",
        [batch_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if n != input.rows.len() as i64 || sum != total || bal != 0 {
        return Err(AppError::new("VERIFY", "묶음 검산이 맞지 않아 등록을 되돌렸습니다."));
    }
    audit::write(tx, "voucher.batch", "voucher_batch", Some(batch_id), &format!("자유수강권 {} 묶음 등록 ({}건, {})", batch_kind_label(&input.kind), n, adjustment::status_label(&input.status)), serde_json::json!({ "studentCount": by_student.len() }))?;
    batch_get(tx, batch_id)
}

fn batch_rows(conn: &Connection, school_year_id: i64, only_id: Option<i64>) -> AppResult<Vec<VoucherBatch>> {
    let mut st = conn.prepare(
        "SELECT b.id, b.school_year_id, b.kind, b.student_id, coalesce(s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, ''), b.reason, b.item_count, b.total_amount, b.created_at
           FROM voucher_batch b LEFT JOIN student s ON s.id = b.student_id
          WHERE (?1 IS NULL OR b.id = ?1) AND (?1 IS NOT NULL OR b.school_year_id = ?2) ORDER BY b.id DESC",
    )?;
    let mut out: Vec<VoucherBatch> = st
        .query_map(params![only_id, school_year_id], |r| {
            let kind: String = r.get(2)?;
            Ok(VoucherBatch { id: r.get(0)?, school_year_id: r.get(1)?, kind_label: batch_kind_label(&kind).into(), kind, student_id: r.get(3)?, student: r.get(4)?, reason: r.get(5)?, item_count: r.get(6)?, total_amount: r.get(7)?, created_at: r.get(8)?, items: vec![] })
        })?
        .collect::<Result<_, _>>()?;
    let mut st_i = conn.prepare(
        "SELECT bi.adjustment_id, bi.student_id, s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, m.ym, p.name, i.name, coalesce(ff.name, ''), bi.amount, a.status
           FROM voucher_batch_item bi JOIN adjustment a ON a.id = bi.adjustment_id JOIN student s ON s.id = bi.student_id
           JOIN fiscal_month m ON m.id = bi.fiscal_month_id JOIN program p ON p.id = bi.program_id JOIN amount_item i ON i.id = a.amount_item_id
           LEFT JOIN fund_type ff ON ff.id = a.from_fund_id
          WHERE bi.batch_id = ?1 ORDER BY bi.adjustment_id",
    )?;
    for b in out.iter_mut() {
        b.items = st_i
            .query_map([b.id], |r| {
                let ym: String = r.get(3)?;
                let status: String = r.get(8)?;
                Ok(VoucherBatchItem { adjustment_id: r.get(0)?, student_id: r.get(1)?, student: r.get(2)?, month_label: format!("{} ({ym})", months::label(&ym)), program_name: r.get(4)?, item_name: r.get(5)?, from_fund_name: r.get(6)?, amount: r.get(7)?, status_label: adjustment::status_label(&status).into(), status })
            })?
            .collect::<Result<_, _>>()?;
    }
    Ok(out)
}

pub fn batch_get(conn: &Connection, id: i64) -> AppResult<VoucherBatch> {
    batch_rows(conn, 0, Some(id))?.into_iter().next().ok_or_else(|| AppError::not_found("묶음을 찾지 못했습니다."))
}

pub fn batch_list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<VoucherBatch>> {
    batch_rows(conn, school_year_id, None)
}

// ── 환불 처리 ───────────────────────────────────────────────────────────────

/// 환불 대상 — 수익자 → 자유수강권 재원경정(학생 단위) 전부. 경정이 수익자 금액을 줄였으므로 그만큼 학부모에게 돌려줘야 한다.
/// 반환 이행(refund_return)의 누계로 상태를 낸다. 자동으로 완료되지 않는다.
pub fn refund_list(conn: &Connection, school_year_id: i64) -> AppResult<Vec<VoucherRefundRow>> {
    let idx = recon::FundIndex::load(conn)?;
    let free = free_fund_id(conn, &idx)?;
    let ben_root = beneficiary_root(conn, &idx)?;
    let transfers = adjustment::list(conn, &AdjustmentFilter { school_year_id, kind: Some("fund_transfer".into()), ..Default::default() })?;
    let mut st_ret = conn.prepare(
        "SELECT a.id, a.amount, a.status, a.processed_on, coalesce(v.collected_confirmed, 0), coalesce(v.evidence, '')
           FROM adjustment a LEFT JOIN voucher_refund v ON v.adjustment_id = a.id
          WHERE a.kind = 'refund_return' AND a.related_adjustment_id = ?1 ORDER BY a.id",
    )?;
    let mut st_batch = conn.prepare("SELECT batch_id FROM voucher_batch_item WHERE adjustment_id = ?1")?;
    let mut out = Vec::new();
    for t in transfers {
        let Some(sid) = t.student_id else { continue };
        let (from_id, to_id): (i64, i64) = conn.query_row("SELECT from_fund_id, to_fund_id FROM adjustment WHERE id = ?1", [t.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        if idx.effective(to_id) != free || idx.root(from_id) != ben_root {
            continue;
        }
        let returns: Vec<VoucherRefundReturn> = st_ret
            .query_map([t.id], |r| {
                let status: String = r.get(2)?;
                Ok(VoucherRefundReturn { adjustment_id: r.get(0)?, amount: r.get(1)?, status_label: adjustment::status_label(&status).into(), status, processed_on: r.get(3)?, collected_confirmed: r.get::<_, i64>(4)? == 1, evidence: r.get(5)? })
            })?
            .collect::<Result<_, _>>()?;
        let returned_done: i64 = returns.iter().filter(|r| r.status == "done").map(|r| r.amount).sum();
        let returned_planned: i64 = returns.iter().filter(|r| r.status == "planned").map(|r| r.amount).sum();
        let status = if t.status == "cancelled" {
            "cancelled"
        } else if returned_done >= t.amount {
            "complete"
        } else if returned_done > 0 {
            "partial"
        } else if returned_planned > 0 {
            "planned"
        } else {
            "none"
        };
        let batch_id: Option<i64> = st_batch.query_row([t.id], |r| r.get(0)).optional()?;
        out.push(VoucherRefundRow {
            transfer_id: t.id,
            student_id: sid,
            student: t.student.clone(),
            fiscal_month_id: t.fiscal_month_id,
            month_label: t.month_label.clone(),
            program_name: t.program_name.clone(),
            item_name: t.item_name.clone(),
            from_fund_id: from_id,
            from_fund_name: t.from_fund_name.clone(),
            amount: t.amount,
            transfer_status_label: t.status_label.clone(),
            transfer_status: t.status.clone(),
            transfer_processed_on: t.processed_on.clone(),
            from_batch: batch_id.is_some(),
            batch_id,
            returned_done,
            returned_planned,
            outstanding: (t.amount - returned_done).max(0),
            status: status.into(),
            status_label: refund_status_label(status).into(),
            orphan: t.orphan,
            returns,
        });
    }
    Ok(out)
}

/// 환불(반환 이행) 등록 — P3 refund_return 으로 등록하고 '실제 수납 확인' 근거를 덧붙인다.
/// 처리 완료는 수납 확인 + 처리일 + 근거가 있어야 한다. 원장 징수액은 다시 차감하지 않는다 (경정이 이미 줄였다).
pub fn refund_register(tx: &Connection, input: &VoucherRefundInput) -> AppResult<Adjustment> {
    let t = adjustment::get(tx, input.transfer_id)?;
    if t.kind != "fund_transfer" || t.status == "cancelled" {
        return Err(AppError::invalid("근거 재원경정이 없거나 취소되었습니다."));
    }
    if t.student_id.is_none() {
        return Err(AppError::invalid("학생 단위 재원경정에만 환불을 등록합니다."));
    }
    if t.status != "done" {
        return Err(AppError::invalid("재원경정이 처리 완료된 뒤에 환불을 등록할 수 있습니다 (처리 예정 경정에는 환불을 확정하지 않습니다)."));
    }
    if !matches!(input.status.as_str(), "planned" | "done") {
        return Err(AppError::invalid("처리 상태는 처리 예정 또는 처리 완료여야 합니다."));
    }
    if input.amount <= 0 {
        return Err(AppError::invalid("환불 금액은 0원보다 커야 합니다."));
    }
    if input.reason.trim().is_empty() {
        return Err(AppError::invalid("사유를 입력해 주세요."));
    }
    check_date("처리일", &input.processed_on)?;
    if input.status == "done" {
        if !input.collected_confirmed {
            return Err(AppError::invalid("실제 수납 근거를 확인하지 않으면 환불을 완료 처리할 수 없습니다. '실제 수납 확인'을 켜 주세요."));
        }
        if input.evidence.trim().is_empty() {
            return Err(AppError::invalid("환불 완료에는 수납 근거(수납 자료·일자 등)를 적어야 합니다."));
        }
        if blank(&input.processed_on).is_none() {
            return Err(AppError::invalid("환불 완료에는 실제 처리일이 필요합니다."));
        }
    }
    let (from_id,): (i64,) = tx.query_row("SELECT from_fund_id FROM adjustment WHERE id = ?1", [t.id], |r| Ok((r.get(0)?,)))?;
    let (_, item_id): (i64, i64) = tx.query_row("SELECT id, amount_item_id FROM adjustment WHERE id = ?1", [t.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let a = adjustment::create(
        tx,
        &AdjustmentInput {
            school_year_id: t.school_year_id,
            fiscal_month_id: t.fiscal_month_id,
            kind: "refund_return".into(),
            program_id: t.program_id,
            student_id: t.student_id,
            from_fund_id: None,
            to_fund_id: None,
            fund_type_id: Some(from_id),
            amount_item_id: item_id,
            amount: input.amount,
            decided_on: None,
            processed_on: blank(&input.processed_on),
            status: input.status.clone(),
            reason: format!("[자유수강권 환불] {}", input.reason.trim()),
            related_entry_id: None,
            related_adjustment_id: Some(t.id),
            confirm_duplicate: false,
        },
    )?;
    tx.execute(
        "INSERT INTO voucher_refund (adjustment_id, transfer_adjustment_id, collected_confirmed, evidence) VALUES (?1, ?2, ?3, ?4)",
        params![a.id, t.id, input.collected_confirmed as i64, input.evidence.trim()],
    )?;
    audit::write(tx, "voucher.refund", "voucher_refund", Some(a.id), &format!("자유수강권 환불 {} 등록", adjustment::status_label(&input.status)), serde_json::json!({ "transferId": t.id }))?;
    Ok(a)
}

/// 처리 예정 환불을 완료로 — 수납 확인·근거 필수. 상태 전환은 P3 set_status 를 쓴다.
pub fn refund_complete(tx: &Connection, adjustment_id: i64, processed_on: &str, collected_confirmed: bool, evidence: &str) -> AppResult<Adjustment> {
    let a = adjustment::get(tx, adjustment_id)?;
    if a.kind != "refund_return" {
        return Err(AppError::invalid("환불(반환 이행) 정정이 아닙니다."));
    }
    if !collected_confirmed || evidence.trim().is_empty() {
        return Err(AppError::invalid("실제 수납 근거를 확인·기록하지 않으면 환불을 완료 처리할 수 없습니다."));
    }
    let r = adjustment::set_status(tx, adjustment_id, "done", Some(processed_on), "자유수강권 환불 완료")?;
    let exists: i64 = tx.query_row("SELECT count(*) FROM voucher_refund WHERE adjustment_id = ?1", [adjustment_id], |r| r.get(0))?;
    if exists == 0 {
        tx.execute(
            "INSERT INTO voucher_refund (adjustment_id, transfer_adjustment_id, collected_confirmed, evidence) VALUES (?1, ?2, 1, ?3)",
            params![adjustment_id, a.related_adjustment_id, evidence.trim()],
        )?;
    } else {
        tx.execute("UPDATE voucher_refund SET collected_confirmed = 1, evidence = ?2 WHERE adjustment_id = ?1", params![adjustment_id, evidence.trim()])?;
        audit::write(tx, "voucher.refund_evidence", "voucher_refund", Some(adjustment_id), "환불 완료 수납 근거 확인", serde_json::json!({}))?;
    }
    Ok(r)
}

// ── 처리 이력 ───────────────────────────────────────────────────────────────

pub fn history(conn: &Connection, school_year_id: i64) -> AppResult<VoucherHistory> {
    let mut st = conn.prepare(
        "SELECT l.target_id, s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, l.action, l.from_status, l.to_status, l.detail, l.at
           FROM voucher_target_log l JOIN voucher_target t ON t.id = l.target_id JOIN student s ON s.id = t.student_id
          WHERE t.school_year_id = ?1 ORDER BY l.id DESC",
    )?;
    let target_logs: Vec<VoucherTargetLogRow> = st
        .query_map([school_year_id], |r| Ok(VoucherTargetLogRow { target_id: r.get(0)?, student: r.get(1)?, action: r.get(2)?, from_status: r.get(3)?, to_status: r.get(4)?, detail: r.get(5)?, at: r.get(6)? }))?
        .collect::<Result<_, _>>()?;
    Ok(VoucherHistory { batches: batch_list(conn, school_year_id)?, target_logs, policy_history: policy_history(conn, school_year_id)?, exception_history: exception_list(conn, school_year_id, true)? })
}

#[cfg(test)]
#[path = "voucher_tests.rs"]
mod tests;
