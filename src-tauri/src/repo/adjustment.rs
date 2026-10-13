//! 수동 회계 정정 — 재원경정 · 수강료 환불 · 재원경정에 따른 반환 이행 · 추가징수.
//!
//! 세 가지를 섞지 않는다.
//!   재원경정(fund_transfer)   재원의 변경. 원장에 합이 0 인 거래 쌍. 전체 징수 기준액은 그대로.
//!   수강료 환불(refund)       징수 기준액 감소. 원장에 음수 징수 거래. 귀속월은 원래 달, 처리일은 실제 날짜.
//!   반환 이행(refund_return)  재원경정 뒤 학부모에게 실제로 돌려준 사실. **원장에 넣지 않는다** —
//!                            경정이 이미 수익자 금액을 줄였으므로 다시 차감하면 중복이다. 이력에만 남긴다.
//!   추가징수(extra_collect)   새 징수액 발생. 원장에 양수 징수 거래.
//! 처리 예정(planned)은 원장 stage='planned' 로 남아 집계에서 빠지고, 완료(done)되면 stage='actual' 이 된다.
//! 취소는 원장 거래를 void 하고 묶음을 cancelled 로 둔다. 확정된 달은 재개 전에는 어떤 것도 바꾸지 않는다.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::money::format_won as w;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{audit, ledger, recon, year};

pub fn kind_label(k: &str) -> &'static str {
    match k {
        "fund_transfer" => "재원경정",
        "refund" => "수강료 환불",
        "refund_return" => "경정 반환 이행",
        "extra_collect" => "추가징수",
        _ => "알 수 없음",
    }
}

pub fn status_label(s: &str) -> &'static str {
    match s {
        "planned" => "처리 예정",
        "done" => "처리 완료",
        "cancelled" => "취소",
        _ => "알 수 없음",
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

fn none_if_blank(d: &Option<String>) -> Option<String> {
    d.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn fund_name(conn: &Connection, id: Option<i64>) -> AppResult<String> {
    match id {
        None => Ok(String::new()),
        Some(i) => Ok(conn.query_row("SELECT name FROM fund_type WHERE id = ?1", [i], |r| r.get(0)).optional()?.unwrap_or_default()),
    }
}

fn student_label(conn: &Connection, id: Option<i64>) -> AppResult<String> {
    match id {
        None => Ok(String::new()),
        Some(i) => Ok(conn
            .query_row(
                "SELECT grade || '학년 ' || class_name || '반 ' || student_no || '번 ' || name FROM student WHERE id = ?1",
                [i],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default()),
    }
}

/// 프로그램·귀속월·항목의 재원별 징수 기준액 (살아 있고 실제 처리된 거래 + 배분 귀속). 학생이 있으면 그 학생 것만.
fn fund_basis(conn: &Connection, fiscal_month_id: i64, program_id: i64, student_id: Option<i64>, item_id: i64) -> AppResult<Vec<(i64, i64)>> {
    let idx = recon::FundIndex::load(conn)?;
    let set_codes: Vec<String> = {
        let code: String = conn.query_row("SELECT code FROM amount_item WHERE id = ?1", [item_id], |r| r.get(0))?;
        match code.as_str() {
            "textbook" | "material" | "textbook_material" => vec!["textbook".into(), "material".into(), "textbook_material".into()],
            c => vec![c.to_string()],
        }
    };
    let mut out: std::collections::BTreeMap<i64, i64> = std::collections::BTreeMap::new();
    // 직접 귀속 거래 (징수 + 재원경정)
    let rows: Vec<(i64, i64, String)> = {
        let mut st2 = conn.prepare(
            "SELECT e.fund_type_id, e.amount, i.code FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id
              WHERE e.fiscal_month_id = ?1 AND e.program_id = ?2 AND e.state = 'active' AND e.stage = 'actual'
                AND e.flow IN ('collection', 'transfer') AND e.fund_type_id IS NOT NULL
                AND (?3 IS NULL OR e.student_id = ?3)",
        )?;
        let r = st2.query_map(params![fiscal_month_id, program_id, student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        r.collect::<Result<_, _>>()?
    };
    for (f, a, code) in rows {
        if set_codes.contains(&code) {
            *out.entry(idx.effective(f)).or_default() += a;
        }
    }
    // 배분으로 귀속된 미배분 거래
    let mut st = conn.prepare(
        "SELECT e.fund_type_id, a.amount, i.code FROM allocation a
           JOIN ledger_entry e ON e.id = a.entry_id JOIN amount_item i ON i.id = e.amount_item_id
          WHERE a.program_id = ?2 AND a.status = 'active' AND e.state = 'active' AND e.stage = 'actual'
            AND e.fiscal_month_id = ?1 AND e.fund_type_id IS NOT NULL AND (?3 IS NULL OR e.student_id = ?3)",
    )?;
    let rows: Vec<(i64, i64, String)> = st
        .query_map(params![fiscal_month_id, program_id, student_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    for (f, a, code) in rows {
        if set_codes.contains(&code) {
            *out.entry(idx.effective(f)).or_default() += a;
        }
    }
    Ok(out.into_iter().collect())
}

/// 같은 학생·프로그램·귀속월·재원·항목의 처리 예정 환불 합 (환불 가능 금액에서 뺀다).
fn planned_refunds(conn: &Connection, input: &AdjustmentInput) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT coalesce(sum(a.amount), 0) FROM adjustment a
          WHERE a.kind = 'refund' AND a.status = 'planned' AND a.fiscal_month_id = ?1 AND a.program_id = ?2
            AND a.amount_item_id = ?3 AND (?4 IS NULL OR a.student_id = ?4) AND (?5 IS NULL OR a.fund_type_id = ?5)",
        params![input.fiscal_month_id, input.program_id, input.amount_item_id, input.student_id, input.fund_type_id],
        |r| r.get(0),
    )?)
}

fn existing_refunds(conn: &Connection, input: &AdjustmentInput) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT coalesce(-sum(e.amount), 0) FROM ledger_entry e
          WHERE e.fiscal_month_id = ?1 AND e.program_id = ?2 AND e.amount_item_id = ?3 AND e.entry_kind = 'refund'
            AND e.state = 'active' AND (?4 IS NULL OR e.student_id = ?4) AND (?5 IS NULL OR e.fund_type_id = ?5)",
        params![input.fiscal_month_id, input.program_id, input.amount_item_id, input.student_id, input.fund_type_id],
        |r| r.get(0),
    )?)
}

/// 같은 경제적 사건일 수 있는 기존 기록 — 학생·프로그램·귀속월·재원·항목·금액이 같은 살아 있는 거래와 정정.
fn find_duplicates(conn: &Connection, input: &AdjustmentInput) -> AppResult<Vec<String>> {
    let mut out = Vec::new();
    let kind = match input.kind.as_str() {
        "refund" => "refund",
        "extra_collect" => "extra_collect",
        _ => return Ok(out),
    };
    let signed = if kind == "refund" { -input.amount } else { input.amount };
    let mut st = conn.prepare(
        "SELECT e.id, e.processed_on, coalesce(f.file_name, '직접 등록'), coalesce(f.version_no, 0)
           FROM ledger_entry e LEFT JOIN upload_file f ON f.id = e.upload_file_id
          WHERE e.fiscal_month_id = ?1 AND e.program_id = ?2 AND e.amount_item_id = ?3 AND e.entry_kind = ?4 AND e.amount = ?5
            AND e.state = 'active' AND (?6 IS NULL OR e.student_id = ?6) AND (?7 IS NULL OR e.fund_type_id IS NULL OR e.fund_type_id = ?7)",
    )?;
    let rows = st.query_map(
        params![input.fiscal_month_id, input.program_id, input.amount_item_id, kind, signed, input.student_id, input.fund_type_id],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?)),
    )?;
    for row in rows {
        let (id, processed, file, ver) = row?;
        out.push(format!(
            "같은 학생·프로그램·귀속월·항목·금액의 {} 거래 #{id} ({}{}{})",
            kind_label(kind),
            file,
            if ver > 0 { format!(" 제{ver}판") } else { String::new() },
            processed.map(|p| format!(", 처리일 {p}")).unwrap_or_default()
        ));
    }
    Ok(out)
}

/// 등록 전 미리보기 — 경정 전후 재원별 금액, 환불 가능 금액, 중복 가능성, 차단 사유.
pub fn preview(conn: &Connection, input: &AdjustmentInput) -> AppResult<AdjustmentPreview> {
    let mut blocked = Vec::new();
    let mut warnings = Vec::new();
    if !matches!(input.kind.as_str(), "fund_transfer" | "refund" | "refund_return" | "extra_collect") {
        return Err(AppError::invalid("알 수 없는 정정 유형입니다."));
    }
    if !matches!(input.status.as_str(), "planned" | "done") {
        blocked.push("처리 상태는 처리 예정 또는 처리 완료여야 합니다.".into());
    }
    if input.amount <= 0 {
        blocked.push("금액은 0원보다 커야 합니다.".into());
    }
    if input.reason.trim().is_empty() {
        blocked.push("사유를 입력해 주세요.".into());
    }
    check_date("결정일", &input.decided_on)?;
    check_date("처리일", &input.processed_on)?;
    if input.status == "done" && none_if_blank(&input.processed_on).is_none() {
        blocked.push("처리 완료로 등록하려면 실제 처리일이 필요합니다.".into());
    }
    let month_year: Option<i64> = conn.query_row("SELECT school_year_id FROM fiscal_month WHERE id = ?1", [input.fiscal_month_id], |r| r.get(0)).optional()?;
    if month_year != Some(input.school_year_id) {
        blocked.push("귀속월이 학년도에 속하지 않습니다.".into());
    }
    let month_closed = year::month_is_closed(conn, input.fiscal_month_id).unwrap_or(false);
    if month_closed {
        blocked.push("확정된 달입니다. 월별 정산에서 재개한 뒤 등록할 수 있습니다. 확정월을 자동으로 재개하지 않습니다.".into());
    }
    let program_name: Option<String> = conn
        .query_row("SELECT name FROM program WHERE id = ?1 AND school_year_id = ?2", params![input.program_id, input.school_year_id], |r| r.get(0))
        .optional()?;
    let Some(program_name) = program_name else {
        blocked.push("프로그램이 이 학년도에 없습니다.".into());
        return Ok(AdjustmentPreview { kind_label: kind_label(&input.kind).into(), program_name: String::new(), student: String::new(), month_label: String::new(), month_closed, funds: vec![], total_before: 0, total_after: 0, refundable: None, existing_refunds: 0, duplicates: vec![], warnings, blocked });
    };
    if let Some(s) = input.student_id {
        let ok: i64 = conn.query_row("SELECT count(*) FROM student WHERE id = ?1 AND school_year_id = ?2", params![s, input.school_year_id], |r| r.get(0))?;
        if ok == 0 {
            blocked.push("학생이 이 학년도에 없습니다.".into());
        }
    } else {
        warnings.push("학생 단위 근거 없이 프로그램 단위로 등록됩니다 (화면에 '프로그램 단위'로 표시).".into());
    }
    if let Some(e) = input.related_entry_id {
        match ledger::get(conn, e) {
            Ok(en) => {
                if en.state != "active" {
                    blocked.push("관련 원본 거래가 취소되었거나 대체되었습니다.".into());
                }
                if en.fiscal_month_id != input.fiscal_month_id || en.program_id != Some(input.program_id) {
                    blocked.push("관련 원본 거래의 귀속월·프로그램이 다릅니다.".into());
                }
            }
            Err(_) => blocked.push("관련 원본 거래를 찾지 못했습니다.".into()),
        }
    }
    let basis = fund_basis(conn, input.fiscal_month_id, input.program_id, input.student_id, input.amount_item_id)?;
    let total_before: i64 = basis.iter().map(|x| x.1).sum();
    let mut funds: Vec<FundBefore> = Vec::new();
    let mut refundable = None;
    let mut existing = 0;
    let mut duplicates = Vec::new();
    match input.kind.as_str() {
        "fund_transfer" => {
            let (Some(from), Some(to)) = (input.from_fund_id, input.to_fund_id) else {
                blocked.push("변경 전·후 재원을 모두 골라 주세요.".into());
                return Ok(AdjustmentPreview { kind_label: kind_label(&input.kind).into(), program_name, student: student_label(conn, input.student_id)?, month_label: year::month_label_of(conn, input.fiscal_month_id)?, month_closed, funds, total_before, total_after: total_before, refundable, existing_refunds: 0, duplicates, warnings, blocked });
            };
            let idx = recon::FundIndex::load(conn)?;
            if idx.effective(from) == idx.effective(to) {
                blocked.push("변경 전 재원과 변경 후 재원이 같습니다 (통합된 재원도 같은 재원입니다).".into());
            }
            let before_from = basis.iter().find(|x| x.0 == idx.effective(from)).map(|x| x.1).unwrap_or(0);
            let before_to = basis.iter().find(|x| x.0 == idx.effective(to)).map(|x| x.1).unwrap_or(0);
            if before_from - input.amount < 0 {
                blocked.push(format!(
                    "변경 전 재원 '{}' 의 현재 기준액 {} 보다 큰 금액은 경정할 수 없습니다 (음수 잔액). 예외는 별도 절차가 필요합니다.",
                    fund_name(conn, Some(from))?, w(before_from)
                ));
            }
            funds.push(FundBefore { fund_id: from, fund_name: fund_name(conn, Some(from))?, before: before_from, change: -input.amount, after: before_from - input.amount });
            funds.push(FundBefore { fund_id: to, fund_name: fund_name(conn, Some(to))?, before: before_to, change: input.amount, after: before_to + input.amount });
            let dup: i64 = conn.query_row(
                "SELECT count(*) FROM adjustment WHERE kind = 'fund_transfer' AND status <> 'cancelled' AND fiscal_month_id = ?1 AND program_id = ?2
                   AND from_fund_id = ?3 AND to_fund_id = ?4 AND amount = ?5 AND amount_item_id = ?6 AND (?7 IS NULL AND student_id IS NULL OR student_id = ?7)",
                params![input.fiscal_month_id, input.program_id, from, to, input.amount, input.amount_item_id, input.student_id],
                |r| r.get(0),
            )?;
            if dup > 0 {
                blocked.push("같은 학생·프로그램·귀속월·재원·금액의 재원경정이 이미 등록되어 있습니다.".into());
            }
        }
        "refund" | "extra_collect" => {
            let Some(f) = input.fund_type_id else {
                blocked.push("재원을 골라 주세요.".into());
                return Ok(AdjustmentPreview { kind_label: kind_label(&input.kind).into(), program_name, student: student_label(conn, input.student_id)?, month_label: year::month_label_of(conn, input.fiscal_month_id)?, month_closed, funds, total_before, total_after: total_before, refundable, existing_refunds: 0, duplicates, warnings, blocked });
            };
            let idx = recon::FundIndex::load(conn)?;
            let before = basis.iter().find(|x| x.0 == idx.effective(f)).map(|x| x.1).unwrap_or(0);
            let change = if input.kind == "refund" { -input.amount } else { input.amount };
            funds.push(FundBefore { fund_id: f, fund_name: fund_name(conn, Some(f))?, before, change, after: before + change });
            if input.kind == "refund" {
                existing = existing_refunds(conn, input)?;
                let planned = planned_refunds(conn, input)?;
                let cap = before - planned;
                refundable = Some(cap);
                if input.amount > cap {
                    blocked.push(format!(
                        "환불 가능 금액 {} 을(를) 넘습니다 (현재 기준액 {} − 처리 예정 환불 {}). 최초 징수·추가징수·기존 환불·재원경정을 반영한 값입니다.",
                        w(cap), w(before), w(planned)
                    ));
                }
            }
            duplicates = find_duplicates(conn, input)?;
            if !duplicates.is_empty() && !input.confirm_duplicate {
                blocked.push("같은 경제적 사건일 수 있는 기록이 있습니다. 실제로 별도 거래라면 '중복 아님 확인'을 켜고 등록해 주세요.".into());
            }
        }
        "refund_return" => {
            let Some(rid) = input.related_adjustment_id else {
                blocked.push("반환 이행은 근거가 되는 재원경정을 골라야 합니다.".into());
                return Ok(AdjustmentPreview { kind_label: kind_label(&input.kind).into(), program_name, student: student_label(conn, input.student_id)?, month_label: year::month_label_of(conn, input.fiscal_month_id)?, month_closed, funds, total_before, total_after: total_before, refundable, existing_refunds: 0, duplicates, warnings, blocked });
            };
            let t = get(conn, rid)?;
            if t.kind != "fund_transfer" || t.status == "cancelled" {
                blocked.push("근거 재원경정이 없거나 취소되었습니다.".into());
            }
            if t.program_id != input.program_id || t.fiscal_month_id != input.fiscal_month_id {
                blocked.push("근거 재원경정의 프로그램·귀속월이 다릅니다.".into());
            }
            let returned: i64 = conn.query_row(
                "SELECT coalesce(sum(amount), 0) FROM adjustment WHERE kind = 'refund_return' AND status <> 'cancelled' AND related_adjustment_id = ?1",
                [rid],
                |r| r.get(0),
            )?;
            if returned + input.amount > t.amount {
                blocked.push(format!("반환 이행 누계({} + {})가 경정 금액 {} 을(를) 넘습니다.", w(returned), w(input.amount), w(t.amount)));
            }
            warnings.push("반환 이행은 원장 징수액을 다시 차감하지 않습니다 (재원경정이 이미 수익자 금액을 줄였습니다). 이력에만 남습니다.".into());
        }
        _ => {}
    }
    let total_after: i64 = total_before + funds.iter().map(|f| f.change).sum::<i64>();
    Ok(AdjustmentPreview {
        kind_label: kind_label(&input.kind).into(),
        program_name,
        student: student_label(conn, input.student_id)?,
        month_label: year::month_label_of(conn, input.fiscal_month_id)?,
        month_closed,
        funds,
        total_before,
        total_after,
        refundable,
        existing_refunds: existing,
        duplicates,
        warnings,
        blocked,
    })
}

/// 등록 — 한 트랜잭션. 미리보기가 막으면 아무것도 쓰지 않는다.
pub fn create(tx: &Connection, input: &AdjustmentInput) -> AppResult<Adjustment> {
    let p = preview(tx, input)?;
    if !p.blocked.is_empty() {
        return Err(AppError::invalid(p.blocked.join(" / ")));
    }
    let stage = if input.status == "done" { "actual" } else { "planned" };
    let processed = none_if_blank(&input.processed_on);
    let scope = if input.student_id.is_some() { "student" } else { "program" };
    let base = NewEntry {
        school_year_id: input.school_year_id,
        fiscal_month_id: input.fiscal_month_id,
        program_id: Some(input.program_id),
        program_group_id: None,
        student_id: input.student_id,
        fund_type_id: input.fund_type_id,
        amount_item_id: input.amount_item_id,
        flow: String::new(),
        entry_kind: String::new(),
        amount: 0,
        stage: Some(stage.into()),
        processed_on: processed.clone(),
        counterparty: None,
        upload_file_id: None,
        source_row_id: None,
        reason: Some(input.reason.trim().to_string()),
    };
    let mut entry_ids: Vec<i64> = Vec::new();
    let mut transfer_group: Option<i64> = None;
    match input.kind.as_str() {
        "fund_transfer" => {
            let from = NewEntry { fund_type_id: input.from_fund_id, flow: "transfer".into(), entry_kind: "fund_transfer".into(), amount: -input.amount, ..base.clone() };
            let to = NewEntry { fund_type_id: input.to_fund_id, flow: "transfer".into(), entry_kind: "fund_transfer".into(), amount: input.amount, ..base.clone() };
            let (a, b) = ledger::insert_transfer(tx, &from, &to)?;
            transfer_group = a.transfer_group_id;
            entry_ids.push(a.id);
            entry_ids.push(b.id);
        }
        "refund" => {
            let e = ledger::insert(tx, &NewEntry { flow: "collection".into(), entry_kind: "refund".into(), amount: -input.amount, ..base.clone() })?;
            entry_ids.push(e.id);
        }
        "extra_collect" => {
            let e = ledger::insert(tx, &NewEntry { flow: "collection".into(), entry_kind: "extra_collect".into(), amount: input.amount, ..base.clone() })?;
            entry_ids.push(e.id);
        }
        "refund_return" => {} // 원장에 넣지 않는다
        _ => unreachable!(),
    }
    tx.execute(
        "INSERT INTO adjustment (school_year_id, fiscal_month_id, kind, scope, program_id, student_id, from_fund_id, to_fund_id, fund_type_id,
                                 amount_item_id, amount, decided_on, processed_on, status, reason, related_entry_id, related_adjustment_id, transfer_group_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            input.school_year_id, input.fiscal_month_id, input.kind, scope, input.program_id, input.student_id,
            if input.kind == "fund_transfer" { input.from_fund_id } else { None },
            if input.kind == "fund_transfer" { input.to_fund_id } else { None },
            if input.kind == "fund_transfer" { None } else { input.fund_type_id },
            input.amount_item_id, input.amount, none_if_blank(&input.decided_on), processed, input.status, input.reason.trim(),
            input.related_entry_id, input.related_adjustment_id, transfer_group
        ],
    )?;
    let id = tx.last_insert_rowid();
    for e in &entry_ids {
        tx.execute("INSERT INTO adjustment_entry (adjustment_id, entry_id) VALUES (?1, ?2)", params![id, e])?;
    }
    tx.execute(
        "INSERT INTO adjustment_status_log (adjustment_id, from_status, to_status, note) VALUES (?1, '', ?2, '등록')",
        params![id, input.status],
    )?;
    // 검산: 재원경정 쌍의 합은 0, 거래 수는 유형대로
    if input.kind == "fund_transfer" {
        let sum: i64 = tx.query_row("SELECT coalesce(sum(amount), 0) FROM ledger_entry WHERE transfer_group_id = ?1 AND state = 'active'", [transfer_group], |r| r.get(0))?;
        if sum != 0 || entry_ids.len() != 2 {
            return Err(AppError::new("VERIFY", "재원경정 쌍의 합이 0 이 아니어서 등록을 되돌렸습니다."));
        }
    }
    audit::write(
        tx,
        &format!("adjust.{}", input.kind),
        "adjustment",
        Some(id),
        &format!("{} 등록 ({}, {}건 거래)", kind_label(&input.kind), status_label(&input.status), entry_ids.len()),
        serde_json::json!({ "programId": input.program_id, "fiscalMonthId": input.fiscal_month_id }),
    )?;
    get(tx, id)
}

fn guard_month(conn: &Connection, a: &Adjustment) -> AppResult<()> {
    if year::month_is_closed(conn, a.fiscal_month_id)? {
        return Err(AppError::invalid(format!("{} 은(는) 확정된 달입니다. 월별 정산에서 재개한 뒤 바꿀 수 있습니다.", a.month_label)));
    }
    Ok(())
}

/// 처리 예정 → 처리 완료 (원장 stage 를 actual 로). 반대 방향은 허용하지 않는다 — 취소 후 다시 등록한다.
pub fn set_status(tx: &Connection, id: i64, status: &str, processed_on: Option<&str>, note: &str) -> AppResult<Adjustment> {
    let a = get(tx, id)?;
    guard_month(tx, &a)?;
    if a.status == "cancelled" {
        return Err(AppError::invalid("취소된 정정의 상태는 바꿀 수 없습니다."));
    }
    if status != "done" {
        return Err(AppError::invalid("처리 예정으로 되돌릴 수 없습니다. 취소한 뒤 다시 등록해 주세요."));
    }
    if a.status == "done" {
        return Err(AppError::invalid("이미 처리 완료입니다."));
    }
    let p = processed_on.map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).or(a.processed_on.clone());
    let Some(p) = p else { return Err(AppError::invalid("처리 완료에는 실제 처리일이 필요합니다.")) };
    check_date("처리일", &Some(p.clone()))?;
    if a.kind == "refund_return" {
        // 원장 거래 없음
    } else {
        for e in &a.entry_ids {
            tx.execute("UPDATE ledger_entry SET stage = 'actual', processed_on = ?2 WHERE id = ?1 AND state = 'active'", params![e, p])?;
        }
    }
    tx.execute("UPDATE adjustment SET status = 'done', processed_on = ?2 WHERE id = ?1", params![id, p])?;
    tx.execute("INSERT INTO adjustment_status_log (adjustment_id, from_status, to_status, note) VALUES (?1, ?2, 'done', ?3)", params![id, a.status, note.trim()])?;
    audit::write(tx, "adjust.done", "adjustment", Some(id), &format!("{} 처리 완료 ({p})", a.kind_label), serde_json::json!({}))?;
    get(tx, id)
}

/// 취소 — 원장 거래를 void(쌍은 함께), 묶음은 cancelled. 사유 필수. 연결된 반환 이행이 살아 있으면 막는다.
pub fn cancel(tx: &Connection, id: i64, reason: &str) -> AppResult<Adjustment> {
    let a = get(tx, id)?;
    guard_month(tx, &a)?;
    let r = reason.trim();
    if r.is_empty() {
        return Err(AppError::invalid("취소 사유를 입력해 주세요."));
    }
    if a.status == "cancelled" {
        return Err(AppError::invalid("이미 취소된 정정입니다."));
    }
    if a.kind == "fund_transfer" {
        let dependents: i64 = tx.query_row(
            "SELECT count(*) FROM adjustment WHERE related_adjustment_id = ?1 AND status <> 'cancelled'",
            [id],
            |r| r.get(0),
        )?;
        if dependents > 0 {
            return Err(AppError::invalid("이 재원경정에 연결된 반환 이행이 살아 있습니다. 반환 이행을 먼저 취소해 주세요."));
        }
    }
    let category = if r.contains("회계 처리 취소") { "accounting_error" } else { "input_error" };
    // void 는 쌍을 함께 접으므로 첫 거래만 넘겨도 된다. 이미 접힌 거래는 건너뛴다.
    for e in &a.entry_ids {
        let state: String = tx.query_row("SELECT state FROM ledger_entry WHERE id = ?1", [e], |r| r.get(0))?;
        if state == "active" {
            ledger::void(tx, *e, category, r)?;
        }
    }
    tx.execute(
        "UPDATE adjustment SET status = 'cancelled', cancelled_at = datetime('now', 'localtime'), cancel_reason = ?2 WHERE id = ?1",
        params![id, r],
    )?;
    tx.execute("INSERT INTO adjustment_status_log (adjustment_id, from_status, to_status, note) VALUES (?1, ?2, 'cancelled', ?3)", params![id, a.status, r])?;
    audit::write(tx, "adjust.cancel", "adjustment", Some(id), &format!("{} 취소: {r}", a.kind_label), serde_json::json!({}))?;
    get(tx, id)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Adjustment> {
    let f = AdjustmentFilter { school_year_id: 0, ..Default::default() };
    list_where(conn, &f, Some(id))?.into_iter().next().ok_or_else(|| AppError::not_found("정정을 찾지 못했습니다."))
}

pub fn list(conn: &Connection, f: &AdjustmentFilter) -> AppResult<Vec<Adjustment>> {
    list_where(conn, f, None)
}

fn list_where(conn: &Connection, f: &AdjustmentFilter, only_id: Option<i64>) -> AppResult<Vec<Adjustment>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.school_year_id, a.fiscal_month_id, m.ym, a.kind, a.scope, a.program_id, p.name, a.student_id,
                coalesce(s.grade || '학년 ' || s.class_name || '반 ' || s.student_no || '번 ' || s.name, ''),
                coalesce(ff.name, ''), coalesce(ft.name, ''), coalesce(fx.name, ''), i.name, a.amount, a.decided_on, a.processed_on,
                a.status, a.reason, a.related_entry_id, coalesce(re.state, ''), a.related_adjustment_id, a.created_at, a.cancelled_at, a.cancel_reason
           FROM adjustment a
           JOIN fiscal_month m ON m.id = a.fiscal_month_id
           JOIN program p ON p.id = a.program_id
           LEFT JOIN student s ON s.id = a.student_id
           LEFT JOIN fund_type ff ON ff.id = a.from_fund_id
           LEFT JOIN fund_type ft ON ft.id = a.to_fund_id
           LEFT JOIN fund_type fx ON fx.id = a.fund_type_id
           JOIN amount_item i ON i.id = a.amount_item_id
           LEFT JOIN ledger_entry re ON re.id = a.related_entry_id
          WHERE (?1 IS NULL OR a.id = ?1)
            AND (?1 IS NOT NULL OR a.school_year_id = ?2)
            AND (?3 IS NULL OR a.fiscal_month_id = ?3)
            AND (?4 IS NULL OR a.processed_on >= ?4)
            AND (?5 IS NULL OR a.processed_on <= ?5)
            AND (?6 IS NULL OR a.program_id = ?6)
            AND (?7 IS NULL OR ?7 = '' OR coalesce(s.name, '') LIKE '%' || ?7 || '%')
            AND (?8 IS NULL OR a.from_fund_id = ?8 OR a.to_fund_id = ?8 OR a.fund_type_id = ?8)
            AND (?9 IS NULL OR ?9 = '' OR a.kind = ?9)
            AND (?10 IS NULL OR ?10 = '' OR a.status = ?10)
          ORDER BY a.id DESC",
    )?;
    let rows = st.query_map(
        params![
            only_id, f.school_year_id, f.fiscal_month_id, f.processed_from, f.processed_to, f.program_id,
            f.student_text, f.fund_type_id, f.kind, f.status
        ],
        |r| {
            let ym: String = r.get(3)?;
            let kind: String = r.get(4)?;
            let status: String = r.get(17)?;
            let related_state: String = r.get(20)?;
            Ok(Adjustment {
                id: r.get(0)?,
                school_year_id: r.get(1)?,
                fiscal_month_id: r.get(2)?,
                month_label: format!("{} ({ym})", crate::domain::months::label(&ym)),
                kind_label: kind_label(&kind).into(),
                kind,
                scope: r.get(5)?,
                program_id: r.get(6)?,
                program_name: r.get(7)?,
                student_id: r.get(8)?,
                student: r.get(9)?,
                from_fund_name: r.get(10)?,
                to_fund_name: r.get(11)?,
                fund_name: r.get(12)?,
                item_name: r.get(13)?,
                amount: r.get(14)?,
                decided_on: r.get(15)?,
                processed_on: r.get(16)?,
                status_label: status_label(&status).into(),
                orphan: status != "cancelled" && !related_state.is_empty() && related_state != "active",
                status,
                reason: r.get(18)?,
                related_entry_id: r.get(19)?,
                related_entry_state: related_state,
                related_adjustment_id: r.get(21)?,
                entry_ids: Vec::new(),
                created_at: r.get(22)?,
                cancelled_at: r.get(23)?,
                cancel_reason: r.get(24)?,
                logs: Vec::new(),
            })
        },
    )?;
    let mut out: Vec<Adjustment> = rows.collect::<Result<_, _>>()?;
    let mut st_e = conn.prepare("SELECT entry_id FROM adjustment_entry WHERE adjustment_id = ?1 ORDER BY entry_id")?;
    let mut st_l = conn.prepare("SELECT from_status, to_status, note, at FROM adjustment_status_log WHERE adjustment_id = ?1 ORDER BY id")?;
    for a in out.iter_mut() {
        a.entry_ids = st_e.query_map([a.id], |r| r.get::<_, i64>(0))?.collect::<Result<_, _>>()?;
        a.logs = st_l
            .query_map([a.id], |r| Ok(AdjustmentLog { from_status: r.get(0)?, to_status: r.get(1)?, note: r.get(2)?, at: r.get(3)? }))?
            .collect::<Result<_, _>>()?;
    }
    Ok(out)
}

/// 원본 거래가 취소된 정정이 만든 살아 있는 원장 거래 id — 대조 엔진이 '원본 재확인 필요' 로 쓴다.
pub fn orphan_entry_ids(conn: &Connection, fiscal_month_id: i64) -> AppResult<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT ae.entry_id FROM adjustment a
           JOIN adjustment_entry ae ON ae.adjustment_id = a.id
           JOIN ledger_entry re ON re.id = a.related_entry_id
          WHERE a.fiscal_month_id = ?1 AND a.status <> 'cancelled' AND re.state <> 'active'",
    )?;
    let ids: Vec<i64> = st.query_map([fiscal_month_id], |r| r.get::<_, i64>(0))?.collect::<Result<_, _>>()?;
    Ok(ids)
}

pub fn students(conn: &Connection, school_year_id: i64) -> AppResult<Vec<StudentRow>> {
    let mut st = conn.prepare(
        "SELECT id, grade || '학년 ' || class_name || '반 ' || student_no || '번 ' || name FROM student
          WHERE school_year_id = ?1 ORDER BY grade, class_name, student_no, name",
    )?;
    let rows: Vec<StudentRow> = st.query_map([school_year_id], |r| Ok(StudentRow { id: r.get(0)?, label: r.get(1)? }))?.collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn entry_picks(conn: &Connection, fiscal_month_id: i64, program_id: i64, student_id: Option<i64>) -> AppResult<Vec<EntryPick>> {
    let mut st = conn.prepare(
        "SELECT e.id, e.entry_kind, coalesce(f.name, '미확정'), i.name, e.amount, e.stage, coalesce(u.file_name, '직접 등록')
           FROM ledger_entry e LEFT JOIN fund_type f ON f.id = e.fund_type_id JOIN amount_item i ON i.id = e.amount_item_id
           LEFT JOIN upload_file u ON u.id = e.upload_file_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active' AND e.flow = 'collection'
            AND (e.program_id = ?2 OR (e.program_id IS NULL AND EXISTS (SELECT 1 FROM allocation a WHERE a.entry_id = e.id AND a.program_id = ?2 AND a.status = 'active')))
            AND (?3 IS NULL OR e.student_id = ?3)
          ORDER BY e.id",
    )?;
    let rows = st.query_map(params![fiscal_month_id, program_id, student_id], |r| {
        let kind: String = r.get(1)?;
        Ok(EntryPick {
            id: r.get(0)?,
            kind_label: match kind.as_str() { "initial_collect" => "최초 징수", "extra_collect" => "추가징수", "refund" => "환불", _ => "거래" }.into(),
            fund_name: r.get(2)?,
            item_name: r.get(3)?,
            amount: r.get(4)?,
            stage: r.get(5)?,
            file_name: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
#[path = "adjustment_tests.rs"]
mod tests;
