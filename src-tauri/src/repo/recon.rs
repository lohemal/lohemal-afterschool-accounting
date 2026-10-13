//! 월별 회계 대조 엔진.
//!
//! 검증 결과는 저장하지 않는다 — 살아 있는 원장 거래와 살아 있는 배분에서 매번 계산한다.
//! 파일 취소·교체·복원이 일어나면 자연히 바뀐다. 담당자의 확인 메모만 `recon_note` 에 남고,
//! 메모를 저장할 때의 지문(fingerprint)과 지금 지문이 다르면 '재확인 필요'다.
//!
//! 판정 규칙 (지시서 4-5)
//!   unverifiable 검증 불가  — 수용비·기타처럼 대응 지출 자료가 없는 항목
//!   group_only   그룹 검증  — 지급이 그룹으로 묶여 개별 프로그램 지급액을 알 수 없음
//!   unallocated  배분 미확정 — 이 달에 아직 프로그램에 배분되지 않은 같은 항목의 징수 거래가 있음
//!   unmapped     매핑 미확정 — 재원이 미확정인 거래가 섞여 있음
//!   missing      자료 부족  — 징수 쪽이나 지출 쪽 자료가 없음 (0원으로 보지 않는다)
//!   mismatch     불일치     — 비교 가능한 재원에서 차액이 있음
//!   ok           정상       — 비교 가능한 재원이 모두 0원 차액
//! 차액 = 징수 기준액 − 지급액. 기준액 = 최초 징수 + 추가징수 − 환불(원장 부호 그대로 합산).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::hash;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{audit, year};

pub const ITEM_SETS: [(&str, &str); 4] = [
    ("instructor", "강사료"),
    ("textbook_material", "교재·재료비"),
    ("admin", "수용비"),
    ("other", "기타"),
];

fn item_set_of(code: &str) -> &'static str {
    match code {
        "instructor" => "instructor",
        "textbook" | "material" | "textbook_material" => "textbook_material",
        "admin" => "admin",
        _ => "other",
    }
}

pub fn status_label(s: &str) -> &'static str {
    match s {
        "ok" => "정상",
        "mismatch" => "불일치",
        "missing" => "자료 부족",
        "unallocated" => "배분 미확정",
        "unmapped" => "매핑 미확정",
        "unverifiable" => "검증 불가",
        "group_only" => "그룹으로 검증",
        _ => "알 수 없음",
    }
}

pub fn note_label(s: &str) -> &'static str {
    match s {
        "todo" => "확인 필요",
        "investigating" => "확인 중",
        "explained" => "사유 확인",
        "none" => "—",
        _ => "알 수 없음",
    }
}

/// 판정 우선순위 (차액 분석 기본 정렬).
pub fn status_rank(s: &str) -> i32 {
    match s {
        "unallocated" => 0,
        "unmapped" => 1,
        "mismatch" => 2,
        "missing" => 3,
        "group_only" => 4,
        "ok" => 5,
        "unverifiable" => 6,
        _ => 7,
    }
}

#[derive(Debug, Clone)]
struct E {
    id: i64,
    program_id: Option<i64>,
    group_id: Option<i64>,
    student_id: Option<i64>,
    fund_id: Option<i64>,
    fund_name: String,
    item_code: String,
    flow: String,
    kind: String,
    amount: i64,
}

fn load_entries(conn: &Connection, month: i64) -> AppResult<Vec<E>> {
    let mut st = conn.prepare(
        "SELECT e.id, e.program_id, e.program_group_id, e.student_id, ef.id, coalesce(ef.name, '미확정'), i.code, e.flow, e.entry_kind, e.amount
           FROM ledger_entry e
           LEFT JOIN fund_type f0 ON f0.id = e.fund_type_id
           LEFT JOIN fund_type ef ON ef.id = coalesce(f0.merged_into_id, f0.id)
           JOIN amount_item i ON i.id = e.amount_item_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active' AND e.flow IN ('collection', 'expenditure')
          ORDER BY e.id",
    )?;
    let rows = st.query_map([month], |r| {
        Ok(E {
            id: r.get(0)?,
            program_id: r.get(1)?,
            group_id: r.get(2)?,
            student_id: r.get(3)?,
            fund_id: r.get(4)?,
            fund_name: r.get(5)?,
            item_code: r.get(6)?,
            flow: r.get(7)?,
            kind: r.get(8)?,
            amount: r.get(9)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// 거래 id → [(배분 id, 프로그램, 금액)] (살아 있는 배분, 거래도 살아 있을 때만).
fn load_allocations(conn: &Connection, month: i64) -> AppResult<HashMap<i64, Vec<(i64, i64, i64)>>> {
    let mut st = conn.prepare(
        "SELECT a.entry_id, a.id, a.program_id, a.amount
           FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active' AND a.status = 'active' ORDER BY a.id",
    )?;
    let mut out: HashMap<i64, Vec<(i64, i64, i64)>> = HashMap::new();
    let rows = st.query_map([month], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?;
    for row in rows {
        let (e, a, p, amt) = row?;
        out.entry(e).or_default().push((a, p, amt));
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Unit {
    Program(i64),
    Group(i64),
}

#[derive(Default, Debug, Clone)]
struct Cell {
    initial: i64,
    extra: i64,
    refund: i64,
    allocated_in: i64,
    /// 재원 → (기준액 합, 지급액 합) — None 은 자료 없음
    basis_by_fund: BTreeMap<String, i64>,
    expense_by_fund: BTreeMap<String, i64>,
    basis_entries: BTreeSet<i64>,
    expense_entries: BTreeSet<i64>,
    alloc_ids: BTreeSet<i64>,
    unmapped: bool,
    has_basis: bool,
    has_expense: bool,
}

struct Computed {
    cells: BTreeMap<(Unit, &'static str), Cell>,
    unallocated_by_set: BTreeMap<&'static str, i64>,
    unallocated_total: i64,
    unlinked_students: i64,
    collection_total: i64,
    expenditure_total: i64,
    unit_names: HashMap<Unit, String>,
    members: HashMap<i64, Vec<i64>>,
}

fn compute(conn: &Connection, month: i64) -> AppResult<Computed> {
    let entries = load_entries(conn, month)?;
    let allocs = load_allocations(conn, month)?;
    // 그룹 구성
    let mut members: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut st = conn.prepare("SELECT group_id, program_id FROM program_group_member ORDER BY group_id, program_id")?;
    for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
        let (g, p) = row?;
        members.entry(g).or_default().push(p);
    }
    let mut cells: BTreeMap<(Unit, &'static str), Cell> = BTreeMap::new();
    let mut unallocated_by_set: BTreeMap<&'static str, i64> = BTreeMap::new();
    let mut unallocated_total = 0i64;
    let mut unlinked = 0i64;
    let mut coll = 0i64;
    let mut exp = 0i64;

    for e in &entries {
        let set = item_set_of(&e.item_code);
        if e.flow == "collection" {
            coll += e.amount;
            if e.student_id.is_none() && e.program_id.is_none() && e.group_id.is_none() {
                unlinked += 1;
            }
            let unit = match (e.program_id, e.group_id) {
                (Some(p), _) => Some(Unit::Program(p)),
                (None, Some(g)) => Some(Unit::Group(g)),
                _ => None,
            };
            match unit {
                Some(u) => {
                    let c = cells.entry((u, set)).or_default();
                    add_basis(c, e, e.amount, false, None);
                }
                None => {
                    let mut rest = e.amount;
                    if let Some(list) = allocs.get(&e.id) {
                        for (aid, pid, amt) in list {
                            let c = cells.entry((Unit::Program(*pid), set)).or_default();
                            add_basis(c, e, *amt, true, Some(*aid));
                            rest -= amt;
                        }
                    }
                    if rest != 0 {
                        *unallocated_by_set.entry(set).or_default() += rest;
                        unallocated_total += rest;
                    }
                }
            }
        } else {
            exp += e.amount;
            let unit = match (e.program_id, e.group_id) {
                (Some(p), _) => Unit::Program(p),
                (None, Some(g)) => Unit::Group(g),
                _ => continue,
            };
            let c = cells.entry((unit, set)).or_default();
            c.has_expense = true;
            c.expense_entries.insert(e.id);
            *c.expense_by_fund.entry(e.fund_name.clone()).or_default() += e.amount;
            if e.fund_id.is_none() {
                c.unmapped = true;
            }
        }
    }
    // 그룹 단위의 기준액 = 구성 프로그램 기준액 합 (그룹에 지출이 있거나 그룹 기준 거래가 있을 때만 만든다)
    let group_units: Vec<(i64, &'static str)> = cells
        .keys()
        .filter_map(|(u, s)| if let Unit::Group(g) = u { Some((*g, *s)) } else { None })
        .collect();
    for (g, set) in group_units {
        let member_ids = members.get(&g).cloned().unwrap_or_default();
        let mut merged = Cell::default();
        for p in &member_ids {
            if let Some(c) = cells.get(&(Unit::Program(*p), set)) {
                merged.initial += c.initial;
                merged.extra += c.extra;
                merged.refund += c.refund;
                merged.allocated_in += c.allocated_in;
                for (f, v) in &c.basis_by_fund {
                    *merged.basis_by_fund.entry(f.clone()).or_default() += v;
                }
                merged.basis_entries.extend(c.basis_entries.iter());
                merged.alloc_ids.extend(c.alloc_ids.iter());
                merged.has_basis |= c.has_basis;
                merged.unmapped |= c.unmapped;
            }
        }
        let gc = cells.get_mut(&(Unit::Group(g), set)).unwrap();
        gc.initial += merged.initial;
        gc.extra += merged.extra;
        gc.refund += merged.refund;
        gc.allocated_in += merged.allocated_in;
        for (f, v) in merged.basis_by_fund {
            *gc.basis_by_fund.entry(f).or_default() += v;
        }
        gc.basis_entries.extend(merged.basis_entries);
        gc.alloc_ids.extend(merged.alloc_ids);
        gc.has_basis |= merged.has_basis;
        gc.unmapped |= merged.unmapped;
    }
    // 이름
    let mut unit_names = HashMap::new();
    for (u, _) in cells.keys() {
        if unit_names.contains_key(u) {
            continue;
        }
        let name = match u {
            Unit::Program(p) => conn.query_row("SELECT name FROM program WHERE id = ?1", [p], |r| r.get::<_, String>(0)).optional()?.unwrap_or_else(|| format!("프로그램 #{p}")),
            Unit::Group(g) => conn.query_row("SELECT name FROM program_group WHERE id = ?1", [g], |r| r.get::<_, String>(0)).optional()?.map(|n| format!("{n} (그룹)")).unwrap_or_else(|| format!("그룹 #{g}")),
        };
        unit_names.insert(*u, name);
    }
    Ok(Computed { cells, unallocated_by_set, unallocated_total, unlinked_students: unlinked, collection_total: coll, expenditure_total: exp, unit_names, members })
}

fn add_basis(c: &mut Cell, e: &E, amount: i64, via_alloc: bool, alloc_id: Option<i64>) {
    match e.kind.as_str() {
        "initial_collect" => c.initial += amount,
        "extra_collect" => c.extra += amount,
        "refund" => c.refund += amount,
        _ => c.initial += amount,
    }
    if via_alloc {
        c.allocated_in += amount;
    }
    c.has_basis = true;
    c.basis_entries.insert(e.id);
    if let Some(a) = alloc_id {
        c.alloc_ids.insert(a);
    }
    *c.basis_by_fund.entry(e.fund_name.clone()).or_default() += amount;
    if e.fund_id.is_none() {
        c.unmapped = true;
    }
}

fn judge(unit: Unit, set: &'static str, c: &Cell, cp: &Computed, group_paid: bool) -> (String, Vec<FundLine>, Vec<String>) {
    let mut hints = Vec::new();
    let mut funds: Vec<FundLine> = Vec::new();
    let names: BTreeSet<&String> = c.basis_by_fund.keys().chain(c.expense_by_fund.keys()).collect();
    for f in names {
        let b = c.basis_by_fund.get(f).copied();
        let e = c.expense_by_fund.get(f).copied();
        let (diff, state) = match (b, e) {
            (Some(b), Some(e)) => (Some(b - e), if b == e { "ok" } else { "mismatch" }),
            (Some(_), None) => (None, "no_expense"),
            (None, Some(_)) => (None, "no_basis"),
            _ => (None, "no_basis"),
        };
        funds.push(FundLine { fund_name: f.clone(), basis: b, expense: e, diff, state: state.into() });
    }
    if c.refund != 0 {
        hints.push("환불 거래가 있습니다.".into());
    }
    if c.extra != 0 {
        hints.push("추가징수 거래가 있습니다.".into());
    }
    if c.allocated_in != 0 {
        hints.push("배분으로 귀속된 징수액이 포함되어 있습니다.".into());
    }
    for f in &funds {
        if f.state == "no_basis" {
            hints.push(format!("재원 '{}' 은(는) 지급 자료만 있어 대조 자료 없음.", f.fund_name));
        } else if f.state == "no_expense" {
            hints.push(format!("재원 '{}' 은(는) 징수 자료만 있어 대조 자료 없음.", f.fund_name));
        }
    }
    if cp.unlinked_students > 0 {
        hints.push(format!("이 달에 학생을 식별하지 못한 징수 거래 {}건이 있습니다.", cp.unlinked_students));
    }
    let status = if set == "admin" || set == "other" {
        "unverifiable"
    } else if group_paid {
        hints.push("지급이 프로그램 그룹으로 묶여 있어 개별 프로그램 지급액은 확인되지 않습니다.".into());
        "group_only"
    } else if cp.unallocated_by_set.get(set).copied().unwrap_or(0) != 0 && matches!(unit, Unit::Program(_) | Unit::Group(_)) {
        hints.push(format!(
            "이 달의 {} 미배분 징수액 {} 이 남아 있어 프로그램별 기준액이 완전하지 않습니다.",
            ITEM_SETS.iter().find(|(c, _)| *c == set).map(|x| x.1).unwrap_or(set),
            crate::domain::money::format_won(cp.unallocated_by_set.get(set).copied().unwrap_or(0))
        ));
        "unallocated"
    } else if c.unmapped {
        hints.push("재원이 미확정인 거래가 섞여 있어 재원별 대조를 할 수 없습니다.".into());
        "unmapped"
    } else if !c.has_basis || !c.has_expense {
        hints.push(if !c.has_basis { "징수 기준 자료가 없습니다 (0원으로 보지 않음)." } else { "지급 자료가 없습니다 (0원으로 보지 않음). 지급 자료 누락 가능성." }.into());
        "missing"
    } else {
        let comparable: Vec<&FundLine> = funds.iter().filter(|f| f.diff.is_some()).collect();
        if comparable.is_empty() {
            hints.push("같은 재원에 징수와 지급이 함께 있는 경우가 없어 비교할 수 없습니다.".into());
            "missing"
        } else if comparable.iter().any(|f| f.diff != Some(0)) {
            let per: Vec<String> = comparable.iter().filter(|f| f.diff != Some(0)).map(|f| format!("{} {}", f.fund_name, crate::domain::money::format_won(f.diff.unwrap_or(0)))).collect();
            hints.push(format!("재원별 차액: {}", per.join(" / ")));
            hints.push("원본 자료의 기준 시점 차이 가능성(9월 전환 등)을 함께 확인하세요.".into());
            "mismatch"
        } else {
            "ok"
        }
    };
    (status.into(), funds, hints)
}

fn fingerprint(c: &Cell, set: &str, unit: Unit) -> String {
    let text = format!(
        "{unit:?}|{set}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}",
        c.initial, c.extra, c.refund, c.allocated_in, c.basis_by_fund, c.expense_by_fund, c.basis_entries, c.alloc_ids
    );
    hash::sha256_hex(text.as_bytes())[..16].to_string()
}

fn latest_note(conn: &Connection, month: i64, unit: Unit, set: &str) -> AppResult<Option<ReconNote>> {
    let (kind, id) = unit_key(unit);
    Ok(conn
        .query_row(
            "SELECT id, status, reason, fingerprint, created_at FROM recon_note
              WHERE fiscal_month_id = ?1 AND unit_kind = ?2 AND unit_id = ?3 AND item_set = ?4 ORDER BY id DESC LIMIT 1",
            params![month, kind, id, set],
            |r| Ok(ReconNote { id: r.get(0)?, status: r.get(1)?, reason: r.get(2)?, fingerprint: r.get(3)?, created_at: r.get(4)? }),
        )
        .optional()?)
}

fn unit_key(u: Unit) -> (&'static str, i64) {
    match u {
        Unit::Program(p) => ("program", p),
        Unit::Group(g) => ("group", g),
    }
}

fn build_row(conn: &Connection, month: i64, cp: &Computed, unit: Unit, set: &'static str, c: &Cell) -> AppResult<ReconRow> {
    // 이 프로그램이 속한 그룹이 같은 항목에 지급을 가지고 있으면 개별 지급액은 미확정이다.
    let group_paid = match unit {
        Unit::Program(p) => cp.members.iter().any(|(g, ms)| ms.contains(&p) && cp.cells.get(&(Unit::Group(*g), set)).map(|gc| gc.has_expense).unwrap_or(false)),
        Unit::Group(_) => false,
    };
    let (status, funds, hints) = judge(unit, set, c, cp, group_paid);
    let basis: i64 = c.basis_by_fund.values().sum();
    let expense: i64 = c.expense_by_fund.values().sum();
    // 차액은 양쪽 자료가 다 있는 재원끼리만 계산한다. 한쪽에만 있는 재원(맞춤형지원 등)은 제외하고 이름을 알린다.
    let comparable_diff: i64 = funds.iter().filter_map(|f| f.diff).sum();
    let uncompared: Vec<String> = funds.iter().filter(|f| f.diff.is_none()).map(|f| f.fund_name.clone()).collect();
    let label = if matches!(status.as_str(), "ok" | "mismatch") && !uncompared.is_empty() {
        format!("{} ({} 제외)", status_label(&status), uncompared.join("·"))
    } else {
        status_label(&status).to_string()
    };
    let fp = fingerprint(c, set, unit);
    let note = latest_note(conn, month, unit, set)?;
    let (note_status, needs_recheck) = match &note {
        Some(n) => (n.status.clone(), n.fingerprint != fp),
        None => ("none".into(), false),
    };
    let (unit_kind, unit_id) = unit_key(unit);
    Ok(ReconRow {
        unit_kind: unit_kind.into(),
        unit_id,
        unit_name: cp.unit_names.get(&unit).cloned().unwrap_or_default(),
        item_set: set.into(),
        item_label: ITEM_SETS.iter().find(|(x, _)| *x == set).map(|x| x.1.to_string()).unwrap_or_default(),
        initial: c.initial,
        extra: c.extra,
        refund: c.refund,
        allocated_in: c.allocated_in,
        basis,
        expense,
        diff: comparable_diff,
        status_label: label,
        status,
        hints,
        funds,
        note_label: note_label(&note_status).into(),
        note_status,
        needs_recheck,
        fingerprint: fp,
    })
}

pub fn month(conn: &Connection, fiscal_month_id: i64) -> AppResult<MonthRecon> {
    let cp = compute(conn, fiscal_month_id)?;
    let mut rows = Vec::new();
    for ((unit, set), c) in &cp.cells {
        rows.push(build_row(conn, fiscal_month_id, &cp, *unit, set, c)?);
    }
    rows.sort_by(|a, b| status_rank(&a.status).cmp(&status_rank(&b.status)).then(a.unit_name.cmp(&b.unit_name)).then(a.item_set.cmp(&b.item_set)));
    let comparable: Vec<&ReconRow> = rows.iter().filter(|r| matches!(r.status.as_str(), "ok" | "mismatch")).collect();
    let comparable_basis = comparable.iter().flat_map(|r| r.funds.iter().filter(|f| f.diff.is_some()).map(|f| f.basis.unwrap_or(0))).sum();
    let comparable_expense = comparable.iter().flat_map(|r| r.funds.iter().filter(|f| f.diff.is_some()).map(|f| f.expense.unwrap_or(0))).sum();
    let todo_count = rows.iter().filter(|r| matches!(r.status.as_str(), "mismatch" | "unallocated" | "unmapped") && r.note_status != "explained" || r.needs_recheck).count() as i64;
    let mismatch_count = rows.iter().filter(|r| r.status == "mismatch").count() as i64;
    Ok(MonthRecon {
        fiscal_month_id,
        collection_total: cp.collection_total,
        expenditure_total: cp.expenditure_total,
        comparable_basis,
        comparable_expense,
        unallocated_total: cp.unallocated_total,
        todo_count,
        mismatch_count,
        rows,
    })
}

// ── 확인 메모 ───────────────────────────────────────────────────────────

pub fn note_save(conn: &Connection, fiscal_month_id: i64, unit_kind: &str, unit_id: i64, item_set: &str, status: &str, reason: &str) -> AppResult<ReconNote> {
    if !matches!(status, "todo" | "investigating" | "explained") {
        return Err(AppError::invalid("확인 상태는 확인 필요·확인 중·사유 확인 중 하나여야 합니다."));
    }
    let unit = match unit_kind {
        "program" => Unit::Program(unit_id),
        "group" => Unit::Group(unit_id),
        _ => return Err(AppError::invalid("알 수 없는 단위입니다.")),
    };
    let set: &'static str = ITEM_SETS.iter().find(|(c, _)| *c == item_set).map(|x| x.0).ok_or_else(|| AppError::invalid("알 수 없는 항목입니다."))?;
    let cp = compute(conn, fiscal_month_id)?;
    let fp = match cp.cells.get(&(unit, set)) {
        Some(c) => fingerprint(c, set, unit),
        None => fingerprint(&Cell::default(), set, unit),
    };
    let year_id: i64 = conn.query_row("SELECT school_year_id FROM fiscal_month WHERE id = ?1", [fiscal_month_id], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO recon_note (school_year_id, fiscal_month_id, unit_kind, unit_id, item_set, status, reason, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![year_id, fiscal_month_id, unit_kind, unit_id, item_set, status, reason.trim(), fp],
    )?;
    let id = conn.last_insert_rowid();
    audit::write(conn, "recon.note", "recon_note", Some(id), &format!("대조 확인 상태 '{}' 기록", note_label(status)), serde_json::json!({ "unitKind": unit_kind, "unitId": unit_id, "itemSet": item_set }))?;
    Ok(conn.query_row(
        "SELECT id, status, reason, fingerprint, created_at FROM recon_note WHERE id = ?1",
        [id],
        |r| Ok(ReconNote { id: r.get(0)?, status: r.get(1)?, reason: r.get(2)?, fingerprint: r.get(3)?, created_at: r.get(4)? }),
    )?)
}

pub fn notes(conn: &Connection, fiscal_month_id: i64, unit_kind: &str, unit_id: i64, item_set: &str) -> AppResult<Vec<ReconNote>> {
    let mut st = conn.prepare(
        "SELECT id, status, reason, fingerprint, created_at FROM recon_note
          WHERE fiscal_month_id = ?1 AND unit_kind = ?2 AND unit_id = ?3 AND item_set = ?4 ORDER BY id DESC",
    )?;
    let rows = st.query_map(params![fiscal_month_id, unit_kind, unit_id, item_set], |r| {
        Ok(ReconNote { id: r.get(0)?, status: r.get(1)?, reason: r.get(2)?, fingerprint: r.get(3)?, created_at: r.get(4)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ── 상세 추적 ───────────────────────────────────────────────────────────

fn kind_label(k: &str) -> &'static str {
    match k {
        "initial_collect" => "최초 징수",
        "extra_collect" => "추가징수",
        "refund" => "환불",
        "fund_transfer" => "재원 이동",
        "instructor_pay" => "강사료 지급",
        "vendor_pay" => "교재·재료비 지급",
        "adjustment" => "정정",
        _ => "",
    }
}

fn trace_entry(conn: &Connection, entry_id: i64, attributed: i64, via_allocation: bool) -> AppResult<TraceEntry> {
    conn.query_row(
        "SELECT e.id, e.entry_kind, coalesce(p.name, g.name || ' (그룹)', '미배분'),
                CASE WHEN e.student_id IS NOT NULL THEN coalesce(s.grade, 0) || '학년 ' || coalesce(s.class_name, '') || '반 ' || coalesce(s.student_no, '') || '번 ' || coalesce(s.name, '') ELSE e.counterparty END,
                coalesce(ef.name, '미확정'), coalesce(ur.fund_text, ''), i.name, e.amount, m.ym, e.processed_on, e.state,
                f.id, coalesce(f.file_name, ''), coalesce(f.version_no, 0), coalesce(f.status, ''), coalesce(sr.sheet_name, ''), coalesce(sr.row_no, 0), e.source_row_id
           FROM ledger_entry e
           LEFT JOIN program p ON p.id = e.program_id
           LEFT JOIN program_group g ON g.id = e.program_group_id
           LEFT JOIN student s ON s.id = e.student_id
           LEFT JOIN fund_type f0 ON f0.id = e.fund_type_id
           LEFT JOIN fund_type ef ON ef.id = coalesce(f0.merged_into_id, f0.id)
           JOIN amount_item i ON i.id = e.amount_item_id
           JOIN fiscal_month m ON m.id = e.fiscal_month_id
           LEFT JOIN upload_file f ON f.id = e.upload_file_id
           LEFT JOIN source_row sr ON sr.id = e.source_row_id
           LEFT JOIN upload_row ur ON ur.source_row_id = e.source_row_id AND ur.upload_file_id = e.upload_file_id AND ur.amount = e.amount AND ur.entry_kind = e.entry_kind
          WHERE e.id = ?1 LIMIT 1",
        [entry_id],
        |r| {
            let kind: String = r.get(1)?;
            let ym: String = r.get(8)?;
            Ok(TraceEntry {
                entry_id: r.get(0)?,
                kind_label: kind_label(&kind).into(),
                entry_kind: kind,
                program_name: r.get(2)?,
                party: r.get(3)?,
                fund_name: r.get(4)?,
                fund_text: r.get(5)?,
                item_name: r.get(6)?,
                amount: r.get(7)?,
                attributed,
                via_allocation,
                month_label: format!("{} ({ym})", crate::domain::months::label(&ym)),
                processed_on: r.get(9)?,
                state: r.get(10)?,
                file_id: r.get(11)?,
                file_name: r.get(12)?,
                version_no: r.get(13)?,
                file_status: r.get(14)?,
                sheet: r.get(15)?,
                row_no: r.get(16)?,
                source_row_id: r.get(17)?,
            })
        },
    )
    .map_err(Into::into)
}

pub fn detail(conn: &Connection, fiscal_month_id: i64, unit_kind: &str, unit_id: i64, item_set: &str) -> AppResult<ReconDetail> {
    let unit = match unit_kind {
        "program" => Unit::Program(unit_id),
        "group" => Unit::Group(unit_id),
        _ => return Err(AppError::invalid("알 수 없는 단위입니다.")),
    };
    let set: &'static str = ITEM_SETS.iter().find(|(c, _)| *c == item_set).map(|x| x.0).ok_or_else(|| AppError::invalid("알 수 없는 항목입니다."))?;
    let cp = compute(conn, fiscal_month_id)?;
    let empty = Cell::default();
    let c = cp.cells.get(&(unit, set)).unwrap_or(&empty);
    let row = build_row(conn, fiscal_month_id, &cp, unit, set, c)?;
    let allocs = load_allocations(conn, fiscal_month_id)?;
    // 귀속 금액: 직접 거래는 전액, 배분 거래는 배분액
    let member_programs: BTreeSet<i64> = match unit {
        Unit::Program(p) => [p].into_iter().collect(),
        Unit::Group(g) => cp.members.get(&g).cloned().unwrap_or_default().into_iter().collect(),
    };
    let mut collection = Vec::new();
    for id in &c.basis_entries {
        let direct: Option<i64> = conn.query_row("SELECT amount FROM ledger_entry WHERE id = ?1 AND (program_id IS NOT NULL OR program_group_id IS NOT NULL)", [id], |r| r.get(0)).optional()?;
        match direct {
            Some(a) => collection.push(trace_entry(conn, *id, a, false)?),
            None => {
                let attributed: i64 = allocs.get(id).map(|l| l.iter().filter(|(_, p, _)| member_programs.contains(p)).map(|x| x.2).sum()).unwrap_or(0);
                collection.push(trace_entry(conn, *id, attributed, true)?);
            }
        }
    }
    let mut expenditure = Vec::new();
    for id in &c.expense_entries {
        let t = trace_entry(conn, *id, 0, false)?;
        let a = t.amount;
        expenditure.push(TraceEntry { attributed: a, ..t });
    }
    // 취소된(이력) 거래 — 합계에는 없지만 조회는 된다.
    let mut voided_ids: Vec<i64> = Vec::new();
    {
        let (kind, id) = unit_key(unit);
        let sql = if kind == "program" {
            "SELECT e.id FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id WHERE e.fiscal_month_id = ?1 AND e.state <> 'active' AND e.program_id = ?2 ORDER BY e.id"
        } else {
            "SELECT e.id FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id WHERE e.fiscal_month_id = ?1 AND e.state <> 'active' AND e.program_group_id = ?2 ORDER BY e.id"
        };
        let mut st = conn.prepare(sql)?;
        for r in st.query_map(params![fiscal_month_id, id], |r| r.get::<_, i64>(0))? {
            voided_ids.push(r?);
        }
    }
    for id in voided_ids {
        let t = trace_entry(conn, id, 0, false)?;
        if item_set_of(&item_code_of(conn, id)?) != set {
            continue;
        }
        if t.flow_is_collection() {
            collection.push(t);
        } else {
            expenditure.push(t);
        }
    }
    let mut parties: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for t in collection.iter().chain(expenditure.iter()).filter(|t| t.state == "active") {
        let e = parties.entry(if t.party.is_empty() { "(없음)".into() } else { t.party.clone() }).or_default();
        e.0 += 1;
        e.1 += t.attributed;
    }
    let members: Vec<String> = member_programs.iter().filter_map(|p| cp.unit_names.get(&Unit::Program(*p)).cloned().or_else(|| conn.query_row("SELECT name FROM program WHERE id = ?1", [p], |r| r.get::<_, String>(0)).ok())).collect();
    let (kind, id) = unit_key(unit);
    Ok(ReconDetail {
        month_label: year::month_label_of(conn, fiscal_month_id)?,
        row,
        members: if matches!(unit, Unit::Group(_)) { members } else { Vec::new() },
        collection,
        expenditure,
        parties: parties.into_iter().map(|(name, (count, total))| SummaryLine { name, count, total }).collect(),
        notes: notes(conn, fiscal_month_id, kind, id, set)?,
    })
}

fn item_code_of(conn: &Connection, entry_id: i64) -> AppResult<String> {
    Ok(conn.query_row("SELECT i.code FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id WHERE e.id = ?1", [entry_id], |r| r.get(0))?)
}

impl TraceEntry {
    fn flow_is_collection(&self) -> bool {
        matches!(self.entry_kind.as_str(), "initial_collect" | "extra_collect" | "refund" | "adjustment")
    }
}

pub fn source_row(conn: &Connection, source_row_id: i64) -> AppResult<SourceRowView> {
    conn.query_row(
        "SELECT f.id, f.file_name, f.version_no, f.stored_name, s.sheet_name, s.row_no, s.cells_json
           FROM source_row s JOIN upload_file f ON f.id = s.upload_file_id WHERE s.id = ?1",
        [source_row_id],
        |r| {
            let cells: Vec<String> = serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default();
            Ok(SourceRowView { file_id: r.get(0)?, file_name: r.get(1)?, version_no: r.get(2)?, stored_name: r.get(3)?, sheet: r.get(4)?, row_no: r.get(5)?, cells })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::not_found("원본 행을 찾지 못했습니다."))
}

#[cfg(test)]
#[path = "recon_tests.rs"]
mod tests;
