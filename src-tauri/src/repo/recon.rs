//! 월별 회계 대조 엔진.
//!
//! 검증 결과는 저장하지 않는다 — 살아 있는 원장 거래와 살아 있는 배분에서 매번 계산한다.
//! 파일 취소·교체·복원이 일어나면 자연히 바뀐다. 담당자의 확인 메모만 `recon_note` 에 남고,
//! 메모를 저장할 때의 지문(fingerprint)과 지금 지문이 다르면 '재확인 필요'다.
//!
//! 재원 분류는 **이름이 아니라 재원 id 와 상위 관계**로 한다 (P2.1, `FundIndex`).
//!   · 통합 재원(`merged_into_id`)은 통합 대상 id 로 접는다 → '초3 지원금' = 방과후이용권, 한 번만 더한다.
//!   · `parent_id` 가 있는 재원(초3 초과금 → 일반 수익자)은 독립 조회도 되고, '수익자 통합 보기'에서는 상위와 합친다.
//!     통합 합계 = 상위 + 하위 (하위를 다시 더하지 않는다).
//!   · 맞춤형지원·방과후이용권·자유수강권은 상위가 없는 독립 재원이라 어떤 합계에도 섞이지 않는다.
//!
//! 차액은 두 가지로 나누어 보여 준다.
//!   전체 단순 차액   = 전체 징수 기준액 − 전체 지급액 (참고 지표, 비교 범위가 다르면 오류가 아니다)
//!   재원별 대조 차액 = 양쪽 자료가 다 있는 재원끼리의 (징수 − 지급) 합 — 판정은 이것으로만 한다.
//!   전체 = 비교 가능 + 비교 제외(한쪽에만 있는 재원) + 매핑 미확정(재원 NULL) 이 항상 성립한다.
//!
//! 판정 (지시서 P2 4-5, P2.1 5장)
//!   unverifiable 검증 불가  — 수용비·기타처럼 대응 지출 자료가 없는 항목
//!   group_only   그룹 검증  — 지급이 그룹으로 묶여 개별 프로그램 지급액을 알 수 없음
//!   unallocated  배분 미확정 — 이 달에 아직 프로그램에 배분되지 않은 같은 항목의 징수 거래가 있음
//!   unmapped     매핑 미확정 — 재원이 미확정인 거래가 섞여 있음
//!   missing      자료 부족  — 징수 쪽이나 지출 쪽 자료가 없음 (0원으로 보지 않는다)
//!   mismatch     불일치     — 비교 가능한 재원에서 차액이 있음 (총액이 같아도 재원이 어긋나면 불일치)
//!   ok           정상       — 비교 가능한 재원이 모두 0원 차액

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
        "orphan" => "원본 재확인 필요",
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
        "orphan" => 1,
        "mismatch" => 2,
        "missing" => 3,
        "group_only" => 4,
        "ok" => 5,
        "unverifiable" => 6,
        _ => 7,
    }
}

// ── 재원 분류 ───────────────────────────────────────────────────────────

/// 재원 표 전체 — 통합·상하위 관계를 id 로 푼다. 이름은 표시에만 쓴다.
#[derive(Debug, Clone)]
pub struct FundIndex {
    names: HashMap<i64, String>,
    sort: HashMap<i64, i32>,
    /// 통합 대상 (없으면 자기 자신)
    effective: HashMap<i64, i64>,
    /// 상위 재원 (통합을 푼 뒤의 id 기준)
    parent: HashMap<i64, i64>,
}

impl FundIndex {
    pub fn load(conn: &Connection) -> AppResult<FundIndex> {
        let mut st = conn.prepare("SELECT id, name, sort_order, merged_into_id, parent_id FROM fund_type")?;
        let rows: Vec<(i64, String, i32, Option<i64>, Option<i64>)> = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?
            .collect::<Result<_, _>>()?;
        let mut idx = FundIndex { names: HashMap::new(), sort: HashMap::new(), effective: HashMap::new(), parent: HashMap::new() };
        for (id, name, sort, merged, _) in &rows {
            idx.names.insert(*id, name.clone());
            idx.sort.insert(*id, *sort);
            idx.effective.insert(*id, merged.unwrap_or(*id));
        }
        for (id, _, _, merged, parent) in &rows {
            if merged.is_some() {
                continue;
            }
            if let Some(p) = parent {
                let pe = idx.effective.get(p).copied().unwrap_or(*p);
                if pe != *id {
                    idx.parent.insert(*id, pe);
                }
            }
        }
        Ok(idx)
    }

    /// 통합을 푼 id. 모르는 id 는 그대로.
    pub fn effective(&self, id: i64) -> i64 {
        self.effective.get(&id).copied().unwrap_or(id)
    }

    /// 통합 보기에서 묶이는 뿌리 (상위 재원이 있으면 상위, 없으면 자기 자신).
    pub fn root(&self, id: i64) -> i64 {
        let e = self.effective(id);
        self.parent.get(&e).copied().unwrap_or(e)
    }

    pub fn name(&self, id: i64) -> String {
        self.names.get(&id).cloned().unwrap_or_else(|| format!("재원 #{id}"))
    }

    pub fn sort_key(&self, id: i64) -> i32 {
        self.sort.get(&id).copied().unwrap_or(i32::MAX)
    }

    /// 뿌리 id 의 하위 재원 이름들 (통합 보기 라벨용).
    pub fn children_names(&self, root: i64) -> Vec<String> {
        let mut v: Vec<(i32, String)> = self.parent.iter().filter(|(_, p)| **p == root).map(|(c, _)| (self.sort_key(*c), self.name(*c))).collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    pub fn merged_label(&self, root: i64) -> String {
        let kids = self.children_names(root);
        if kids.is_empty() {
            self.name(root)
        } else {
            format!("{}({} 포함)", self.name(root), kids.join("·"))
        }
    }
}

// ── 거래 적재 ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct E {
    id: i64,
    program_id: Option<i64>,
    group_id: Option<i64>,
    student_id: Option<i64>,
    /// 통합을 푼 재원 id. None = 미확정
    fund: Option<i64>,
    item_code: String,
    flow: String,
    kind: String,
    amount: i64,
}

fn load_entries(conn: &Connection, month: i64, idx: &FundIndex) -> AppResult<Vec<E>> {
    let mut st = conn.prepare(
        "SELECT e.id, e.program_id, e.program_group_id, e.student_id, e.fund_type_id, i.code, e.flow, e.entry_kind, e.amount
           FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id
          WHERE e.fiscal_month_id = ?1 AND e.state = 'active' AND e.stage = 'actual'
            AND e.flow IN ('collection', 'expenditure', 'transfer')
          ORDER BY e.id",
    )?;
    let rows = st.query_map([month], |r| {
        let raw: Option<i64> = r.get(4)?;
        Ok(E {
            id: r.get(0)?,
            program_id: r.get(1)?,
            group_id: r.get(2)?,
            student_id: r.get(3)?,
            fund: raw.map(|f| idx.effective(f)),
            item_code: r.get(5)?,
            flow: r.get(6)?,
            kind: r.get(7)?,
            amount: r.get(8)?,
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

/// 재원별 금액 칸 키: Some(id) = 통합을 푼 재원, None = 미확정
type FundKey = Option<i64>;

#[derive(Default, Debug, Clone)]
struct Cell {
    initial: i64,
    extra: i64,
    refund: i64,
    /// 재원경정으로 들어오고 나간 금액의 합 (항상 0 이어야 한다)
    transfer: i64,
    allocated_in: i64,
    /// 원본 거래가 취소된 정정이 만든 거래가 섞여 있다
    orphan: bool,
    transfer_entries: i64,
    basis_by_fund: BTreeMap<FundKey, i64>,
    expense_by_fund: BTreeMap<FundKey, i64>,
    basis_entries: BTreeSet<i64>,
    expense_entries: BTreeSet<i64>,
    alloc_ids: BTreeSet<i64>,
    has_basis: bool,
    has_expense: bool,
}

impl Cell {
    fn transfer_count(&self) -> i64 {
        self.transfer_entries
    }
    fn unmapped(&self) -> bool {
        self.basis_by_fund.contains_key(&None) || self.expense_by_fund.contains_key(&None)
    }
    fn absorb(&mut self, o: &Cell) {
        self.initial += o.initial;
        self.extra += o.extra;
        self.refund += o.refund;
        self.transfer += o.transfer;
        self.transfer_entries += o.transfer_entries;
        self.allocated_in += o.allocated_in;
        self.orphan |= o.orphan;
        for (f, v) in &o.basis_by_fund {
            *self.basis_by_fund.entry(*f).or_default() += v;
        }
        self.basis_entries.extend(o.basis_entries.iter());
        self.alloc_ids.extend(o.alloc_ids.iter());
        self.has_basis |= o.has_basis;
    }
}

struct Computed {
    idx: FundIndex,
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
    let idx = FundIndex::load(conn)?;
    let entries = load_entries(conn, month, &idx)?;
    let allocs = load_allocations(conn, month)?;
    let orphans: std::collections::HashSet<i64> = crate::repo::adjustment::orphan_entry_ids(conn, month)?.into_iter().collect();
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
        if e.flow == "collection" || e.flow == "transfer" {
            if e.flow == "collection" {
                coll += e.amount;
            }
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
                    if orphans.contains(&e.id) {
                        c.orphan = true;
                    }
                }
                None => {
                    let mut rest = e.amount;
                    if let Some(list) = allocs.get(&e.id) {
                        for (aid, pid, amt) in list {
                            add_basis(cells.entry((Unit::Program(*pid), set)).or_default(), e, *amt, true, Some(*aid));
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
            *c.expense_by_fund.entry(e.fund).or_default() += e.amount;
            if orphans.contains(&e.id) {
                c.orphan = true;
            }
        }
    }
    // 그룹 기준액 = 구성 프로그램 기준액 합 (그룹 칸이 있을 때만; 대시보드 합계에 겹쳐 더하지 않는다)
    let group_units: Vec<(i64, &'static str)> = cells
        .keys()
        .filter_map(|(u, s)| if let Unit::Group(g) = u { Some((*g, *s)) } else { None })
        .collect();
    for (g, set) in group_units {
        let mut merged = Cell::default();
        for p in members.get(&g).cloned().unwrap_or_default() {
            if let Some(c) = cells.get(&(Unit::Program(p), set)) {
                merged.absorb(c);
            }
        }
        cells.get_mut(&(Unit::Group(g), set)).unwrap().absorb(&merged);
    }
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
    Ok(Computed { idx, cells, unallocated_by_set, unallocated_total, unlinked_students: unlinked, collection_total: coll, expenditure_total: exp, unit_names, members })
}

fn add_basis(c: &mut Cell, e: &E, amount: i64, via_alloc: bool, alloc_id: Option<i64>) {
    match e.kind.as_str() {
        "extra_collect" => c.extra += amount,
        "refund" => c.refund += amount,
        "fund_transfer" => {
            c.transfer += amount;
            c.transfer_entries += 1;
        }
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
    *c.basis_by_fund.entry(e.fund).or_default() += amount;
}

// ── 재원별 줄 · 판정 ────────────────────────────────────────────────────

fn line_state(k: FundKey, b: Option<i64>, e: Option<i64>) -> (Option<i64>, &'static str) {
    match (k, b, e) {
        (None, _, _) => (None, "unmapped"),
        (_, Some(b), Some(e)) => (Some(b - e), if b == e { "ok" } else { "mismatch" }),
        (_, Some(_), None) => (None, "no_expense"),
        _ => (None, "no_basis"),
    }
}

/// 세부 보기 재원 줄. 미확정(None) 재원은 'unmapped' 로 따로 남긴다.
fn fund_lines(idx: &FundIndex, c: &Cell) -> Vec<FundLine> {
    let mut keys: Vec<FundKey> = c.basis_by_fund.keys().chain(c.expense_by_fund.keys()).copied().collect();
    keys.sort();
    keys.dedup();
    keys.sort_by_key(|k| match k { Some(id) => (0, idx.sort_key(*id), *id), None => (1, 0, 0) });
    keys.into_iter()
        .map(|k| {
            let b = c.basis_by_fund.get(&k).copied();
            let e = c.expense_by_fund.get(&k).copied();
            let (diff, state) = line_state(k, b, e);
            FundLine {
                fund_id: k,
                fund_name: match k { Some(id) => idx.name(id), None => "미확정".into() },
                root_id: k.map(|id| idx.root(id)),
                basis: b,
                expense: e,
                diff,
                state: state.into(),
            }
        })
        .collect()
}

/// 수익자 통합 보기 — 상위 재원이 같은 줄을 합친다. 합계는 세부 보기와 같다.
pub fn merge_lines(idx: &FundIndex, lines: &[FundLine]) -> Vec<FundLine> {
    let mut out: BTreeMap<(i32, Option<i64>), FundLine> = BTreeMap::new();
    for l in lines {
        let root = l.root_id;
        let key = (match root { Some(r) => idx.sort_key(r), None => i32::MAX }, root);
        let entry = out.entry(key).or_insert_with(|| FundLine {
            fund_id: root,
            fund_name: match root { Some(r) => idx.merged_label(r), None => "미확정".into() },
            root_id: root,
            basis: None,
            expense: None,
            diff: None,
            state: String::new(),
        });
        entry.basis = match (entry.basis, l.basis) { (None, None) => None, (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)) };
        entry.expense = match (entry.expense, l.expense) { (None, None) => None, (a, b) => Some(a.unwrap_or(0) + b.unwrap_or(0)) };
    }
    out.into_values()
        .map(|mut l| {
            let (diff, state) = line_state(l.fund_id, l.basis, l.expense);
            l.diff = diff;
            l.state = state.into();
            l
        })
        .collect()
}

fn fingerprint(c: &Cell, set: &str, unit: Unit) -> String {
    let text = format!(
        "{unit:?}|{set}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}",
        c.initial, c.extra, c.refund, c.allocated_in, c.basis_by_fund, c.expense_by_fund, c.basis_entries, c.alloc_ids
    );
    hash::sha256_hex(text.as_bytes())[..16].to_string()
}

fn unit_key(u: Unit) -> (&'static str, i64) {
    match u {
        Unit::Program(p) => ("program", p),
        Unit::Group(g) => ("group", g),
    }
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

fn w(v: i64) -> String {
    crate::domain::money::format_won(v)
}

fn build_row(conn: &Connection, month: i64, cp: &Computed, unit: Unit, set: &'static str, c: &Cell) -> AppResult<ReconRow> {
    let group_paid = match unit {
        Unit::Program(p) => cp.members.iter().any(|(g, ms)| ms.contains(&p) && cp.cells.get(&(Unit::Group(*g), set)).map(|gc| gc.has_expense).unwrap_or(false)),
        Unit::Group(_) => false,
    };
    let funds = fund_lines(&cp.idx, c);
    let funds_merged = merge_lines(&cp.idx, &funds);

    // 금액 근거 — 전체 = 비교 가능 + 비교 제외 + 매핑 미확정 이 항상 성립한다.
    let basis: i64 = c.basis_by_fund.values().sum();
    let expense: i64 = c.expense_by_fund.values().sum();
    let comparable_basis: i64 = funds.iter().filter(|f| f.diff.is_some()).map(|f| f.basis.unwrap_or(0)).sum();
    let comparable_expense: i64 = funds.iter().filter(|f| f.diff.is_some()).map(|f| f.expense.unwrap_or(0)).sum();
    let unmapped_basis: i64 = c.basis_by_fund.get(&None).copied().unwrap_or(0);
    let unmapped_expense: i64 = c.expense_by_fund.get(&None).copied().unwrap_or(0);
    let excluded_basis: i64 = funds.iter().filter(|f| f.state == "no_expense").map(|f| f.basis.unwrap_or(0)).sum();
    let excluded_expense: i64 = funds.iter().filter(|f| f.state == "no_basis").map(|f| f.expense.unwrap_or(0)).sum();
    debug_assert_eq!(basis, comparable_basis + excluded_basis + unmapped_basis);
    debug_assert_eq!(expense, comparable_expense + excluded_expense + unmapped_expense);
    let simple_diff = basis - expense;
    let comparable_diff = comparable_basis - comparable_expense;
    let unallocated_in_set = cp.unallocated_by_set.get(set).copied().unwrap_or(0);

    let mut hints = Vec::new();
    let mut excluded: Vec<ExcludedFund> = Vec::new();
    for f in &funds {
        match f.state.as_str() {
            "no_basis" => excluded.push(ExcludedFund { fund_name: f.fund_name.clone(), basis: 0, expense: f.expense.unwrap_or(0), reason: "징수 기준 자료 없음 (지급만 있음)".into() }),
            "no_expense" => excluded.push(ExcludedFund { fund_name: f.fund_name.clone(), basis: f.basis.unwrap_or(0), expense: 0, reason: "지급 자료 없음 (징수만 있음)".into() }),
            "unmapped" => excluded.push(ExcludedFund { fund_name: "미확정".into(), basis: f.basis.unwrap_or(0), expense: f.expense.unwrap_or(0), reason: "재원 매핑 미확정".into() }),
            _ => {}
        }
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
    if c.transfer != 0 {
        hints.push(format!("재원경정 쌍의 합이 0 이 아닙니다({}). 원장 점검이 필요합니다.", w(c.transfer)));
    }
    let n_transfer = c.transfer_count();
    if n_transfer > 0 {
        hints.push(format!("재원경정 {}건이 재원별 기준액에 반영되어 있습니다(전체 기준액은 그대로).", n_transfer));
    }
    for x in &excluded {
        if x.reason.starts_with("징수") {
            hints.push(format!("재원 '{}' 지급 {} 은(는) 징수 기준 자료가 없어 비교에서 뺐습니다 (재원 성격이 아니라 근거 자료 부족 때문).", x.fund_name, w(x.expense)));
        } else if x.reason.starts_with("지급") {
            hints.push(format!("재원 '{}' 징수 {} 은(는) 지급 자료가 없어 비교에서 뺐습니다.", x.fund_name, w(x.basis)));
        }
    }
    if cp.unlinked_students > 0 {
        hints.push(format!("이 달에 학생을 식별하지 못한 징수 거래 {}건이 있습니다.", cp.unlinked_students));
    }
    let comparable: Vec<&FundLine> = funds.iter().filter(|f| f.diff.is_some()).collect();
    let status = if set == "admin" || set == "other" {
        "unverifiable"
    } else if group_paid {
        hints.push("지급이 프로그램 그룹으로 묶여 있어 개별 프로그램 지급액은 확인되지 않습니다.".into());
        "group_only"
    } else if c.orphan {
        hints.push("근거 원본 거래가 취소된 수동 정정이 섞여 있습니다. 정정 이력에서 '원본 재확인 필요' 항목을 확인해 주세요.".into());
        "orphan"
    } else if unallocated_in_set != 0 {
        hints.push(format!(
            "이 달의 {} 미배분 징수액 {} 이 남아 있어 프로그램별 기준액이 완전하지 않습니다.",
            ITEM_SETS.iter().find(|(x, _)| *x == set).map(|x| x.1).unwrap_or(set),
            w(unallocated_in_set)
        ));
        "unallocated"
    } else if c.unmapped() {
        hints.push(format!("재원이 미확정인 거래(징수 {} · 지급 {})가 섞여 있어 재원별 대조를 할 수 없습니다.", w(unmapped_basis), w(unmapped_expense)));
        "unmapped"
    } else if !c.has_basis || !c.has_expense {
        hints.push(if !c.has_basis { "징수 기준 자료가 없습니다 (0원으로 보지 않음)." } else { "지급 자료가 없습니다 (0원으로 보지 않음). 지급 자료 누락 가능성." }.into());
        "missing"
    } else if comparable.is_empty() {
        hints.push("같은 재원에 징수와 지급이 함께 있는 경우가 없어 비교할 수 없습니다.".into());
        "missing"
    } else if comparable.iter().any(|f| f.diff != Some(0)) {
        let per: Vec<String> = comparable.iter().filter(|f| f.diff != Some(0)).map(|f| format!("{} {}", f.fund_name, w(f.diff.unwrap_or(0)))).collect();
        hints.push(format!("재원별 차액: {}", per.join(" / ")));
        if comparable_diff == 0 {
            hints.push("비교 가능한 재원의 총액은 같지만 재원별로 어긋납니다 — 재원 간 이동(과목경정) 확인 대상.".into());
        }
        if simple_diff != comparable_diff {
            hints.push(format!("전체 단순 차액 {} 과 재원별 대조 차액 {} 이 다른 까닭: 비교 제외 징수 {} · 비교 제외 지급 {} · 미확정 징수 {} · 미확정 지급 {}.",
                w(simple_diff), w(comparable_diff), w(excluded_basis), w(excluded_expense), w(unmapped_basis), w(unmapped_expense)));
        }
        hints.push("원본 자료의 기준 시점 차이 가능성(9월 전환 등)을 함께 확인하세요.".into());
        "mismatch"
    } else {
        "ok"
    };
    let total_match_fund_mismatch = status == "mismatch" && comparable_diff == 0;
    let excluded_names: Vec<String> = funds.iter().filter(|f| f.state == "no_basis" || f.state == "no_expense").map(|f| f.fund_name.clone()).collect();
    let label = if total_match_fund_mismatch {
        "총액 일치 · 재원 불일치".to_string()
    } else if status == "ok" && !excluded_names.is_empty() {
        format!("비교 가능 재원 정상 · {} 미검증", excluded_names.join("·"))
    } else if status == "mismatch" && !excluded_names.is_empty() {
        format!("{} ({} 제외)", status_label(status), excluded_names.join("·"))
    } else {
        status_label(status).to_string()
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
        transfer_count: c.transfer_count(),
        allocated_in: c.allocated_in,
        basis,
        expense,
        simple_diff,
        comparable_basis,
        comparable_expense,
        diff: comparable_diff,
        excluded_basis,
        excluded_expense,
        unmapped_basis,
        unmapped_expense,
        unallocated_in_set,
        excluded,
        total_match_fund_mismatch,
        status: status.into(),
        status_label: label,
        hints,
        funds,
        funds_merged,
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
    // 월 합계는 그룹과 구성 프로그램을 겹쳐 더하지 않는다: 그룹으로 검증되는 프로그램 칸은 뺀다.
    let counted: Vec<&ReconRow> = rows.iter().filter(|r| r.status != "group_only").collect();
    let comparable_basis = counted.iter().filter(|r| matches!(r.status.as_str(), "ok" | "mismatch")).map(|r| r.comparable_basis).sum();
    let comparable_expense = counted.iter().filter(|r| matches!(r.status.as_str(), "ok" | "mismatch")).map(|r| r.comparable_expense).sum();
    let excluded_basis = counted.iter().map(|r| if matches!(r.status.as_str(), "ok" | "mismatch") { r.excluded_basis + r.unmapped_basis } else { r.basis }).sum::<i64>() + cp.unallocated_total;
    let excluded_expense = counted.iter().map(|r| if matches!(r.status.as_str(), "ok" | "mismatch") { r.excluded_expense + r.unmapped_expense } else { r.expense }).sum();
    let todo_count = rows.iter().filter(|r| matches!(r.status.as_str(), "mismatch" | "unallocated" | "unmapped" | "orphan") && r.note_status != "explained" || r.needs_recheck).count() as i64;
    Ok(MonthRecon {
        fiscal_month_id,
        collection_total: cp.collection_total,
        expenditure_total: cp.expenditure_total,
        simple_diff: cp.collection_total - cp.expenditure_total,
        comparable_basis,
        comparable_expense,
        excluded_basis,
        excluded_expense,
        unallocated_total: cp.unallocated_total,
        todo_count,
        mismatch_count: rows.iter().filter(|r| r.status == "mismatch").count() as i64,
        missing_count: rows.iter().filter(|r| r.status == "missing").count() as i64,
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
                coalesce(ef.name, '미확정'), coalesce(f0.name, ''), coalesce(ur.fund_text, ''), i.name, e.amount, m.ym, e.processed_on, e.state,
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
            let ym: String = r.get(9)?;
            let stored_fund: String = r.get(5)?;
            let fund_text: String = r.get(6)?;
            Ok(TraceEntry {
                entry_id: r.get(0)?,
                kind_label: kind_label(&kind).into(),
                entry_kind: kind,
                program_name: r.get(2)?,
                party: r.get(3)?,
                fund_name: r.get(4)?,
                // 원본 재원명: 엑셀 표기가 있으면 그것, 없으면 저장된 재원 이름(통합 전)
                fund_text: if fund_text.is_empty() { stored_fund } else { fund_text },
                item_name: r.get(7)?,
                amount: r.get(8)?,
                attributed,
                via_allocation,
                month_label: format!("{} ({ym})", crate::domain::months::label(&ym)),
                processed_on: r.get(10)?,
                state: r.get(11)?,
                file_id: r.get(12)?,
                file_name: r.get(13)?,
                version_no: r.get(14)?,
                file_status: r.get(15)?,
                sheet: r.get(16)?,
                row_no: r.get(17)?,
                source_row_id: r.get(18)?,
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
            "SELECT e.id FROM ledger_entry e WHERE e.fiscal_month_id = ?1 AND e.state <> 'active' AND e.program_id = ?2 ORDER BY e.id"
        } else {
            "SELECT e.id FROM ledger_entry e WHERE e.fiscal_month_id = ?1 AND e.state <> 'active' AND e.program_group_id = ?2 ORDER BY e.id"
        };
        let mut st = conn.prepare(sql)?;
        for r in st.query_map(params![fiscal_month_id, id], |r| r.get::<_, i64>(0))? {
            voided_ids.push(r?);
        }
    }
    for id in voided_ids {
        if item_set_of(&item_code_of(conn, id)?) != set {
            continue;
        }
        let t = trace_entry(conn, id, 0, false)?;
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
        matches!(self.entry_kind.as_str(), "initial_collect" | "extra_collect" | "refund" | "adjustment" | "fund_transfer")
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

#[cfg(test)]
#[path = "recon_fund_tests.rs"]
mod fund_tests;
