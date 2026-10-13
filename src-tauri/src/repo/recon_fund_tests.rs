//! P2.1 — 재원 분류·차액 근거 시험 (지시서 11장 1~18). 가명·가상 금액만.

use super::*;
use crate::db::Db;
use crate::model::{CatalogInput, NewEntry};
use crate::repo::{catalog, ledger, program, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    ben: i64,
    g3x: i64,
    voucher: i64,
    g3s: i64,
    free: i64,
    custom: i64,
    instr: i64,
    tm: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let sep = db.read(|c| year::months(c, y.id)).unwrap().iter().find(|m| m.ym == "2026-09").unwrap().id;
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    T {
        db, year_id: y.id, sep,
        ben: fund("beneficiary"), g3x: fund("g3_excess"), voucher: fund("after_voucher"), g3s: fund("g3_support"),
        free: fund("free_voucher"), custom: fund("custom_support"), instr: item("instructor"), tm: item("textbook_material"),
    }
}

impl T {
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    fn entry(&self, program: i64, fund: Option<i64>, item: i64, flow: &str, amount: i64) -> i64 {
        let kind = if flow == "collection" { "initial_collect" } else if item == self.instr { "instructor_pay" } else { "vendor_pay" };
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: self.sep, program_id: Some(program), program_group_id: None, student_id: None,
                    fund_type_id: fund, amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None,
                    counterparty: Some("가명".into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn row(&self, name: &str, set: &str) -> ReconRow {
        let r = self.db.read(|c| month(c, self.sep)).unwrap();
        r.rows.into_iter().find(|x| x.unit_name == name && x.item_set == set).expect("row")
    }
}

fn line<'a>(lines: &'a [FundLine], name: &str) -> &'a FundLine {
    lines.iter().find(|l| l.fund_name == name).unwrap_or_else(|| panic!("fund line {name} not in {:?}", lines.iter().map(|l| l.fund_name.clone()).collect::<Vec<_>>()))
}

/// 전체 = 비교 가능 + 비교 제외 + 미확정, 세부 보기 합 = 통합 보기 합 — 모든 행에서 성립해야 한다.
fn check_identities(r: &ReconRow) {
    assert_eq!(r.basis, r.comparable_basis + r.excluded_basis + r.unmapped_basis, "징수 항등식 {}", r.unit_name);
    assert_eq!(r.expense, r.comparable_expense + r.excluded_expense + r.unmapped_expense, "지급 항등식 {}", r.unit_name);
    assert_eq!(r.simple_diff, r.basis - r.expense);
    assert_eq!(r.diff, r.comparable_basis - r.comparable_expense);
    let sum = |v: &[FundLine], f: fn(&FundLine) -> Option<i64>| v.iter().map(|l| f(l).unwrap_or(0)).sum::<i64>();
    assert_eq!(sum(&r.funds, |l| l.basis), sum(&r.funds_merged, |l| l.basis), "세부·통합 징수 합계");
    assert_eq!(sum(&r.funds, |l| l.expense), sum(&r.funds_merged, |l| l.expense), "세부·통합 지급 합계");
    assert_eq!(sum(&r.funds, |l| l.basis), r.basis);
    assert_eq!(sum(&r.funds, |l| l.expense), r.expense);
}

#[test]
fn fund_index_uses_ids_not_names() {
    let t = setup();
    let idx = t.db.read(|c| FundIndex::load(c)).unwrap();
    assert_eq!(idx.effective(t.g3s), t.voucher, "초3 지원금 → 방과후이용권");
    assert_eq!(idx.root(t.g3x), t.ben, "초3 초과금의 뿌리는 일반 수익자");
    assert_eq!(idx.root(t.ben), t.ben);
    assert_eq!(idx.root(t.voucher), t.voucher, "이용권은 독립");
    assert_eq!(idx.root(t.custom), t.custom, "맞춤형지원은 독립 — 수익자가 아니다");
    assert_eq!(idx.root(t.free), t.free);
    assert_eq!(idx.name(t.ben), "일반 수익자");
    assert_eq!(idx.name(t.g3x), "초3 초과금");
    assert_eq!(idx.merged_label(t.ben), "일반 수익자(초3 초과금 포함)");
    // 8. 이름을 바꿔도 분류는 그대로
    for (id, name) in [(t.ben, "학부모 부담"), (t.g3x, "3학년 추가 부담"), (t.custom, "수익자 맞춤")] {
        t.db.write(|tx| catalog::fund_type_save(tx, &CatalogInput { id: Some(id), name: name.into(), parent_id: if id == t.g3x { Some(t.ben) } else { None }, is_active: None })).unwrap();
    }
    let idx = t.db.read(|c| FundIndex::load(c)).unwrap();
    assert_eq!(idx.root(t.g3x), t.ben);
    assert_eq!(idx.root(t.custom), t.custom, "'수익자' 글자가 들어가도 수익자 합계에 섞이지 않는다");
    assert_eq!(idx.merged_label(t.ben), "학부모 부담(3학년 추가 부담 포함)");
}

#[test]
fn beneficiary_family_and_independent_funds() {
    let t = setup();
    let p = t.program("코딩");
    // 1~3. 일반 수익자 + 초3 초과금 함께, 양쪽 모두
    t.entry(p, Some(t.ben), t.instr, "collection", 1_000_000);
    t.entry(p, Some(t.g3x), t.instr, "collection", 200_000);
    t.entry(p, Some(t.ben), t.instr, "expenditure", 1_000_000);
    t.entry(p, Some(t.g3x), t.instr, "expenditure", 200_000);
    // 6·7. 이용권('초3 지원금' id 로 징수, '방과후이용권' id 로 지급)·자유수강권
    t.entry(p, Some(t.g3s), t.instr, "collection", 300_000);
    t.entry(p, Some(t.voucher), t.instr, "expenditure", 300_000);
    t.entry(p, Some(t.free), t.instr, "collection", 100_000);
    t.entry(p, Some(t.free), t.instr, "expenditure", 100_000);
    // 5. 맞춤형지원은 지급에만
    t.entry(p, Some(t.custom), t.instr, "expenditure", 500_000);
    let r = t.row("코딩", "instructor");
    check_identities(&r);
    // 세부 보기: 다섯 줄, 초3 초과금 독립
    let names: Vec<&str> = r.funds.iter().map(|l| l.fund_name.as_str()).collect();
    assert_eq!(names, ["일반 수익자", "자유수강권", "초3 초과금", "방과후이용권", "맞춤형지원"]);
    assert_eq!(line(&r.funds, "초3 초과금").basis, Some(200_000));
    assert_eq!(line(&r.funds, "방과후이용권").basis, Some(300_000), "초3 지원금은 이용권으로 한 번만");
    // 4. 통합 보기: 수익자 = 1,000,000 + 200,000, 하위를 다시 더하지 않는다
    let merged = line(&r.funds_merged, "일반 수익자(초3 초과금 포함)");
    assert_eq!((merged.basis, merged.expense, merged.diff), (Some(1_200_000), Some(1_200_000), Some(0)));
    assert!(!r.funds_merged.iter().any(|l| l.fund_name == "초3 초과금"));
    assert_eq!(line(&r.funds_merged, "맞춤형지원").basis, None, "맞춤형지원은 수익자에 섞이지 않는다");
    assert_eq!(line(&r.funds_merged, "방과후이용권").basis, Some(300_000));
    assert_eq!(line(&r.funds_merged, "자유수강권").basis, Some(100_000));
    assert_eq!(r.funds_merged.len(), 4);
    // 전체 합계 2,100,000 (지급), 비교 가능 1,600,000, 제외 500,000
    assert_eq!(r.expense, 2_100_000);
    assert_eq!(r.comparable_expense, 1_600_000);
    assert_eq!(r.excluded_expense, 500_000);
    assert_eq!(r.excluded[0].fund_name, "맞춤형지원");
    assert_eq!(r.excluded[0].reason, "징수 기준 자료 없음 (지급만 있음)");
    assert_eq!(r.status, "ok");
    assert_eq!(r.status_label, "정상 (맞춤형지원 제외)");
    assert_eq!(r.simple_diff, -500_000);
    assert_eq!(r.diff, 0);
}

#[test]
fn only_g3_excess_and_only_beneficiary() {
    let t = setup();
    let a = t.program("A");
    t.entry(a, Some(t.g3x), t.tm, "collection", 12_000);
    t.entry(a, Some(t.g3x), t.tm, "expenditure", 12_000);
    let r = t.row("A", "textbook_material");
    check_identities(&r);
    assert_eq!(r.funds.len(), 1);
    assert_eq!(r.funds_merged[0].fund_name, "일반 수익자(초3 초과금 포함)");
    assert_eq!(r.funds_merged[0].basis, Some(12_000));
    assert_eq!(r.status, "ok");
    let b = t.program("B");
    t.entry(b, Some(t.ben), t.tm, "collection", 5_000);
    t.entry(b, Some(t.ben), t.tm, "expenditure", 7_000);
    let r = t.row("B", "textbook_material");
    check_identities(&r);
    assert_eq!((r.status.as_str(), r.diff, r.simple_diff), ("mismatch", -2_000, -2_000));
}

#[test]
fn simple_diff_vs_comparable_diff_with_excluded_amounts() {
    // 로봇과학 교재·재료비 사례: 일반 수익자 -180,000 · 이용권 +6,200 · 초3 초과금은 지급에만 6,200
    let t = setup();
    let p = t.program("로봇과학");
    t.entry(p, Some(t.ben), t.tm, "collection", 900_000);
    t.entry(p, Some(t.ben), t.tm, "expenditure", 1_080_000);
    t.entry(p, Some(t.voucher), t.tm, "collection", 180_000);
    t.entry(p, Some(t.voucher), t.tm, "expenditure", 173_800);
    t.entry(p, Some(t.g3x), t.tm, "expenditure", 6_200);
    let r = t.row("로봇과학", "textbook_material");
    check_identities(&r);
    assert_eq!((r.basis, r.expense, r.simple_diff), (1_080_000, 1_260_000, -180_000));
    assert_eq!((r.comparable_basis, r.comparable_expense, r.diff), (1_080_000, 1_253_800, -173_800));
    assert_eq!((r.excluded_basis, r.excluded_expense), (0, 6_200));
    assert_eq!(r.status, "mismatch");
    assert!(r.hints.iter().any(|h| h.contains("전체 단순 차액 -180,000") && h.contains("비교 제외 지급 6,200")), "{:?}", r.hints);
    assert!(!r.total_match_fund_mismatch);
}

#[test]
fn zero_total_but_fund_mismatch_is_still_mismatch() {
    let t = setup();
    let p = t.program("바둑");
    t.entry(p, Some(t.ben), t.tm, "collection", 200_000);
    t.entry(p, Some(t.ben), t.tm, "expenditure", 212_000);
    t.entry(p, Some(t.voucher), t.tm, "collection", 232_000);
    t.entry(p, Some(t.voucher), t.tm, "expenditure", 220_000);
    let r = t.row("바둑", "textbook_material");
    check_identities(&r);
    assert_eq!((r.simple_diff, r.diff), (0, 0));
    assert_eq!(r.status, "mismatch");
    assert!(r.total_match_fund_mismatch);
    assert_eq!(r.status_label, "총액 일치 · 재원 불일치");
    assert!(r.hints.iter().any(|h| h.contains("과목경정")));
    assert_eq!(line(&r.funds, "일반 수익자").diff, Some(-12_000));
    assert_eq!(line(&r.funds, "방과후이용권").diff, Some(12_000));
    // 통합 보기에서도 수익자 줄 하나로 -12,000 가 남아 정상처럼 보이지 않는다
    assert_eq!(line(&r.funds_merged, "일반 수익자(초3 초과금 포함)").diff, Some(-12_000));
}

#[test]
fn custom_support_only_payment_and_missing_sides() {
    let t = setup();
    // 15. 맞춤형지원만 지급 → 자료 부족 (비교할 재원 없음), 지급액은 숨기지 않는다
    let a = t.program("전래놀이");
    t.entry(a, Some(t.custom), t.instr, "expenditure", 880_000);
    let r = t.row("전래놀이", "instructor");
    check_identities(&r);
    assert_eq!((r.status.as_str(), r.expense, r.excluded_expense, r.comparable_expense), ("missing", 880_000, 880_000, 0));
    assert_eq!(r.excluded[0].fund_name, "맞춤형지원");
    // 16·17. 한쪽 자료만
    let b = t.program("B");
    t.entry(b, Some(t.ben), t.instr, "collection", 10_000);
    let rb = t.row("B", "instructor");
    check_identities(&rb);
    assert_eq!((rb.status.as_str(), rb.excluded_basis), ("missing", 10_000));
    let c = t.program("C");
    t.entry(c, Some(t.ben), t.instr, "expenditure", 10_000);
    assert_eq!(t.row("C", "instructor").status, "missing");
    // 맞춤형지원에 대응 자료가 생기면 그 재원 자체도 대조된다
    t.entry(a, Some(t.custom), t.instr, "collection", 880_000);
    let r = t.row("전래놀이", "instructor");
    assert_eq!((r.status.as_str(), r.comparable_expense, r.excluded_expense), ("ok", 880_000, 0));
}

#[test]
fn unmapped_and_unallocated_amounts_are_reported_not_zeroed() {
    let t = setup();
    let p = t.program("한자");
    t.entry(p, None, t.instr, "collection", 50_000);
    t.entry(p, Some(t.ben), t.instr, "collection", 20_000);
    t.entry(p, Some(t.ben), t.instr, "expenditure", 20_000);
    let r = t.row("한자", "instructor");
    check_identities(&r);
    assert_eq!((r.status.as_str(), r.unmapped_basis, r.comparable_basis), ("unmapped", 50_000, 20_000));
    assert!(r.excluded.iter().any(|x| x.reason == "재원 매핑 미확정" && x.basis == 50_000));
    // 18. 미배분
    t.db.write(|tx| ledger::insert(tx, &NewEntry {
        school_year_id: t.year_id, fiscal_month_id: t.sep, program_id: None, program_group_id: None, student_id: None,
        fund_type_id: Some(t.ben), amount_item_id: t.tm, flow: "collection".into(), entry_kind: "initial_collect".into(), amount: 30_000,
        stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
    })).unwrap();
    let q = t.program("미술");
    t.entry(q, Some(t.ben), t.tm, "collection", 1);
    t.entry(q, Some(t.ben), t.tm, "expenditure", 1);
    let r = t.row("미술", "textbook_material");
    assert_eq!((r.status.as_str(), r.unallocated_in_set), ("unallocated", 30_000));
    let m = t.db.read(|c| month(c, t.sep)).unwrap();
    assert_eq!(m.unallocated_total, 30_000);
    assert_eq!(m.simple_diff, m.collection_total - m.expenditure_total);
    assert_eq!(m.collection_total, m.comparable_basis + m.excluded_basis, "월 합계 항등식(징수)");
    assert_eq!(m.expenditure_total, m.comparable_expense + m.excluded_expense, "월 합계 항등식(지급)");
}

#[test]
fn month_totals_do_not_double_count_groups() {
    let t = setup();
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    let gid = t.db.write(|tx| program::group_create(tx, t.year_id, "우쿨렐레&통기타", &[uk, gt])).unwrap();
    t.entry(uk, Some(t.ben), t.instr, "collection", 300_000);
    t.entry(gt, Some(t.ben), t.instr, "collection", 200_000);
    t.db.write(|tx| ledger::insert(tx, &NewEntry {
        school_year_id: t.year_id, fiscal_month_id: t.sep, program_id: None, program_group_id: Some(gid), student_id: None,
        fund_type_id: Some(t.ben), amount_item_id: t.instr, flow: "expenditure".into(), entry_kind: "instructor_pay".into(), amount: 500_000,
        stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
    })).unwrap();
    let m = t.db.read(|c| month(c, t.sep)).unwrap();
    assert_eq!((m.collection_total, m.expenditure_total), (500_000, 500_000));
    assert_eq!((m.comparable_basis, m.comparable_expense), (500_000, 500_000));
    assert_eq!(m.collection_total, m.comparable_basis + m.excluded_basis);
    assert_eq!(m.expenditure_total, m.comparable_expense + m.excluded_expense);
}
