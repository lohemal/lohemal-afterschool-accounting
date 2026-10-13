//! P7-1.1 통합 프로그램 기준 대조 시험 (지시서 14장 C·D). 가명·가상 금액만 쓴다.

use super::*;
use crate::db::Db;
use crate::model::{LinkItem, NewEntry};
use crate::repo::{adjustment, catalog, ledger, program, program_link, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    fund_ben: i64,
    fund_voucher: i64,
    fund_free: i64,
    item_instructor: i64,
    item_tm: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let sep = db.read(|c| year::months(c, y.id)).unwrap().iter().find(|m| m.ym == "2026-09").unwrap().id;
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    T { db, year_id: y.id, sep, fund_ben: fund("beneficiary"), fund_voucher: fund("after_voucher"), fund_free: fund("free_voucher"), item_instructor: item("instructor"), item_tm: item("textbook_material") }
}

impl T {
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    fn entry(&self, program: i64, fund: i64, item: i64, flow: &str, kind: &str, amount: i64, party: &str) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: self.sep, program_id: Some(program), program_group_id: None, student_id: None,
                    fund_type_id: Some(fund), amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None,
                    counterparty: Some(party.into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn collect(&self, program: i64, item: i64, amount: i64) -> i64 {
        self.entry(program, self.fund_ben, item, "collection", "initial_collect", amount, "가명")
    }
    fn pay(&self, program: i64, item: i64, amount: i64, party: &str) -> i64 {
        self.entry(program, self.fund_ben, item, "expenditure", if item == self.item_instructor { "instructor_pay" } else { "vendor_pay" }, amount, party)
    }
    fn link(&self, child: i64, parent: i64) {
        self.db.write(|tx| program_link::apply(tx, self.year_id, &[LinkItem { program_id: child, parent_id: Some(parent), expected_known: false, expected_parent_id: None }], "시험", "suggested")).unwrap();
    }
    fn recon(&self) -> MonthRecon {
        self.db.read(|c| month(c, self.sep)).unwrap()
    }
    fn row<'a>(&self, r: &'a MonthRecon, name: &str, set: &str) -> &'a ReconRow {
        r.rows.iter().find(|x| x.unit_name == name && x.item_set == set).unwrap_or_else(|| panic!("row {name}/{set} not found: {:?}", r.rows.iter().map(|x| (x.unit_name.clone(), x.item_set.clone(), x.status.clone())).collect::<Vec<_>>()))
    }
}

/// 로봇과학A·B 징수 + 로봇과학 강사료·교재비 지급 (지시서 7-2 의 예시 구조)
fn robot(t: &T) -> (i64, i64, i64) {
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    let b = t.program("로봇과학B");
    t.collect(a, t.item_instructor, 500_000);
    t.collect(b, t.item_instructor, 400_000);
    t.collect(a, t.item_tm, 350_000);
    t.collect(b, t.item_tm, 250_000);
    t.pay(robot, t.item_instructor, 900_000, "강사 가명A");
    t.pay(robot, t.item_tm, 600_000, "업체 가명B");
    (robot, a, b)
}

// ── D26·D30. 매핑 확인 필요 → 확정 후 정상 ───────────────────────────────

#[test]
fn d26_d30_pending_suggestion_is_not_judged_until_confirmed() {
    let t = setup();
    let (robot, a, b) = robot(&t);
    let r = t.recon();
    // 확정 전: 세부반·통합 모두 '매핑 확인 필요', 원본 금액은 그대로 보이고 차액은 판정하지 않는다
    for name in ["로봇과학", "로봇과학A", "로봇과학B"] {
        for set in ["instructor", "textbook_material"] {
            let row = t.row(&r, name, set);
            assert_eq!(row.status, "link_pending", "{name}/{set}");
            assert_eq!(row.status_label, "매핑 확인 필요");
            assert!(row.link_pending && !row.link_hint.is_empty());
        }
    }
    let rr = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((rr.basis, rr.expense), (0, 600_000), "원본 지급액을 숨기거나 0 으로 바꾸지 않는다");
    assert!(rr.link_hint.contains("'로봇과학A'") && rr.link_hint.contains("'로봇과학B'"));
    assert_eq!(t.row(&r, "로봇과학A", "instructor").basis, 500_000);
    assert_eq!(r.link_pending_count, 6);
    assert_eq!(r.todo_count, 6);
    assert_eq!(r.rows.len(), 6);
    // 확정(일괄) → 통합 행만 남고 세부반은 펼침 줄로
    t.db.write(|tx| program_link::apply(tx, t.year_id, &[LinkItem { program_id: a, parent_id: Some(robot), expected_known: false, expected_parent_id: None }, LinkItem { program_id: b, parent_id: Some(robot), expected_known: false, expected_parent_id: None }], "", "suggested")).unwrap();
    let r = t.recon();
    assert_eq!(r.link_pending_count, 0);
    assert_eq!(r.rows.len(), 2, "{:?}", r.rows.iter().map(|x| (x.unit_name.clone(), x.item_set.clone())).collect::<Vec<_>>());
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!((ri.status.as_str(), ri.basis, ri.expense, ri.diff), ("ok", 900_000, 900_000, 0));
    assert!(ri.is_parent);
    assert_eq!(ri.children.iter().map(|c| (c.program_name.as_str(), c.basis, c.expense)).collect::<Vec<_>>(), vec![("로봇과학A", 500_000, 0), ("로봇과학B", 400_000, 0)]);
    let rt = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((rt.status.as_str(), rt.basis, rt.expense, rt.diff), ("ok", 600_000, 600_000, 0));
    assert_eq!(r.comparable_basis, 1_500_000);
    assert_eq!(r.comparable_expense, 1_500_000);
    assert_eq!(r.collection_total, 1_500_000, "세부반 징수는 한 번만 센다");
    // E36. Excel 내보내기 — 통합 행마다 재원 줄 1 + 세부반 줄 2
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("월별검증.xlsx");
    let x = t.db.read(|c| crate::ops::export::recon_month(c, tmp.path(), t.sep, &crate::model::ReconExportFilter::default(), &out)).unwrap();
    assert_eq!(x.rows, 6, "{:?}", x);
    assert!(out.exists());
}

// ── C15~C18. 합산 · 항목 분리 ───────────────────────────────────────────

#[test]
fn c15_c18_sums_classes_per_item_without_mixing_items() {
    let t = setup();
    let (robot, a, b) = robot(&t);
    t.link(a, robot);
    t.link(b, robot);
    // 강사료 지급을 줄이면 강사료만 차액, 교재비는 그대로 정상
    let r = t.recon();
    assert_eq!(t.row(&r, "로봇과학", "instructor").status, "ok");
    t.pay(robot, t.item_instructor, 30_000, "강사 가명A");
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!((ri.status.as_str(), ri.diff), ("mismatch", -30_000));
    assert_eq!(ri.status_label, "차액 확인 필요");
    assert_eq!(t.row(&r, "로봇과학", "textbook_material").status, "ok", "강사료와 교재·재료비를 섞지 않는다");
    assert_eq!(r.mismatch_count, 1);
}

// ── C19~C21. 여러 강사·업체 · 중복 집계 방지 ─────────────────────────────

#[test]
fn c19_c21_multiple_payees_each_once() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    let b = t.program("로봇과학B");
    t.link(a, robot);
    t.link(b, robot);
    t.collect(a, t.item_instructor, 600_000);
    t.collect(b, t.item_instructor, 400_000);
    let p1 = t.pay(robot, t.item_instructor, 600_000, "강사 가명A");
    let p2 = t.pay(robot, t.item_instructor, 400_000, "강사 가명B");
    t.collect(a, t.item_tm, 300_000);
    t.pay(robot, t.item_tm, 200_000, "업체 가명C");
    t.pay(robot, t.item_tm, 100_000, "업체 가명D");
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!((ri.status.as_str(), ri.expense), ("ok", 1_000_000));
    let rt = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((rt.status.as_str(), rt.expense), ("ok", 300_000));
    // 상세: 지급 거래는 각각 보존되고 한 번씩만 나온다
    let d = t.db.read(|c| detail(c, t.sep, "program", robot, "instructor")).unwrap();
    assert_eq!(d.expenditure.iter().map(|e| e.entry_id).collect::<Vec<_>>(), vec![p1, p2]);
    assert_eq!(d.collection.len(), 2);
    assert_eq!(d.members, vec!["로봇과학", "로봇과학A", "로봇과학B"]);
    assert_eq!(d.parties.iter().map(|p| (p.name.as_str(), p.total)).collect::<Vec<_>>(), vec![("가명", 1_000_000), ("강사 가명A", 600_000), ("강사 가명B", 400_000)]);
    // 월 합계: 지급 1,300,000 · 징수 1,300,000 — 세부반을 다시 더하지 않는다
    assert_eq!((r.collection_total, r.expenditure_total), (1_300_000, 1_300_000));
    assert_eq!(r.rows.len(), 2);
    // 세부반에 직접 붙은 지급은 통합 지급액에 한 번 들고 단서가 붙는다
    t.pay(a, t.item_instructor, 10_000, "강사 가명E");
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!(ri.expense, 1_010_000);
    assert!(ri.hints.iter().any(|h| h.contains("'로봇과학A' 에 직접 연결된 지급")));
    assert_eq!(ri.children.iter().find(|c| c.program_name == "로봇과학A").unwrap().expense, 10_000);
}

// ── C22~C25. 환불 · 추가징수 · 재원경정 · 지원금 ─────────────────────────

#[test]
fn c22_c25_refund_extra_transfer_and_voucher_follow_existing_rules() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    let b = t.program("로봇과학B");
    t.link(a, robot);
    t.link(b, robot);
    t.collect(a, t.item_instructor, 500_000);
    t.collect(b, t.item_instructor, 400_000);
    // 환불(음수) · 추가징수 는 원장 부호 그대로
    t.entry(a, t.fund_ben, t.item_instructor, "collection", "refund", -50_000, "가명");
    t.entry(b, t.fund_ben, t.item_instructor, "collection", "extra_collect", 20_000, "가명");
    // 지원금 재원 (이용권·자유수강권) 은 재원별로 따로 대조
    t.entry(a, t.fund_voucher, t.item_instructor, "collection", "initial_collect", 100_000, "가명");
    t.entry(b, t.fund_free, t.item_instructor, "collection", "initial_collect", 30_000, "가명");
    t.pay(robot, t.item_instructor, 870_000, "강사 가명A");
    t.entry(robot, t.fund_voucher, t.item_instructor, "expenditure", "instructor_pay", 100_000, "강사 가명A");
    t.entry(robot, t.fund_free, t.item_instructor, "expenditure", "instructor_pay", 30_000, "강사 가명A");
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!((ri.initial, ri.extra, ri.refund), (1_030_000, 20_000, -50_000));
    assert_eq!((ri.status.as_str(), ri.basis, ri.expense, ri.diff), ("ok", 1_000_000, 1_000_000, 0));
    let by: Vec<(String, Option<i64>, Option<i64>)> = ri.funds.iter().map(|f| (f.fund_name.clone(), f.basis, f.expense)).collect();
    assert!(by.contains(&("방과후이용권".into(), Some(100_000), Some(100_000))));
    assert!(by.contains(&("자유수강권".into(), Some(30_000), Some(30_000))));
    let ca = ri.children.iter().find(|c| c.program_name == "로봇과학A").unwrap();
    assert_eq!((ca.initial, ca.refund, ca.basis), (600_000, -50_000, 550_000));
    // 재원경정(세부반 단위, 쌍의 합 0): 전체 기준액 불변, 재원별 이동
    let item = t.item_instructor;
    let input = crate::model::AdjustmentInput {
        school_year_id: t.year_id, fiscal_month_id: t.sep, kind: "fund_transfer".into(), program_id: a, student_id: None,
        from_fund_id: Some(t.fund_ben), to_fund_id: Some(t.fund_voucher), fund_type_id: None, amount_item_id: item, amount: 10_000,
        decided_on: Some("2026-10-13".into()), processed_on: Some("2026-10-13".into()), status: "done".into(),
        reason: "시험".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
    };
    t.db.write(|tx| adjustment::create(tx, &input)).unwrap();
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!(ri.basis, 1_000_000, "재원경정은 총징수액을 바꾸지 않는다");
    assert_eq!(ri.transfer_count, 2);
    assert_eq!(ri.status, "mismatch");
    assert!(ri.total_match_fund_mismatch, "총액 일치 · 재원 불일치");
}

// ── D27~D29. 자료 확인 필요 · 정상 · 차액 ───────────────────────────────

#[test]
fn d27_d29_states_after_confirmation() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    t.link(a, robot);
    t.collect(a, t.item_instructor, 500_000);
    // 지급 자료 누락 → 자료 확인 필요 (0원으로 보지 않음)
    let r = t.recon();
    let ri = t.row(&r, "로봇과학", "instructor");
    assert_eq!((ri.status.as_str(), ri.status_label.as_str()), ("missing", "자료 확인 필요"));
    assert!(ri.is_parent && ri.children.len() == 1);
    t.pay(robot, t.item_instructor, 500_000, "강사 가명A");
    assert_eq!(t.row(&t.recon(), "로봇과학", "instructor").status, "ok");
    t.pay(robot, t.item_instructor, 1, "강사 가명A");
    assert_eq!(t.row(&t.recon(), "로봇과학", "instructor").status, "mismatch");
    // 세부반에 징수가 없고 통합에 직접 징수만 있으면 직접 귀속 줄로 보인다
    let coding = t.program("코딩");
    let ca = t.program("코딩A");
    t.link(ca, coding);
    t.collect(coding, t.item_instructor, 70_000);
    t.pay(coding, t.item_instructor, 70_000, "강사 가명B");
    let r = t.recon();
    let rc = t.row(&r, "코딩", "instructor");
    assert_eq!(rc.status, "ok");
    assert_eq!(rc.children.len(), 1);
    assert!(rc.children[0].direct && rc.children[0].basis == 70_000);
}

// ── 그룹과 함께 · 미배분 배분 · 확인 메모 재확인 ───────────────────────

#[test]
fn umbrella_inside_group_and_allocations_and_note_recheck() {
    let t = setup();
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    let uka = t.program("우쿨렐레A");
    t.link(uka, uk);
    let g = t.db.write(|tx| program::group_create(tx, t.year_id, "우쿨렐레&통기타", &[uk, gt])).unwrap();
    t.collect(uka, t.item_instructor, 300_000);
    t.collect(gt, t.item_instructor, 200_000);
    t.db.write(|tx| ledger::insert(tx, &NewEntry { school_year_id: t.year_id, fiscal_month_id: t.sep, program_id: None, program_group_id: Some(g), student_id: None, fund_type_id: Some(t.fund_ben), amount_item_id: t.item_instructor, flow: "expenditure".into(), entry_kind: "instructor_pay".into(), amount: 500_000, stage: None, processed_on: None, counterparty: Some("강사".into()), upload_file_id: None, source_row_id: None, reason: None })).unwrap();
    let r = t.recon();
    let rg = t.row(&r, "우쿨렐레&통기타 (그룹)", "instructor");
    assert_eq!((rg.status.as_str(), rg.basis, rg.expense), ("ok", 500_000, 500_000), "그룹은 통합 프로그램(세부반 합산)을 구성원으로 받는다");
    assert_eq!(t.row(&r, "우쿨렐레", "instructor").status, "group_only");
    assert!(!r.rows.iter().any(|x| x.unit_name == "우쿨렐레A"));
    // 미배분 징수를 세부반에 배분하면 통합 행에 든다
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    t.link(a, robot);
    let e = t.db.write(|tx| ledger::insert(tx, &NewEntry { school_year_id: t.year_id, fiscal_month_id: t.sep, program_id: None, program_group_id: None, student_id: None, fund_type_id: Some(t.fund_ben), amount_item_id: t.item_tm, flow: "collection".into(), entry_kind: "initial_collect".into(), amount: 90_000, stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None })).unwrap();
    t.db.write(|tx| crate::repo::allocation::manual(tx, e.id, &[(a, 90_000)], "시험")).unwrap();
    t.pay(robot, t.item_tm, 90_000, "업체");
    let r = t.recon();
    let rt = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((rt.status.as_str(), rt.allocated_in, rt.basis), ("ok", 90_000, 90_000));
    // 확인 메모 뒤 연결을 바꾸면 '재확인 필요'
    let n = t.db.write(|tx| note_save(tx, t.sep, "program", robot, "textbook_material", "explained", "확인")).unwrap();
    assert!(!n.fingerprint.is_empty());
    assert!(!t.row(&t.recon(), "로봇과학", "textbook_material").needs_recheck);
    let b = t.program("로봇과학B");
    t.collect(b, t.item_tm, 5_000);
    t.link(b, robot);
    assert!(t.row(&t.recon(), "로봇과학", "textbook_material").needs_recheck);
}
