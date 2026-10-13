//! 회계 거래 원장. **덧붙이기만 한다.**
//!
//! - 틀린 거래는 `void`(취소) 또는 `replace`(대체)로 처리하고 `ledger_correction` 에 연결을 남긴다.
//! - 재원 간 이동(과목경정)은 반드시 쌍으로 넣고 합이 0 이어야 한다.
//! - 월별 합계는 저장하지 않고 `month_summary` 가 매번 다시 계산한다.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::domain::money;
use crate::error::{AppError, AppResult};
use crate::model::{Correction, LedgerEntry, MonthSummary, MonthSummaryRow, NewEntry};
use crate::repo::audit;

pub const FLOWS: &[&str] = &["collection", "expenditure", "transfer"];
pub const KINDS: &[&str] = &[
    "initial_collect",
    "extra_collect",
    "refund",
    "fund_transfer",
    "instructor_pay",
    "vendor_pay",
    "adjustment",
];

const COLS: &str = "id, school_year_id, fiscal_month_id, program_id, student_id, fund_type_id, amount_item_id,
                    flow, entry_kind, amount, stage, state, processed_on, counterparty, transfer_group_id,
                    upload_file_id, source_row_id, supersedes_id, reason, created_at, program_group_id";

fn map(r: &Row) -> rusqlite::Result<LedgerEntry> {
    Ok(LedgerEntry {
        id: r.get(0)?,
        school_year_id: r.get(1)?,
        fiscal_month_id: r.get(2)?,
        program_id: r.get(3)?,
        student_id: r.get(4)?,
        fund_type_id: r.get(5)?,
        amount_item_id: r.get(6)?,
        flow: r.get(7)?,
        entry_kind: r.get(8)?,
        amount: r.get(9)?,
        stage: r.get(10)?,
        state: r.get(11)?,
        processed_on: r.get(12)?,
        counterparty: r.get(13)?,
        transfer_group_id: r.get(14)?,
        upload_file_id: r.get(15)?,
        source_row_id: r.get(16)?,
        supersedes_id: r.get(17)?,
        reason: r.get(18)?,
        created_at: r.get(19)?,
        program_group_id: r.get(20)?,
    })
}

pub fn get(conn: &Connection, id: i64) -> AppResult<LedgerEntry> {
    conn.query_row(&format!("SELECT {COLS} FROM ledger_entry WHERE id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("거래를 찾지 못했습니다."))
}

fn validate(conn: &Connection, e: &NewEntry) -> AppResult<()> {
    if !FLOWS.contains(&e.flow.as_str()) {
        return Err(AppError::invalid(format!("알 수 없는 거래 흐름: {}", e.flow)));
    }
    if !KINDS.contains(&e.entry_kind.as_str()) {
        return Err(AppError::invalid(format!("알 수 없는 거래 유형: {}", e.entry_kind)));
    }
    if (e.flow == "transfer") != (e.entry_kind == "fund_transfer") {
        return Err(AppError::invalid("재원 이동은 흐름 'transfer' 와 유형 'fund_transfer' 를 함께 써야 합니다."));
    }
    if e.entry_kind == "refund" && e.amount > 0 {
        return Err(AppError::invalid("환불은 음수 금액으로 등록합니다."));
    }
    if matches!(e.entry_kind.as_str(), "initial_collect" | "extra_collect") && e.amount < 0 {
        return Err(AppError::invalid("징수는 양수 금액으로 등록합니다."));
    }
    if let Some(s) = &e.stage {
        if s != "planned" && s != "actual" {
            return Err(AppError::invalid("단계는 planned 또는 actual 이어야 합니다."));
        }
    }
    if e.program_id.is_some() && e.program_group_id.is_some() {
        return Err(AppError::invalid("프로그램과 프로그램 그룹을 동시에 둘 수 없습니다."));
    }
    if let Some(d) = &e.processed_on {
        if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
            return Err(AppError::invalid("처리일은 YYYY-MM-DD 형식이어야 합니다."));
        }
    }
    // 귀속월이 같은 학년도에 속하는지.
    let month_year: Option<i64> = conn
        .query_row(
            "SELECT school_year_id FROM fiscal_month WHERE id = ?1",
            [e.fiscal_month_id],
            |r| r.get(0),
        )
        .optional()?;
    match month_year {
        Some(y) if y == e.school_year_id => {}
        Some(_) => return Err(AppError::invalid("귀속월이 다른 학년도에 속해 있습니다.")),
        None => return Err(AppError::not_found("귀속월을 찾지 못했습니다.")),
    }
    Ok(())
}

fn insert_raw(
    conn: &Connection,
    e: &NewEntry,
    transfer_group_id: Option<i64>,
    supersedes_id: Option<i64>,
) -> AppResult<i64> {
    conn.execute(
        "INSERT INTO ledger_entry
           (school_year_id, fiscal_month_id, program_id, student_id, fund_type_id, amount_item_id,
            flow, entry_kind, amount, stage, processed_on, counterparty, transfer_group_id,
            upload_file_id, source_row_id, supersedes_id, reason, program_group_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            e.school_year_id,
            e.fiscal_month_id,
            e.program_id,
            e.student_id,
            e.fund_type_id,
            e.amount_item_id,
            e.flow,
            e.entry_kind,
            e.amount,
            e.stage.clone().unwrap_or_else(|| "actual".into()),
            e.processed_on,
            e.counterparty.clone().unwrap_or_default(),
            transfer_group_id,
            e.upload_file_id,
            e.source_row_id,
            supersedes_id,
            e.reason.clone().unwrap_or_default(),
            e.program_group_id,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 일반 거래 하나 등록. 재원 이동은 `insert_transfer` 를 쓴다.
pub fn insert(conn: &Connection, e: &NewEntry) -> AppResult<LedgerEntry> {
    validate(conn, e)?;
    if e.flow == "transfer" {
        return Err(AppError::invalid("재원 이동은 쌍으로 등록해야 합니다 (insert_transfer)."));
    }
    let id = insert_raw(conn, e, None, None)?;
    get(conn, id)
}

/// 재원 간 이동(과목경정). 두 줄의 합이 0 이어야 하고, 같은 월·프로그램·항목이어야 한다.
pub fn insert_transfer(conn: &Connection, from: &NewEntry, to: &NewEntry) -> AppResult<(LedgerEntry, LedgerEntry)> {
    validate(conn, from)?;
    validate(conn, to)?;
    if from.flow != "transfer" || to.flow != "transfer" {
        return Err(AppError::invalid("재원 이동 거래는 흐름이 'transfer' 여야 합니다."));
    }
    if money::sum([from.amount, to.amount])? != 0 {
        return Err(AppError::invalid("재원 이동 두 줄의 합은 0 이어야 합니다."));
    }
    if from.amount == 0 {
        return Err(AppError::invalid("재원 이동 금액은 0 일 수 없습니다."));
    }
    if from.fund_type_id == to.fund_type_id {
        return Err(AppError::invalid("같은 재원 사이의 이동은 뜻이 없습니다."));
    }
    if from.fiscal_month_id != to.fiscal_month_id
        || from.program_id != to.program_id
        || from.amount_item_id != to.amount_item_id
    {
        return Err(AppError::invalid("재원 이동 두 줄은 같은 귀속월·프로그램·항목이어야 합니다."));
    }
    let group: i64 = conn.query_row(
        "SELECT coalesce(max(transfer_group_id), 0) + 1 FROM ledger_entry",
        [],
        |r| r.get(0),
    )?;
    let a = insert_raw(conn, from, Some(group), None)?;
    let b = insert_raw(conn, to, Some(group), None)?;
    Ok((get(conn, a)?, get(conn, b)?))
}

fn check_category(category: &str) -> AppResult<()> {
    if category != "input_error" && category != "accounting_error" {
        return Err(AppError::invalid("정정 구분은 input_error 또는 accounting_error 여야 합니다."));
    }
    Ok(())
}

fn check_reason(reason: &str) -> AppResult<String> {
    let r = reason.trim();
    if r.is_empty() {
        return Err(AppError::invalid("정정 사유를 입력해 주세요."));
    }
    Ok(r.to_string())
}

fn require_active(conn: &Connection, id: i64) -> AppResult<LedgerEntry> {
    let e = get(conn, id)?;
    if e.state != "active" {
        return Err(AppError::invalid("이미 취소되었거나 대체된 거래입니다."));
    }
    Ok(e)
}

/// 거래 취소. 행은 남고 state 만 'voided' 가 되며 정정 이력이 생긴다.
/// 재원 이동은 쌍 전체가 함께 취소된다.
pub fn void(conn: &Connection, id: i64, category: &str, reason: &str) -> AppResult<Correction> {
    check_category(category)?;
    let reason = check_reason(reason)?;
    let e = require_active(conn, id)?;
    let targets: Vec<i64> = match e.transfer_group_id {
        Some(g) => {
            let mut st = conn.prepare(
                "SELECT id FROM ledger_entry WHERE transfer_group_id = ?1 AND state = 'active'",
            )?;
            let rows = st.query_map([g], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        }
        None => vec![id],
    };
    let mut first: Option<i64> = None;
    for t in &targets {
        conn.execute("UPDATE ledger_entry SET state = 'voided' WHERE id = ?1", [t])?;
        conn.execute(
            "INSERT INTO ledger_correction (entry_id, action, category, reason, completion)
             VALUES (?1, 'void', ?2, ?3, ?4)",
            params![t, category, reason, completion_for(category)],
        )?;
        first.get_or_insert(conn.last_insert_rowid());
    }
    audit::write(
        conn,
        "ledger.void",
        "ledger_entry",
        Some(id),
        &format!("거래 {}건 취소 ({})", targets.len(), category_label(category)),
        serde_json::json!({ "entryIds": targets }),
    )?;
    correction_get(conn, first.unwrap_or(0))
}

/// 거래 대체. 원 거래는 'superseded', 새 거래는 `supersedes_id` 로 원 거래를 가리킨다.
pub fn replace(
    conn: &Connection,
    id: i64,
    replacement: &NewEntry,
    category: &str,
    reason: &str,
) -> AppResult<(Correction, LedgerEntry)> {
    check_category(category)?;
    let reason = check_reason(reason)?;
    let old = require_active(conn, id)?;
    if old.transfer_group_id.is_some() {
        return Err(AppError::invalid("재원 이동은 대체할 수 없습니다. 취소한 뒤 새로 등록해 주세요."));
    }
    validate(conn, replacement)?;
    if replacement.flow == "transfer" {
        return Err(AppError::invalid("대체 거래로 재원 이동을 넣을 수 없습니다."));
    }
    conn.execute("UPDATE ledger_entry SET state = 'superseded' WHERE id = ?1", [id])?;
    let new_id = insert_raw(conn, replacement, None, Some(id))?;
    conn.execute(
        "INSERT INTO ledger_correction (entry_id, replacement_entry_id, action, category, reason, completion)
         VALUES (?1, ?2, 'replace', ?3, ?4, ?5)",
        params![id, new_id, category, reason, completion_for(category)],
    )?;
    let corr_id = conn.last_insert_rowid();
    audit::write(
        conn,
        "ledger.replace",
        "ledger_entry",
        Some(id),
        &format!("거래 대체 ({})", category_label(category)),
        serde_json::json!({ "replacementEntryId": new_id }),
    )?;
    Ok((correction_get(conn, corr_id)?, get(conn, new_id)?))
}

/// 회계 처리 오류(B)의 후속 처리 완료 표시.
pub fn correction_complete(conn: &Connection, correction_id: i64, completed_on: &str) -> AppResult<Correction> {
    let c = correction_get(conn, correction_id)?;
    if c.category != "accounting_error" {
        return Err(AppError::invalid("입력 오류 정정에는 후속 처리 완료가 없습니다."));
    }
    if chrono::NaiveDate::parse_from_str(completed_on, "%Y-%m-%d").is_err() {
        return Err(AppError::invalid("처리 완료일은 YYYY-MM-DD 형식이어야 합니다."));
    }
    conn.execute(
        "UPDATE ledger_correction SET completion = 'done', completed_on = ?2 WHERE id = ?1",
        params![correction_id, completed_on],
    )?;
    correction_get(conn, correction_id)
}

fn completion_for(category: &str) -> &'static str {
    if category == "accounting_error" {
        "pending"
    } else {
        "none"
    }
}

fn category_label(category: &str) -> &'static str {
    if category == "accounting_error" {
        "회계 처리 오류"
    } else {
        "입력 오류"
    }
}

pub fn correction_get(conn: &Connection, id: i64) -> AppResult<Correction> {
    conn.query_row(
        "SELECT id, entry_id, replacement_entry_id, action, category, reason, completion, completed_on, created_at
           FROM ledger_correction WHERE id = ?1",
        [id],
        |r| {
            Ok(Correction {
                id: r.get(0)?,
                entry_id: r.get(1)?,
                replacement_entry_id: r.get(2)?,
                action: r.get(3)?,
                category: r.get(4)?,
                reason: r.get(5)?,
                completion: r.get(6)?,
                completed_on: r.get(7)?,
                created_at: r.get(8)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("정정 이력을 찾지 못했습니다."))
}

/// 거래 하나의 정정 사슬 — 원 거래부터 마지막 대체 거래까지.
pub fn chain(conn: &Connection, id: i64) -> AppResult<Vec<LedgerEntry>> {
    let mut cur = get(conn, id)?;
    // 뿌리까지 거슬러 올라간다.
    while let Some(p) = cur.supersedes_id {
        cur = get(conn, p)?;
    }
    let mut out = vec![cur.clone()];
    loop {
        let next: Option<i64> = conn
            .query_row(
                "SELECT id FROM ledger_entry WHERE supersedes_id = ?1",
                [cur.id],
                |r| r.get(0),
            )
            .optional()?;
        match next {
            Some(n) => {
                cur = get(conn, n)?;
                out.push(cur.clone());
            }
            None => break,
        }
    }
    Ok(out)
}

/// 월별 합계를 원본 거래에서 다시 계산한다. 살아 있는(active) 거래만 센다.
pub fn month_summary(conn: &Connection, fiscal_month_id: i64) -> AppResult<MonthSummary> {
    let mut st = conn.prepare(
        "SELECT e.program_id, coalesce(p.name, g.name || ' (그룹)', '미배분'),
                ef.id, coalesce(ef.name, '미확정'), e.amount_item_id, i.name,
                e.flow, e.stage, count(*), sum(e.amount)
           FROM ledger_entry e
           LEFT JOIN program p ON p.id = e.program_id
           LEFT JOIN program_group g ON g.id = e.program_group_id
           LEFT JOIN fund_type f0 ON f0.id = e.fund_type_id
           LEFT JOIN fund_type ef ON ef.id = coalesce(f0.merged_into_id, f0.id)
           JOIN amount_item i ON i.id = e.amount_item_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active'
          GROUP BY e.program_id, e.program_group_id, ef.id, e.amount_item_id, e.flow, e.stage
          ORDER BY (p.name IS NULL AND g.name IS NULL), coalesce(p.name, g.name), ef.sort_order, i.sort_order, e.flow, e.stage",
    )?;
    let rows = st.query_map([fiscal_month_id], |r| {
        Ok(MonthSummaryRow {
            program_id: r.get(0)?,
            program_name: r.get(1)?,
            fund_type_id: r.get(2)?,
            fund_type_name: r.get(3)?,
            amount_item_id: r.get(4)?,
            amount_item_name: r.get(5)?,
            flow: r.get(6)?,
            stage: r.get(7)?,
            entry_count: r.get(8)?,
            total: r.get(9)?,
        })
    })?;
    let rows: Vec<MonthSummaryRow> = rows.collect::<Result<_, _>>()?;
    let pick = |flow: &str| money::sum(rows.iter().filter(|r| r.flow == flow).map(|r| r.total));
    Ok(MonthSummary {
        fiscal_month_id,
        collection_total: pick("collection")?,
        expenditure_total: pick("expenditure")?,
        transfer_balance: pick("transfer")?,
        rows,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::testutil::fixture;

    #[test]
    fn amounts_are_stored_as_integers_only() {
        let f = fixture();
        // 6. 금액 정수 저장 — 실수를 직접 넣으면 CHECK 가 막는다.
        let err = f
            .db
            .write(|tx| {
                tx.execute(
                    "INSERT INTO ledger_entry (school_year_id, fiscal_month_id, program_id, fund_type_id, amount_item_id,
                                               flow, entry_kind, amount)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'collection', 'initial_collect', 3.5)",
                    params![f.year_id, f.months[0], f.program_coding, f.fund_beneficiary, f.item_instructor],
                )?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "INVALID", "{err:?}");

        // 큰 정수도 정확히 저장된다 (i64).
        let big = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 9_000_000_000))).unwrap();
        assert_eq!(big.amount, 9_000_000_000);
        let typ: String = f
            .db
            .read(|c| Ok(c.query_row("SELECT typeof(amount) FROM ledger_entry WHERE id = ?1", [big.id], |r| r.get(0))?))
            .unwrap();
        assert_eq!(typ, "integer");
    }

    #[test]
    fn month_totals_are_recomputed_from_entries() {
        let f = fixture();
        f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        f.db.write(|tx| insert(tx, &f.entry(1, "collection", "refund", -24_900))).unwrap();
        let mut pay = f.entry(1, "expenditure", "instructor_pay", 995_000);
        pay.counterparty = Some("가명강사".into());
        f.db.write(|tx| insert(tx, &pay)).unwrap();
        // 다른 달의 거래는 섞이지 않는다.
        f.db.write(|tx| insert(tx, &f.entry(2, "collection", "initial_collect", 1_000))).unwrap();

        let s = f.db.read(|c| month_summary(c, f.months[0])).unwrap();
        assert_eq!(s.collection_total, 115_100);
        assert_eq!(s.expenditure_total, 995_000);
        assert_eq!(s.transfer_balance, 0);
        assert_eq!(s.rows.len(), 2, "프로그램×재원×항목×흐름×단계로 묶인다");

        let months = f.db.read(|c| crate::repo::year::months(c, f.year_id)).unwrap();
        assert_eq!(months[0].entry_count, 4);
        assert_eq!(months[0].collection_total, 115_100);
        assert_eq!(months[1].collection_total, 1_000);
    }

    #[test]
    fn void_keeps_row_and_links_correction() {
        let f = fixture();
        let e = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        let c = f.db.write(|tx| void(tx, e.id, "input_error", "업로드 열 어긋남")).unwrap();
        assert_eq!(c.entry_id, e.id);
        assert_eq!(c.action, "void");
        assert_eq!(c.completion, "none");
        let after = f.db.read(|c| get(c, e.id)).unwrap();
        assert_eq!(after.state, "voided");
        assert_eq!(after.amount, 70_000, "원본 금액은 그대로 남는다");
        let s = f.db.read(|c| month_summary(c, f.months[0])).unwrap();
        assert_eq!(s.collection_total, 0, "합계에서는 빠진다");
        // 두 번 취소할 수 없다.
        assert_eq!(f.db.write(|tx| void(tx, e.id, "input_error", "다시")).unwrap_err().code, "INVALID");
        // 사유 없이는 안 된다.
        let e2 = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 1))).unwrap();
        assert_eq!(f.db.write(|tx| void(tx, e2.id, "input_error", "  ")).unwrap_err().code, "INVALID");
    }

    #[test]
    fn replace_links_original_and_replacement_both_ways() {
        let f = fixture();
        let e = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        let fixed = f.entry(1, "collection", "initial_collect", 77_000);
        let (c, new) = f.db.write(|tx| replace(tx, e.id, &fixed, "input_error", "금액 오타")).unwrap();
        assert_eq!(c.action, "replace");
        assert_eq!(c.replacement_entry_id, Some(new.id));
        assert_eq!(new.supersedes_id, Some(e.id));
        assert_eq!(f.db.read(|c| get(c, e.id)).unwrap().state, "superseded");
        let s = f.db.read(|c| month_summary(c, f.months[0])).unwrap();
        assert_eq!(s.collection_total, 77_000);
        // 사슬: 원 거래 → 대체 거래. 대체 거래를 다시 대체하면 사슬이 길어진다.
        let (_, newer) = f
            .db
            .write(|tx| replace(tx, new.id, &f.entry(1, "collection", "initial_collect", 75_000), "input_error", "재정정"))
            .unwrap();
        let from_root = f.db.read(|c| chain(c, e.id)).unwrap();
        assert_eq!(from_root.iter().map(|x| x.id).collect::<Vec<_>>(), vec![e.id, new.id, newer.id]);
        let from_tail = f.db.read(|c| chain(c, newer.id)).unwrap();
        assert_eq!(from_tail.len(), 3);
    }

    #[test]
    fn accounting_error_tracks_follow_up_completion() {
        let f = fixture();
        let e = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        let c = f.db.write(|tx| void(tx, e.id, "accounting_error", "에듀파인 이중 징수")).unwrap();
        assert_eq!(c.completion, "pending");
        let done = f.db.write(|tx| correction_complete(tx, c.id, "2026-06-10")).unwrap();
        assert_eq!(done.completion, "done");
        assert_eq!(done.completed_on.as_deref(), Some("2026-06-10"));
        // 입력 오류에는 후속 처리가 없다.
        let e2 = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 1))).unwrap();
        let c2 = f.db.write(|tx| void(tx, e2.id, "input_error", "오타")).unwrap();
        assert_eq!(f.db.write(|tx| correction_complete(tx, c2.id, "2026-06-10")).unwrap_err().code, "INVALID");
    }

    #[test]
    fn ledger_rows_cannot_be_edited_or_deleted() {
        let f = fixture();
        let e = f.db.write(|tx| insert(tx, &f.entry(1, "collection", "initial_collect", 70_000))).unwrap();
        let err = f
            .db
            .write(|tx| {
                tx.execute("UPDATE ledger_entry SET amount = 1 WHERE id = ?1", [e.id])?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "LEDGER_IMMUTABLE");
        let err = f
            .db
            .write(|tx| {
                tx.execute("UPDATE ledger_entry SET fund_type_id = ?2 WHERE id = ?1", params![e.id, f.fund_free_voucher])?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "LEDGER_IMMUTABLE");
        let err = f
            .db
            .write(|tx| {
                tx.execute("DELETE FROM ledger_entry WHERE id = ?1", [e.id])?;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(err.code, "LEDGER_NO_DELETE");
        // 상태·단계·처리일은 바꿀 수 있다.
        f.db.write(|tx| {
            tx.execute(
                "UPDATE ledger_entry SET stage = 'planned', processed_on = '2026-03-31' WHERE id = ?1",
                [e.id],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn fund_transfer_must_balance_and_voids_as_a_pair() {
        let f = fixture();
        let mut from = f.entry(3, "transfer", "fund_transfer", -70_000);
        from.fund_type_id = Some(f.fund_beneficiary);
        let mut to = f.entry(3, "transfer", "fund_transfer", 70_000);
        to.fund_type_id = Some(f.fund_free_voucher);

        let mut unbalanced = to.clone();
        unbalanced.amount = 60_000;
        assert_eq!(f.db.write(|tx| insert_transfer(tx, &from, &unbalanced)).unwrap_err().code, "INVALID");

        let mut same_fund = to.clone();
        same_fund.fund_type_id = Some(f.fund_beneficiary);
        assert_eq!(f.db.write(|tx| insert_transfer(tx, &from, &same_fund)).unwrap_err().code, "INVALID");

        // 단건으로는 넣을 수 없다.
        assert_eq!(f.db.write(|tx| insert(tx, &from)).unwrap_err().code, "INVALID");

        let (a, b) = f.db.write(|tx| insert_transfer(tx, &from, &to)).unwrap();
        assert_eq!(a.transfer_group_id, b.transfer_group_id);
        let s = f.db.read(|c| month_summary(c, f.months[2])).unwrap();
        assert_eq!(s.transfer_balance, 0);
        assert_eq!(s.collection_total, 0, "재원 이동은 징수 합계를 바꾸지 않는다");
        let by_fund: Vec<(String, i64)> = s.rows.iter().map(|r| (r.fund_type_name.clone(), r.total)).collect();
        assert!(by_fund.contains(&("수익자".into(), -70_000)));
        assert!(by_fund.contains(&("자유수강권".into(), 70_000)));

        // 한쪽을 취소하면 쌍이 함께 취소된다.
        f.db.write(|tx| void(tx, a.id, "input_error", "경정 취소")).unwrap();
        assert_eq!(f.db.read(|c| get(c, b.id)).unwrap().state, "voided");
        assert_eq!(f.db.write(|tx| replace(tx, b.id, &to, "input_error", "x")).unwrap_err().code, "INVALID");
    }

    #[test]
    fn retroactive_refund_lands_in_original_month_with_later_processed_date() {
        // 원칙 3: 6월에 확정된 자유수강권 환불을 3·4·5월에 귀속시키고 처리일은 6월로 남긴다.
        let f = fixture();
        for seq in 1..=3 {
            f.db.write(|tx| insert(tx, &f.entry(seq, "collection", "initial_collect", 70_000))).unwrap();
        }
        for seq in 1..=3 {
            let mut r = f.entry(seq, "collection", "refund", -70_000);
            r.processed_on = Some("2026-06-15".into());
            r.reason = Some("자유수강권 확정 소급".into());
            f.db.write(|tx| insert(tx, &r)).unwrap();
        }
        let months = f.db.read(|c| crate::repo::year::months(c, f.year_id)).unwrap();
        assert_eq!(months[0].collection_total, 0);
        assert_eq!(months[2].collection_total, 0);
        assert_eq!(months[3].collection_total, 0, "6월에는 아무것도 반영되지 않는다");
        let e = f.db.read(|c| get(c, 4)).unwrap();
        assert_eq!(e.processed_on.as_deref(), Some("2026-06-15"));
    }

    #[test]
    fn validation_rejects_bad_kinds_and_signs() {
        let f = fixture();
        let mut bad = f.entry(1, "collection", "refund", 10);
        assert_eq!(f.db.write(|tx| insert(tx, &bad)).unwrap_err().code, "INVALID");
        bad = f.entry(1, "collection", "initial_collect", -10);
        assert_eq!(f.db.write(|tx| insert(tx, &bad)).unwrap_err().code, "INVALID");
        bad = f.entry(1, "collection", "weird", 10);
        assert_eq!(f.db.write(|tx| insert(tx, &bad)).unwrap_err().code, "INVALID");
        bad = f.entry(1, "collection", "initial_collect", 10);
        bad.processed_on = Some("2026/03/01".into());
        assert_eq!(f.db.write(|tx| insert(tx, &bad)).unwrap_err().code, "INVALID");
        bad = f.entry(1, "collection", "initial_collect", 10);
        bad.fiscal_month_id = 999_999;
        assert_eq!(f.db.write(|tx| insert(tx, &bad)).unwrap_err().code, "NOT_FOUND");
    }

    #[test]
    fn planned_and_actual_payments_are_kept_apart() {
        let f = fixture();
        let mut planned = f.entry(1, "expenditure", "vendor_pay", 378_000);
        planned.stage = Some("planned".into());
        planned.amount_item_id = f.item_textbook_material;
        f.db.write(|tx| insert(tx, &planned)).unwrap();
        let mut actual = planned.clone();
        actual.stage = Some("actual".into());
        f.db.write(|tx| insert(tx, &actual)).unwrap();
        let s = f.db.read(|c| month_summary(c, f.months[0])).unwrap();
        let stages: Vec<&str> = s.rows.iter().map(|r| r.stage.as_str()).collect();
        assert!(stages.contains(&"planned") && stages.contains(&"actual"));
        assert_eq!(s.rows.len(), 2);
    }
}
