//! P4 시험 (지시서 A~H). 가명·가상 금액만.

use super::*;
use crate::db::Db;
use crate::excel::parse::StudentKey;
use crate::model::NewEntry;
use crate::repo::{catalog, ledger, program, student, year};

#[allow(dead_code)]
struct T {
    db: Db,
    year_id: i64,
    months: Vec<i64>,
    ben: i64,
    g3x: i64,
    voucher: i64,
    free: i64,
    custom: i64,
    instr: i64,
    textbook: i64,
    coding: i64,
    art: i64,
    stu: i64,
    stu2: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months: Vec<i64> = db.read(|c| year::months(c, y.id)).unwrap().iter().map(|m| m.id).collect();
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    let coding = db.write(|tx| program::create(tx, y.id, "코딩")).unwrap();
    let art = db.write(|tx| program::create(tx, y.id, "미술")).unwrap();
    let stu = db.write(|tx| student::upsert(tx, y.id, &StudentKey { grade: Some(1), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() })).unwrap().unwrap();
    let stu2 = db.write(|tx| student::upsert(tx, y.id, &StudentKey { grade: Some(2), class_name: "나래".into(), student_no: "5".into(), name: "가명B".into() })).unwrap().unwrap();
    T { db, year_id: y.id, months, ben: fund("beneficiary"), g3x: fund("g3_excess"), voucher: fund("after_voucher"), free: fund("free_voucher"), custom: fund("custom_support"), instr: item("instructor"), textbook: item("textbook_material"), coding, art, stu, stu2 }
}

impl T {
    /// 차례(1=3월)로 회계월 id
    fn m(&self, seq: usize) -> i64 {
        self.months[seq - 1]
    }
    fn collect_for(&self, stu: i64, seq: usize, program: i64, fund: i64, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: self.m(seq), program_id: Some(program), program_group_id: None, student_id: Some(stu),
                    fund_type_id: Some(fund), amount_item_id: self.instr, flow: "collection".into(), entry_kind: "initial_collect".into(), amount,
                    stage: None, processed_on: Some("2026-04-10".into()), counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn collect(&self, seq: usize, fund: i64, amount: i64) -> i64 {
        self.collect_for(self.stu, seq, self.coding, fund, amount)
    }
    fn policy(&self, basis: &str, annual: Option<i64>, s1: Option<i64>, s2: Option<i64>, carry: bool) -> VoucherPolicy {
        self.db.write(|tx| policy_save(tx, &VoucherPolicyInput { school_year_id: self.year_id, basis: basis.into(), annual_limit: annual, semester1_limit: s1, semester2_limit: s2, carry_over: carry, ..Default::default() })).unwrap()
    }
    fn target_input(&self, stu: i64, status: &str, from: &str) -> VoucherTargetInput {
        VoucherTargetInput { school_year_id: self.year_id, student_id: stu, status: status.into(), confirmed_on: Some("2026-05-10".into()), apply_from: Some(from.into()), apply_to: None, transfer_in: false, transfer_in_on: None, note: String::new(), reason: "시험".into() }
    }
    fn confirm(&self, stu: i64, from: &str) -> VoucherTarget {
        self.db.write(|tx| target_save(tx, &self.target_input(stu, "confirmed", from))).unwrap()
    }
    fn usage(&self, stu: i64) -> VoucherUsage {
        self.db.read(|c| usage(c, self.year_id, stu, &[])).unwrap()
    }
    fn preview(&self, stu: i64) -> VoucherRetroPreview {
        self.db.read(|c| retro_preview(c, self.year_id, &[stu], &[])).unwrap()
    }
    fn apply_normal(&self, p: &VoucherRetroPreview, kind: &str, status: &str) -> AppResult<VoucherBatch> {
        let rows: Vec<VoucherApplyRow> = p.proposals.iter().filter(|x| x.state == "normal").map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
        self.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: self.year_id, kind: kind.into(), reason: "시험 소급".into(), decided_on: Some("2026-05-10".into()), processed_on: Some("2026-05-15".into()), status: status.into(), rows, decisions: vec![] }))
    }
    fn free_actual(&self, seq: usize) -> i64 {
        self.db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT coalesce(sum(amount), 0) FROM ledger_entry WHERE fiscal_month_id = ?1 AND student_id = ?2 AND fund_type_id = ?3 AND state = 'active' AND stage = 'actual'",
                    params![self.m(seq), self.stu, self.free],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
}

fn row<'a>(u: &'a VoucherUsage, seq: i32, program: i64) -> &'a VoucherUsageRow {
    u.rows.iter().find(|r| r.seq == seq && r.program_id == program).expect("row")
}

// ── A. 정책 ─────────────────────────────────────────────────────────────

#[test]
fn a_policy_kinds_history_and_validation() {
    let t = setup();
    assert!(t.db.read(|c| policy_current(c, t.year_id)).unwrap().is_none());
    // 1. 연간
    let p = t.policy("annual", Some(600_000), None, None, false);
    assert_eq!((p.basis.as_str(), p.annual_limit), ("annual", Some(600_000)));
    // 2·3. 학기별 이월 없음 / 있음
    let p = t.policy("semester", None, Some(300_000), Some(300_000), false);
    assert!(!p.carry_over);
    let p = t.policy("semester", None, Some(300_000), Some(300_000), true);
    assert!(p.carry_over);
    // 4. 제한 없음
    let p = t.policy("unlimited", None, None, None, false);
    assert_eq!(p.limit_label_check(), "제한 없음");
    // 5. 이력: 네 번 저장 → 네 행, 현재는 마지막 하나
    let h = t.db.read(|c| policy_history(c, t.year_id)).unwrap();
    assert_eq!(h.len(), 4);
    assert_eq!(h.iter().filter(|x| x.superseded_at.is_none()).count(), 1);
    assert_eq!(t.db.read(|c| policy_current(c, t.year_id)).unwrap().unwrap().basis, "unlimited");
    // 7. 검증: 음수·누락·잘못된 기준
    let bad = |basis: &str, a: Option<i64>, s1: Option<i64>, s2: Option<i64>| t.db.write(|tx| policy_save(tx, &VoucherPolicyInput { school_year_id: t.year_id, basis: basis.into(), annual_limit: a, semester1_limit: s1, semester2_limit: s2, carry_over: false, ..Default::default() })).unwrap_err().code;
    assert_eq!(bad("annual", None, None, None), "INVALID");
    assert_eq!(bad("annual", Some(-1), None, None), "INVALID");
    assert_eq!(bad("semester", None, Some(1), None), "INVALID");
    assert_eq!(bad("weekly", None, None, None), "INVALID");
    assert_eq!(t.db.read(|c| policy_history(c, t.year_id)).unwrap().len(), 4, "실패한 저장은 이력에 남지 않는다");
}

impl VoucherPolicy {
    fn limit_label_check(&self) -> String {
        super::limit_label(&self.basis, self.annual_limit, self.semester1_limit, self.semester2_limit, self.carry_over)
    }
}

#[test]
fn a_student_exception_overrides_policy_and_can_be_ended() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.confirm(t.stu, "2026-03-01");
    assert_eq!(t.usage(t.stu).limit_source, "학년도 정책");
    // 6. 학생 예외
    let x = t.db.write(|tx| exception_save(tx, &VoucherExceptionInput { school_year_id: t.year_id, student_id: t.stu, basis: "annual".into(), annual_limit: Some(900_000), semester1_limit: None, semester2_limit: None, carry_over: false, reason: "개별 승인".into() })).unwrap();
    let u = t.usage(t.stu);
    assert_eq!(u.limit_source, "학생 예외");
    assert_eq!(u.limit_total, Some(900_000));
    assert!(t.db.read(|c| target_list(c, t.year_id)).unwrap()[0].has_exception);
    // 다른 학생은 정책 그대로
    t.confirm(t.stu2, "2026-03-01");
    assert_eq!(t.usage(t.stu2).limit_total, Some(600_000));
    // 사유 없는 예외 거부
    assert_eq!(t.db.write(|tx| exception_save(tx, &VoucherExceptionInput { school_year_id: t.year_id, student_id: t.stu, basis: "unlimited".into(), annual_limit: None, semester1_limit: None, semester2_limit: None, carry_over: false, reason: " ".into() })).unwrap_err().code, "INVALID");
    // 8. 해제 → 정책 복귀, 이력 유지
    assert_eq!(t.db.write(|tx| exception_end(tx, x.id, "")).unwrap_err().code, "INVALID");
    t.db.write(|tx| exception_end(tx, x.id, "승인 종료")).unwrap();
    assert_eq!(t.usage(t.stu).limit_source, "학년도 정책");
    assert_eq!(t.db.read(|c| exception_list(c, t.year_id, true)).unwrap().len(), 1);
    assert_eq!(t.db.read(|c| exception_list(c, t.year_id, false)).unwrap().len(), 0);
}

// ── B. 대상자 ───────────────────────────────────────────────────────────

#[test]
fn b_target_register_status_dates_and_history() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    // 9. 확인 전 등록
    let tg = t.db.write(|tx| target_save(tx, &VoucherTargetInput { school_year_id: t.year_id, student_id: t.stu, status: "pending".into(), confirmed_on: None, apply_from: None, apply_to: None, transfer_in: false, transfer_in_on: None, note: "신청서 접수".into(), reason: String::new() })).unwrap();
    assert_eq!(tg.status_label, "확인 전");
    assert_eq!(tg.logs.len(), 1);
    assert!(t.usage(t.stu).blocked.iter().any(|b| b.contains("확인 전")));
    // 10. 확정 — 확정일·적용 시작일 필수, 상태 변경 사유 필수
    let mut bad = t.target_input(t.stu, "confirmed", "2026-03-01");
    bad.apply_from = None;
    assert_eq!(t.db.write(|tx| target_save(tx, &bad)).unwrap_err().code, "INVALID");
    let mut no_reason = t.target_input(t.stu, "confirmed", "2026-03-01");
    no_reason.reason = String::new();
    assert_eq!(t.db.write(|tx| target_save(tx, &no_reason)).unwrap_err().code, "INVALID");
    let tg = t.confirm(t.stu, "2026-03-01");
    assert_eq!((tg.status.as_str(), tg.apply_from.as_deref()), ("confirmed", Some("2026-03-01")));
    assert_eq!(tg.logs.len(), 2);
    assert_eq!(tg.logs[1].action, "status");
    // 학년도 밖 날짜·순서 오류
    let mut out = t.target_input(t.stu, "confirmed", "2025-12-01");
    out.note = "x".into();
    assert_eq!(t.db.write(|tx| target_save(tx, &out)).unwrap_err().code, "INVALID");
    let mut rev = t.target_input(t.stu, "ended", "2026-06-01");
    rev.apply_to = Some("2026-05-01".into());
    assert_eq!(t.db.write(|tx| target_save(tx, &rev)).unwrap_err().code, "INVALID");
    // 11. 지원 종료 — 적용 종료일
    let mut end = t.target_input(t.stu, "ended", "2026-03-01");
    end.apply_to = Some("2026-06-30".into());
    let tg = t.db.write(|tx| target_save(tx, &end)).unwrap();
    assert_eq!(tg.status_label, "지원 종료");
    t.collect(4, t.ben, 30_000);
    t.collect(5, t.ben, 30_000);
    let u = t.usage(t.stu);
    assert_eq!(row(&u, 4, t.coding).state, "retro", "6월까지는 적용");
    assert_eq!(row(&u, 5, t.coding).state, "outside", "7월은 적용 기간 밖");
    // 12. 대상 취소 → 계산 차단, 이력 유지
    let mut cancel = t.target_input(t.stu, "cancelled", "2026-03-01");
    cancel.apply_to = Some("2026-06-30".into());
    let tg = t.db.write(|tx| target_save(tx, &cancel)).unwrap();
    assert_eq!(tg.logs.len(), 4);
    assert!(t.usage(t.stu).blocked.iter().any(|b| b.contains("취소")));
    // 13. 같은 내용 재저장 거부 · 14. 학생당 한 행
    assert_eq!(t.db.write(|tx| target_save(tx, &cancel)).unwrap_err().code, "INVALID");
    assert_eq!(t.db.read(|c| target_list(c, t.year_id)).unwrap().len(), 1);
    // 지우기 금지
    let err = t.db.write(|tx| Ok(tx.execute("DELETE FROM voucher_target WHERE id = ?1", [tg.id])?)).unwrap_err();
    assert!(err.detail.unwrap_or_default().contains("VOUCHER_NO_DELETE") || err.message.contains("VOUCHER_NO_DELETE"));
}

#[test]
fn b_student_can_be_registered_manually_without_duplicates() {
    let t = setup();
    let s = t.db.write(|tx| student_create(tx, &StudentCreateInput { school_year_id: t.year_id, grade: 3, class_name: "다솜".into(), student_no: "7".into(), name: "가명C".into() })).unwrap();
    assert!(s.label.contains("가명C"));
    let again = t.db.write(|tx| student_create(tx, &StudentCreateInput { school_year_id: t.year_id, grade: 3, class_name: "다솜".into(), student_no: "7".into(), name: "가명C".into() })).unwrap();
    assert_eq!(s.id, again.id, "네 가지가 같으면 같은 학생");
    assert_eq!(t.db.write(|tx| student_create(tx, &StudentCreateInput { school_year_id: t.year_id, grade: 9, class_name: "".into(), student_no: "".into(), name: "가명D".into() })).unwrap_err().code, "INVALID");
}

// ── C. 사용액 계산 ────────────────────────────────────────────────────────

#[test]
fn c_usage_invariant_and_exhaustion_scenario_2() {
    let t = setup();
    t.policy("annual", Some(300_000), None, None, false);
    t.confirm(t.stu, "2026-03-01");
    // 3~9월 자유수강권 40,000 ×7 = 280,000 실제 사용, 10월 수강료 50,000 은 수익자
    for seq in 1..=7 {
        t.collect(seq, t.free, 40_000);
    }
    t.collect(8, t.ben, 50_000);
    let u = t.usage(t.stu);
    assert!(u.blocked.is_empty(), "{:?}", u.blocked);
    assert_eq!(u.used_actual, 280_000);
    let oct = row(&u, 8, t.coding);
    assert_eq!((oct.tuition, oct.free_expected, oct.beneficiary_expected), (50_000, 20_000, 30_000));
    assert_eq!(oct.remaining_after, Some(0));
    assert_eq!(u.remaining, Some(0));
    assert_eq!(u.exhausted_ym.as_deref(), Some("2026-10"));
    assert_eq!(oct.state, "retro");
    assert_eq!(oct.delta, 20_000);
    // 불변식: 수강료 = 자유 + 수익자 + 기타
    for r in &u.rows {
        assert_eq!(r.tuition, r.free_actual + r.beneficiary_actual + r.other_actual);
    }
    // 경정 등록 뒤 실제 = 계산
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (1, 20_000));
    t.apply_normal(&p, "monthly", "done").unwrap();
    let u = t.usage(t.stu);
    let oct = row(&u, 8, t.coding);
    assert_eq!((oct.free_actual, oct.beneficiary_actual, oct.state.as_str()), (20_000, 30_000, "ok"));
    assert_eq!(u.used_actual, 300_000);
    assert_eq!(u.remaining, Some(0));
    assert_eq!(t.free_actual(8), 20_000);
}

#[test]
fn c_unlimited_and_semester_carry_over_and_item_separation() {
    let t = setup();
    // 시나리오 3: 제한 없음 → 전환 없음
    t.policy("unlimited", None, None, None, false);
    t.confirm(t.stu, "2026-03-01");
    t.collect(1, t.free, 900_000);
    t.collect(2, t.free, 900_000);
    let u = t.usage(t.stu);
    assert!(u.rows.iter().all(|r| r.state == "ok" && r.free_expected == r.tuition));
    assert_eq!((u.limit_total, u.remaining), (None, None));
    assert!(t.preview(t.stu).proposals.is_empty());
    // 학기별 이월: 1학기 100,000 씀, 2학기 250,000 수강료 (수익자)
    t.policy("semester", None, Some(200_000), Some(200_000), true);
    t.confirm(t.stu2, "2026-03-01");
    t.collect_for(t.stu2, 1, t.coding, t.free, 100_000);
    t.collect_for(t.stu2, 7, t.coding, t.ben, 250_000);
    let u = t.usage(t.stu2);
    assert_eq!(row(&u, 7, t.coding).free_expected, 250_000);
    assert_eq!(u.remaining, Some(50_000));
    assert_eq!(u.remaining_s1, Some(100_000));
    // 이월 없음이면 200,000 까지만
    t.policy("semester", None, Some(200_000), Some(200_000), false);
    let u = t.usage(t.stu2);
    assert_eq!(row(&u, 7, t.coding).free_expected, 200_000);
    assert_eq!(u.remaining, Some(0));
    // 항목(강사료·교재비)은 따로 칸이 된다
    t.db.write(|tx| {
        ledger::insert(tx, &NewEntry {
            school_year_id: t.year_id, fiscal_month_id: t.m(7), program_id: Some(t.coding), program_group_id: None, student_id: Some(t.stu2),
            fund_type_id: Some(t.ben), amount_item_id: t.textbook, flow: "collection".into(), entry_kind: "initial_collect".into(), amount: 10_000,
            stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
        }).map(|_| ())
    }).unwrap();
    let u = t.usage(t.stu2);
    assert_eq!(u.rows.iter().filter(|r| r.seq == 7).count(), 2);
}

#[test]
fn c_two_programs_same_month_need_user_decision_scenario_5() {
    let t = setup();
    t.policy("annual", Some(50_000), None, None, false);
    t.confirm(t.stu, "2026-03-01");
    t.collect_for(t.stu, 3, t.art, t.ben, 30_000);
    t.collect_for(t.stu, 3, t.coding, t.ben, 40_000);
    let u = t.usage(t.stu);
    assert!(u.needs_decision);
    assert_eq!(u.decision_months, vec!["5월 (2026-05)".to_string()]);
    assert!(u.rows.iter().all(|r| r.state == "decide"));
    assert_eq!(u.used_expected, 50_000, "임시값 합은 한도");
    let p = t.preview(t.stu);
    assert_eq!(p.normal_count, 0, "배분을 정하기 전에는 등록 가능한 행이 없다");
    assert!(p.proposals.iter().all(|x| x.state == "decide"));
    // 담당자 배분: 미술 10,000 · 코딩 40,000 → 자유 50,000 / 수익자 20,000
    let d = vec![
        VoucherDecision { student_id: t.stu, fiscal_month_id: t.m(3), program_id: t.art, amount_item_id: t.instr, free_amount: 10_000 },
        VoucherDecision { student_id: t.stu, fiscal_month_id: t.m(3), program_id: t.coding, amount_item_id: t.instr, free_amount: 40_000 },
    ];
    let u = t.db.read(|c| usage(c, t.year_id, t.stu, &d)).unwrap();
    assert!(!u.needs_decision);
    assert_eq!(row(&u, 3, t.art).free_expected, 10_000);
    assert_eq!(row(&u, 3, t.coding).free_expected, 40_000);
    assert_eq!(u.rows.iter().map(|r| r.beneficiary_expected).sum::<i64>(), 20_000);
    let p = t.db.read(|c| retro_preview(c, t.year_id, &[t.stu], &d)).unwrap();
    assert_eq!((p.normal_count, p.normal_total), (2, 50_000));
    // 배분 없이 등록하려 하면 재검증에서 막힌다 (전부 되돌림)
    let rows: Vec<VoucherApplyRow> = p.proposals.iter().map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
    let err = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "시험".into(), decided_on: None, processed_on: Some("2026-05-20".into()), status: "done".into(), rows: rows.clone(), decisions: vec![] })).unwrap_err();
    assert!(err.message.contains("배분 결정 필요"), "{}", err.message);
    assert_eq!(t.db.read(|c| batch_list(c, t.year_id)).unwrap().len(), 0);
    // 배분과 함께 등록 → 총 70,000 = 자유 50,000 + 수익자 20,000
    t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "시험".into(), decided_on: None, processed_on: Some("2026-05-20".into()), status: "done".into(), rows, decisions: d.clone() })).unwrap();
    let u = t.db.read(|c| usage(c, t.year_id, t.stu, &d)).unwrap();
    assert_eq!(u.used_actual, 50_000);
    assert_eq!(u.rows.iter().map(|r| r.tuition).sum::<i64>(), 70_000);
    assert_eq!(u.rows.iter().map(|r| r.beneficiary_actual).sum::<i64>(), 20_000);
    assert!(u.rows.iter().all(|r| r.state == "ok"));
}

#[test]
fn c_over_usage_is_flagged_not_reversed_and_unallocated_is_warned() {
    let t = setup();
    t.policy("annual", Some(100_000), None, None, false);
    t.confirm(t.stu, "2026-04-01");
    t.collect(1, t.free, 50_000); // 적용 기간 전 사용
    t.collect(2, t.free, 120_000); // 한도 초과 사용
    let u = t.usage(t.stu);
    assert_eq!(row(&u, 1, t.coding).state, "over");
    let apr = row(&u, 2, t.coding);
    assert_eq!((apr.free_expected, apr.free_actual, apr.state.as_str()), (100_000, 120_000, "over"));
    assert_eq!(u.over_total, 70_000);
    let p = t.preview(t.stu);
    assert_eq!(p.normal_count, 0);
    assert!(p.proposals.iter().all(|x| x.state == "over"));
    assert!(u.warnings.iter().any(|x| x.contains("자동으로 되돌리지 않")));
    // 미배분 징수액은 계산에서 빠지고 경고
    t.db.write(|tx| {
        ledger::insert(tx, &NewEntry {
            school_year_id: t.year_id, fiscal_month_id: t.m(3), program_id: None, program_group_id: None, student_id: Some(t.stu),
            fund_type_id: Some(t.ben), amount_item_id: t.instr, flow: "collection".into(), entry_kind: "initial_collect".into(), amount: 30_000,
            stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
        }).map(|_| ())
    }).unwrap();
    let u = t.usage(t.stu);
    assert!(u.warnings.iter().any(|x| x.contains("배분되지 않은 징수액 30,000")));
    assert_eq!(u.rows.iter().filter(|r| r.seq == 3).count(), 0);
}

// ── D. 소급 지원 ────────────────────────────────────────────────────────────

#[test]
fn d_retroactive_scenario_1_transfer_refund_and_june_usage() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    for seq in 1..=4 {
        t.collect(seq, t.ben, 30_000); // 3~6월 수익자 30,000
    }
    // 5월 확정, 적용 3월
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (4, 120_000));
    assert!(p.proposals.iter().all(|x| x.from_fund_id == Some(t.ben) && x.amount == 30_000));
    // 3~5월만 소급(90,000), 6월은 당월 처리로 따로
    let retro_rows: Vec<VoucherApplyRow> = p.proposals.iter().filter(|x| x.fiscal_month_id != t.m(4)).map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
    let b = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "5월 확정 소급".into(), decided_on: Some("2026-05-10".into()), processed_on: Some("2026-05-15".into()), status: "done".into(), rows: retro_rows, decisions: vec![] })).unwrap();
    assert_eq!((b.item_count, b.total_amount, b.kind_label.as_str()), (3, 90_000, "소급 지원 경정"));
    assert_eq!(b.student_id, Some(t.stu));
    // 환불 대상 90,000 (3건) 미처리
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|x| x.status == "none" && x.outstanding == 30_000 && x.from_batch));
    assert_eq!(r.iter().map(|x| x.outstanding).sum::<i64>(), 90_000);
    // 6월 당월 사용 30,000 → 총 120,000
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (1, 30_000), "이미 자유수강권인 금액은 다시 제안하지 않는다");
    t.apply_normal(&p, "monthly", "done").unwrap();
    let u = t.usage(t.stu);
    assert_eq!(u.used_actual, 120_000);
    assert_eq!(u.remaining, Some(480_000));
    assert!(u.rows.iter().all(|r| r.state == "ok"));
    // 다시 미리보기 → 제안 없음 (중복 경정 없음)
    assert_eq!(t.preview(t.stu).normal_count, 0);
    // 원장: 재원경정 8건(4쌍), 합 0, 전체 징수 120,000 그대로
    let (n, sum): (i64, i64) = t.db.read(|c| Ok(c.query_row("SELECT count(*), coalesce(sum(amount), 0) FROM ledger_entry WHERE flow = 'transfer' AND state = 'active'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!((n, sum), (8, 0));
    let s = t.db.read(|c| ledger::month_summary(c, t.m(1))).unwrap();
    assert_eq!(s.collection_total, 30_000);
}

#[test]
fn d_retroactive_excludes_existing_voucher_pending_and_respects_limit_and_other_funds() {
    let t = setup();
    t.policy("annual", Some(100_000), None, None, false);
    t.confirm(t.stu, "2026-03-01");
    // 3월: 수익자 20,000 + 자유 10,000 (이미 일부 자유수강권) → 제안 20,000
    t.collect(1, t.ben, 20_000);
    t.collect(1, t.free, 10_000);
    // 4월: 맞춤형지원 30,000 만 → 지원 대상 수강료 0, 한도를 쓰지 않고 제안도 없다
    t.collect(2, t.custom, 30_000);
    // 5월: 수익자 50,000, 6월: 수익자 50,000 → 한도 100,000 중 남은 70,000 → 5월 50,000 · 6월 20,000
    t.collect(3, t.ben, 50_000);
    t.collect(4, t.ben, 50_000);
    let p = t.preview(t.stu);
    let normal: Vec<&VoucherProposal> = p.proposals.iter().filter(|x| x.state == "normal").collect();
    assert_eq!(normal.iter().map(|x| x.amount).sum::<i64>(), 90_000);
    assert_eq!(normal.iter().find(|x| x.fiscal_month_id == t.m(1)).unwrap().amount, 20_000);
    assert_eq!(normal.iter().find(|x| x.fiscal_month_id == t.m(4)).unwrap().amount, 20_000, "한도까지만");
    assert!(!p.proposals.iter().any(|x| x.fiscal_month_id == t.m(2)));
    let u = t.usage(t.stu);
    let apr = row(&u, 2, t.coding);
    assert_eq!((apr.tuition, apr.other_actual, apr.free_expected, apr.state.as_str()), (30_000, 30_000, 0, "ok"));
    assert_eq!(u.remaining, Some(0));
    // 처리 예정으로 등록한 3월 경정은 다음 미리보기에서 '처리 예정' 으로 빠진다 (중복 방지)
    let rows: Vec<VoucherApplyRow> = normal.iter().filter(|x| x.fiscal_month_id == t.m(1)).map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
    t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "예정".into(), decided_on: None, processed_on: None, status: "planned".into(), rows, decisions: vec![] })).unwrap();
    let p = t.preview(t.stu);
    assert!(p.proposals.iter().any(|x| x.state == "pending" && x.fiscal_month_id == t.m(1) && x.amount == 20_000));
    assert_eq!(p.proposals.iter().filter(|x| x.state == "normal").map(|x| x.amount).sum::<i64>(), 70_000);
    let u = t.usage(t.stu);
    assert_eq!(u.pending_total, 20_000);
    assert_eq!(row(&u, 1, t.coding).state, "pending");
    // 한도를 넘는 금액을 직접 넣으면 재검증에서 막힌다
    let over = VoucherApplyRow { student_id: t.stu, fiscal_month_id: t.m(4), program_id: t.coding, amount_item_id: t.instr, from_fund_id: t.ben, amount: 30_000, confirmed: false };
    let err = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "초과".into(), decided_on: None, processed_on: Some("2026-06-01".into()), status: "done".into(), rows: vec![over], decisions: vec![] })).unwrap_err();
    assert!(err.message.contains("한도 초과"), "{}", err.message);
    // 맞춤형지원을 자유수강권으로 바꾸려는 행도 막힌다
    let other = VoucherApplyRow { student_id: t.stu, fiscal_month_id: t.m(2), program_id: t.coding, amount_item_id: t.instr, from_fund_id: t.custom, amount: 10_000, confirmed: false };
    let err = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "x".into(), decided_on: None, processed_on: Some("2026-06-01".into()), status: "done".into(), rows: vec![other], decisions: vec![] })).unwrap_err();
    assert_eq!(err.code, "INVALID");
}

#[test]
fn d_apply_start_date_after_some_months_only_counts_later_months() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    for seq in 1..=4 {
        t.collect(seq, t.ben, 30_000);
    }
    t.confirm(t.stu, "2026-05-01");
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (2, 60_000));
    let u = t.usage(t.stu);
    assert_eq!(row(&u, 1, t.coding).state, "outside");
    assert_eq!(row(&u, 3, t.coding).state, "retro");
}

// ── E. 환불 ─────────────────────────────────────────────────────────────────

#[test]
fn e_refund_requires_collection_evidence_and_tracks_status() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.collect(1, t.ben, 30_000);
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    t.apply_normal(&p, "retro", "done").unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.len(), 1);
    let tid = r[0].transfer_id;
    assert_eq!(r[0].status_label, "미처리");
    // 31. 수납 확인 없이 완료 → 거부
    let done_no_conf = VoucherRefundInput { transfer_id: tid, amount: 30_000, status: "done".into(), processed_on: Some("2026-05-20".into()), collected_confirmed: false, evidence: "수납 대장".into(), reason: "소급 환불".into() };
    assert!(t.db.write(|tx| refund_register(tx, &done_no_conf)).unwrap_err().message.contains("수납"));
    // 근거 없이 → 거부
    let no_ev = VoucherRefundInput { collected_confirmed: true, evidence: " ".into(), ..done_no_conf.clone() };
    assert_eq!(t.db.write(|tx| refund_register(tx, &no_ev)).unwrap_err().code, "INVALID");
    // 32. 처리 예정 등록 (수납 확인 불필요)
    let planned = VoucherRefundInput { transfer_id: tid, amount: 10_000, status: "planned".into(), processed_on: None, collected_confirmed: false, evidence: String::new(), reason: "예정".into() };
    let a1 = t.db.write(|tx| refund_register(tx, &planned)).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!((r[0].status.as_str(), r[0].returned_planned, r[0].outstanding), ("planned", 10_000, 30_000));
    // 33. 완료 처리에도 수납 확인·근거 필수
    assert_eq!(t.db.write(|tx| refund_complete(tx, a1.id, "2026-05-21", false, "x")).unwrap_err().code, "INVALID");
    t.db.write(|tx| refund_complete(tx, a1.id, "2026-05-21", true, "수납 대장 5/21")).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!((r[0].status_label.as_str(), r[0].returned_done, r[0].outstanding), ("일부 완료", 10_000, 20_000));
    // 34. 나머지 완료 → 처리 완료, 누계 초과는 거부
    let over = VoucherRefundInput { transfer_id: tid, amount: 20_001, status: "done".into(), processed_on: Some("2026-05-22".into()), collected_confirmed: true, evidence: "수납 대장".into(), reason: "잔액".into() };
    assert!(t.db.write(|tx| refund_register(tx, &over)).unwrap_err().message.contains("넘습니다"));
    let rest = VoucherRefundInput { amount: 20_000, ..over };
    t.db.write(|tx| refund_register(tx, &rest)).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!((r[0].status_label.as_str(), r[0].outstanding), ("처리 완료", 0));
    assert_eq!(r[0].returns.len(), 2);
    assert!(r[0].returns.iter().all(|x| x.collected_confirmed));
    // 35. 환불은 원장 징수액을 다시 줄이지 않는다
    let s = t.db.read(|c| ledger::month_summary(c, t.m(1))).unwrap();
    assert_eq!(s.collection_total, 30_000);
    assert_eq!(t.usage(t.stu).used_actual, 30_000);
    // 36. 환불 취소(P3) → 다시 미처리 쪽으로
    t.db.write(|tx| adjustment::cancel(tx, a1.id, "금액 입력 오류")).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!((r[0].status_label.as_str(), r[0].outstanding), ("일부 완료", 10_000));
    // 37. 경정 자체가 취소되면 환불 대상도 취소 (살아 있는 환불이 있으면 경정을 먼저 취소할 수 없다)
    assert!(t.db.write(|tx| adjustment::cancel(tx, tid, "학생 선택 오류")).is_err());
}

#[test]
fn e_refund_only_for_done_student_transfers() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.collect(1, t.ben, 30_000);
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    t.apply_normal(&p, "retro", "planned").unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r[0].transfer_status, "planned");
    let i = VoucherRefundInput { transfer_id: r[0].transfer_id, amount: 30_000, status: "done".into(), processed_on: Some("2026-05-20".into()), collected_confirmed: true, evidence: "x".into(), reason: "x".into() };
    assert!(t.db.write(|tx| refund_register(tx, &i)).unwrap_err().message.contains("처리 완료된 뒤"));
    // 프로그램 단위 경정은 환불 목록에 없다
    t.db.write(|tx| adjustment::create(tx, &AdjustmentInput { school_year_id: t.year_id, fiscal_month_id: t.m(1), kind: "fund_transfer".into(), program_id: t.coding, student_id: None, from_fund_id: Some(t.ben), to_fund_id: Some(t.free), fund_type_id: None, amount_item_id: t.instr, amount: 1_000, decided_on: None, processed_on: Some("2026-05-01".into()), status: "done".into(), reason: "프로그램 단위".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false })).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.len(), 1);
    assert!(r.iter().all(|x| x.student_id == t.stu));
}

// ── F. 전입생 ────────────────────────────────────────────────────────────────

#[test]
fn f_transfer_in_scenario_4_prior_usage_must_be_confirmed() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.collect(7, t.ben, 40_000); // 9월 수익자 40,000
    // 38. 전입생 등록 — 전입일 필수
    let mut i = t.target_input(t.stu, "confirmed", "2026-09-01");
    i.transfer_in = true;
    assert_eq!(t.db.write(|tx| target_save(tx, &i)).unwrap_err().code, "INVALID");
    i.transfer_in_on = Some("2026-09-01".into());
    let tg = t.db.write(|tx| target_save(tx, &i)).unwrap();
    assert!(tg.transfer_in && tg.prior_needs_confirm);
    assert_eq!(tg.prior_status_label, "이전 학교 사용액 확인 필요");
    // 39. 미확인 → 계산 차단 (0 으로 가정하지 않음)
    let u = t.usage(t.stu);
    assert!(u.blocked.iter().any(|b| b.contains("이전 학교 사용액 확인 필요")));
    assert_eq!(t.preview(t.stu).normal_count, 0);
    // 40. 금액 없이 확인 불가 · 음수 불가 · 전입생 아닌 학생 불가
    assert_eq!(t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg.id, prior_used: None, confirmed: true, note: String::new() })).unwrap_err().code, "INVALID");
    assert_eq!(t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg.id, prior_used: Some(-1), confirmed: true, note: String::new() })).unwrap_err().code, "INVALID");
    let tg2 = t.confirm(t.stu2, "2026-03-01");
    assert_eq!(t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg2.id, prior_used: Some(0), confirmed: true, note: String::new() })).unwrap_err().code, "INVALID");
    // 41. 250,000 확인 → 잔액 350,000 → 경정 40,000 → 잔액 310,000
    let tg = t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg.id, prior_used: Some(250_000), confirmed: true, note: "이전 학교 공문".into() })).unwrap();
    assert!(!tg.prior_needs_confirm);
    assert!(tg.logs.iter().any(|l| l.action == "prior" && l.detail.contains("250,000")));
    let u = t.usage(t.stu);
    assert!(u.blocked.is_empty(), "{:?}", u.blocked);
    assert_eq!(row(&u, 7, t.coding).remaining_before, Some(350_000));
    assert_eq!(u.remaining, Some(310_000));
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (1, 40_000));
    t.apply_normal(&p, "retro", "done").unwrap();
    let u = t.usage(t.stu);
    assert_eq!((u.used_actual, u.remaining), (40_000, Some(310_000)));
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r[0].outstanding, 40_000);
    // 42. 0원도 명시 확인하면 됨 · 43. 학기별이면 전입 학기에서 뺀다
    t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg.id, prior_used: Some(0), confirmed: true, note: "사용 없음 확인".into() })).unwrap();
    assert_eq!(t.usage(t.stu).remaining, Some(560_000));
    t.policy("semester", None, Some(300_000), Some(300_000), false);
    t.db.write(|tx| target_prior_set(tx, &VoucherPriorInput { target_id: tg.id, prior_used: Some(250_000), confirmed: true, note: String::new() })).unwrap();
    let u = t.usage(t.stu);
    assert_eq!(u.remaining_s1, Some(300_000));
    assert_eq!(u.remaining, Some(10_000));
}

// ── G. 일괄 처리 ──────────────────────────────────────────────────────────

#[test]
fn g_bulk_apply_is_atomic_and_revalidates_closed_months_scenario_6() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    for seq in 1..=3 {
        t.collect(seq, t.ben, 30_000);
    }
    t.confirm(t.stu, "2026-03-01");
    // 4월 확정 → 4월 행은 차단, 나머지는 정상
    t.db.write(|tx| year::month_close(tx, t.m(2), "확정")).unwrap();
    let p = t.preview(t.stu);
    assert_eq!(p.proposals.iter().filter(|x| x.state == "normal").count(), 2);
    let apr = p.proposals.iter().find(|x| x.fiscal_month_id == t.m(2)).unwrap();
    assert_eq!(apr.state, "blocked");
    assert!(apr.reasons.iter().any(|r| r.contains("확정된 달")));
    // 44. 차단 행을 섞어 등록하면 전부 되돌린다 (조용히 건너뛰지 않는다)
    let all: Vec<VoucherApplyRow> = p.proposals.iter().map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
    let err = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "전부".into(), decided_on: None, processed_on: Some("2026-05-15".into()), status: "done".into(), rows: all, decisions: vec![] })).unwrap_err();
    assert!(err.message.contains("확정된 달"), "{}", err.message);
    assert_eq!(t.db.read(|c| batch_list(c, t.year_id)).unwrap().len(), 0);
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM ledger_entry WHERE flow = 'transfer'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 0, "아무 거래도 남지 않는다");
    // 45. 정상 행만 등록
    let b = t.apply_normal(&p, "retro", "done").unwrap();
    assert_eq!((b.item_count, b.total_amount), (2, 60_000));
    // 46. 재개 후 4월 처리 → 재확정
    t.db.write(|tx| year::month_reopen(tx, t.m(2), "소급 처리")).unwrap();
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (1, 30_000));
    t.apply_normal(&p, "retro", "done").unwrap();
    t.db.write(|tx| year::month_close(tx, t.m(2), "재확정")).unwrap();
    let closes: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM month_close WHERE fiscal_month_id = ?1", [t.m(2)], |r| r.get(0))?)).unwrap();
    assert_eq!(closes, 3, "확정 → 재개 → 재확정 이력");
    assert!(t.usage(t.stu).rows.iter().all(|r| r.state == "ok"));
    // 47. 빈 묶음·사유 없음·완료인데 처리일 없음 → 거부
    let empty = VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "x".into(), decided_on: None, processed_on: Some("2026-05-15".into()), status: "done".into(), rows: vec![], decisions: vec![] };
    assert_eq!(t.db.write(|tx| apply(tx, &empty)).unwrap_err().code, "INVALID");
    // 48. 묶음 이력은 지우거나 고칠 수 없다
    let err = t.db.write(|tx| Ok(tx.execute("DELETE FROM voucher_batch WHERE id = ?1", [b.id])?)).unwrap_err();
    assert!(format!("{:?}", err).contains("VOUCHER_NO_DELETE"));
    let err = t.db.write(|tx| Ok(tx.execute("UPDATE voucher_batch SET total_amount = 1 WHERE id = ?1", [b.id])?)).unwrap_err();
    assert!(format!("{:?}", err).contains("VOUCHER_IMMUTABLE"));
    // 49. 처리 이력에 묶음·대상자 로그가 보인다
    let h = t.db.read(|c| history(c, t.year_id)).unwrap();
    assert_eq!(h.batches.len(), 2);
    assert!(h.batches.iter().all(|b| b.items.iter().all(|i| i.status == "done")));
    assert!(h.target_logs.iter().any(|l| l.action == "register"));
    assert_eq!(h.policy_history.len(), 1);
}

#[test]
fn g_bulk_apply_covers_many_students_and_fails_as_a_whole() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.collect_for(t.stu, 1, t.coding, t.ben, 30_000);
    t.collect_for(t.stu2, 1, t.art, t.ben, 20_000);
    t.confirm(t.stu, "2026-03-01");
    t.confirm(t.stu2, "2026-03-01");
    // 전체 대상자 미리보기
    let p = t.db.read(|c| retro_preview(c, t.year_id, &[], &[])).unwrap();
    assert_eq!((p.student_count, p.normal_count, p.normal_total), (2, 2, 50_000));
    // 한 학생 행에 잘못된 금액을 섞으면 두 학생 모두 등록되지 않는다
    let mut rows: Vec<VoucherApplyRow> = p.proposals.iter().map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: x.state == "check" }).collect();
    rows[1].amount += 1;
    let err = t.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "x".into(), decided_on: None, processed_on: Some("2026-05-15".into()), status: "done".into(), rows, decisions: vec![] })).unwrap_err();
    assert!(err.message.contains("재검증"), "{}", err.message);
    assert_eq!(t.db.read(|c| batch_list(c, t.year_id)).unwrap().len(), 0);
    let b = t.apply_normal(&p, "retro", "done").unwrap();
    assert_eq!(b.student_id, None, "여러 학생 묶음");
    assert_eq!(b.items.len(), 2);
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.iter().map(|x| x.outstanding).sum::<i64>(), 50_000);
}

// ── H. 회귀 (대조 연동) ─────────────────────────────────────────────────────

#[test]
fn h_recon_recomputes_after_voucher_transfer_and_notes_need_recheck() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    t.collect(1, t.ben, 30_000);
    t.collect_for(t.stu2, 1, t.coding, t.free, 5_000);
    t.db.write(|tx| {
        ledger::insert(tx, &NewEntry {
            school_year_id: t.year_id, fiscal_month_id: t.m(1), program_id: Some(t.coding), program_group_id: None, student_id: None,
            fund_type_id: Some(t.free), amount_item_id: t.instr, flow: "expenditure".into(), entry_kind: "instructor_pay".into(), amount: 35_000,
            stage: None, processed_on: None, counterparty: Some("강사가명".into()), upload_file_id: None, source_row_id: None, reason: None,
        }).map(|_| ())
    }).unwrap();
    let before = t.db.read(|c| recon::month(c, t.m(1))).unwrap();
    let r0 = before.rows.iter().find(|r| r.unit_name == "코딩" && r.item_set == "instructor").unwrap();
    assert_eq!((r0.status.as_str(), r0.diff), ("mismatch", -30_000), "자유수강권 징수 5,000 vs 지급 35,000");
    t.db.write(|tx| recon::note_save(tx, t.m(1), "program", t.coding, "instructor", "investigating", "자유수강권 확정 대기").map(|_| ())).unwrap();
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    t.apply_normal(&p, "retro", "done").unwrap();
    let after = t.db.read(|c| recon::month(c, t.m(1))).unwrap();
    let r1 = after.rows.iter().find(|r| r.unit_name == "코딩" && r.item_set == "instructor").unwrap();
    assert_eq!(r1.status, "ok");
    assert_eq!(r1.status_label, "비교 가능 재원 정상 · 일반 수익자 미검증");
    assert_eq!(r1.diff, 0);
    assert_eq!(r1.transfer_count, 2);
    assert!(r1.needs_recheck, "기존 메모는 보존되고 재확인 필요");
    assert_eq!(r1.note_status, "investigating");
    assert_eq!(r1.basis, r0.basis, "전체 기준액은 그대로");
}

#[test]
fn h_source_file_cancel_makes_voucher_transfer_orphan_and_schema_v6_survives() {
    let t = setup();
    // 거래 취소(void) 뒤 사용액은 줄고, 묶음·환불 대상은 그대로 남아 담당자가 확인한다
    t.policy("annual", Some(600_000), None, None, false);
    let e = t.collect(1, t.ben, 30_000);
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    t.apply_normal(&p, "retro", "done").unwrap();
    t.db.write(|tx| ledger::void(tx, e, "input_error", "원본 취소 시험").map(|_| ())).unwrap();
    let u = t.usage(t.stu);
    // 원본 징수 30,000 은 사라졌지만 경정 쌍(−30,000 수익자 / +30,000 자유)은 살아 있다 → 수익자 −30,000 · 자유 30,000
    let r = row(&u, 1, t.coding);
    assert_eq!((r.tuition, r.free_actual, r.beneficiary_actual), (0, 30_000, -30_000));
    assert_eq!(r.state, "over", "확인 대상으로만 표시 — 자동으로 되돌리지 않는다");
    assert_eq!(t.db.read(|c| refund_list(c, t.year_id)).unwrap().len(), 1);
    assert_eq!(t.db.read(|c| batch_list(c, t.year_id)).unwrap().len(), 1);
    assert_eq!(crate::db::migrate::current_version(&rusqlite::Connection::open_in_memory().unwrap()).unwrap(), 0);
    assert_eq!(crate::db::migrate::latest_version(), 10);
}

// ══════════════════════════════════════════════════════════════════════════
// P4.1 — 방과후이용권·자유수강권 중복 대상자 · 초3 초과금 우선 · 학기 기간 · 지원 대상 항목 (지시서 14장 1~45)
// ══════════════════════════════════════════════════════════════════════════

impl T {
    fn policy_input(&self, basis: &str, annual: Option<i64>, s1: Option<i64>, s2: Option<i64>, carry: bool) -> VoucherPolicyInput {
        VoucherPolicyInput { school_year_id: self.year_id, basis: basis.into(), annual_limit: annual, semester1_limit: s1, semester2_limit: s2, carry_over: carry, ..Default::default() }
    }
    fn exception(&self, stu: i64, annual: i64) {
        self.db.write(|tx| exception_save(tx, &VoucherExceptionInput { school_year_id: self.year_id, student_id: stu, basis: "annual".into(), annual_limit: Some(annual), semester1_limit: None, semester2_limit: None, carry_over: false, reason: "시험 예외".into() })).unwrap();
    }
    fn rows_of<'a>(&self, p: &'a VoucherRetroPreview, state: &str) -> Vec<VoucherApplyRow> {
        p.proposals.iter().filter(|x| x.state == state).map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: state == "check" }).collect()
    }
    fn apply_rows(&self, rows: Vec<VoucherApplyRow>, decisions: Vec<VoucherDecision>) -> AppResult<VoucherBatch> {
        self.db.write(|tx| apply(tx, &VoucherApplyInput { school_year_id: self.year_id, kind: "retro".into(), reason: "P4.1 시험".into(), decided_on: Some("2026-05-10".into()), processed_on: Some("2026-05-15".into()), status: "done".into(), rows, decisions }))
    }
    fn fund_amt(&self, u: &VoucherUsage, seq: i32, program: i64, name: &str) -> i64 {
        row(u, seq, program).funds.iter().find(|f| f.fund_name == name).map(|f| f.amount).unwrap_or(0)
    }
}

// ── A. 방과후이용권 중복 대상 (1~6) · 시나리오 1·2 ─────────────────────────

#[test]
fn p41_a_dual_target_converts_g3x_first_and_keeps_voucher_untouched() {
    let t = setup();
    t.policy("annual", Some(100_000), None, None, false);
    // 시나리오 1: 이용권 60,000 + 초3 초과금 40,000 · 잔액 100,000
    t.collect(2, t.voucher, 60_000);
    t.collect(2, t.g3x, 40_000);
    t.confirm(t.stu, "2026-03-01");
    let u = t.usage(t.stu);
    assert!(u.dual, "이용권 실적이 있으면 중복 대상자");
    let r = row(&u, 2, t.coding);
    assert_eq!((r.tuition, r.voucher_actual, r.g3x_actual, r.general_actual, r.free_actual), (100_000, 60_000, 40_000, 0, 0));
    assert_eq!(r.free_expected, 40_000, "이용권 60,000 은 지원 대상 수강료에 들지 않는다 (6)");
    assert_eq!((r.retro_g3x, r.retro_general), (40_000, 0));
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total, p.check_count), (1, 40_000, 0));
    let x = &p.proposals[0];
    assert_eq!((x.from_fund_name.as_str(), x.amount, x.remaining_after), ("초3 초과금", 40_000, 0));
    assert!(x.reasons.iter().any(|s| s.contains("우선")));
    t.apply_normal(&p, "retro", "done").unwrap();
    let u = t.usage(t.stu);
    assert_eq!(t.fund_amt(&u, 2, t.coding, "방과후이용권"), 60_000, "이용권 불변 (5)");
    assert_eq!(t.fund_amt(&u, 2, t.coding, "자유수강권"), 40_000);
    assert_eq!(t.fund_amt(&u, 2, t.coding, "초3 초과금"), 0, "초3 초과금 전액 전환 (2)");
    assert_eq!(row(&u, 2, t.coding).tuition, 100_000, "전체 유지");
    assert_eq!((u.used_actual, u.voucher_total, u.remaining), (40_000, 60_000, Some(60_000)), "이용권은 자유수강권 사용액에 들지 않는다");
    assert_eq!(t.preview(t.stu).normal_count, 0, "이용권 60,000 을 다시 전환하지 않는다");
    // 시나리오 2: 잔액 25,000 → 25,000 만 전환, 15,000 은 초3 초과금 유지 (3·4)
    t.collect_for(t.stu2, 2, t.coding, t.voucher, 60_000);
    t.collect_for(t.stu2, 2, t.coding, t.g3x, 40_000);
    t.confirm(t.stu2, "2026-03-01");
    t.exception(t.stu2, 25_000);
    let p = t.preview(t.stu2);
    assert_eq!((p.normal_count, p.normal_total), (1, 25_000));
    assert_eq!(p.proposals[0].remaining_after, 15_000);
    t.apply_normal(&p, "retro", "done").unwrap();
    let u = t.usage(t.stu2);
    assert_eq!(t.fund_amt(&u, 2, t.coding, "방과후이용권"), 60_000);
    assert_eq!(t.fund_amt(&u, 2, t.coding, "자유수강권"), 25_000);
    assert_eq!(t.fund_amt(&u, 2, t.coding, "초3 초과금"), 15_000);
    assert_eq!(row(&u, 2, t.coding).tuition, 100_000);
    assert_eq!(u.remaining, Some(0));
    assert_eq!(row(&u, 2, t.coding).state, "ok", "한도 소진분은 초3 초과금으로 남고 확인 대상이 아니다");
}

// ── B. 초3 초과금 소급 환불 (7~12) · 시나리오 3 ──────────────────────────

#[test]
fn p41_b_g3x_retroactive_refund_flow() {
    let t = setup();
    t.policy("annual", Some(600_000), None, None, false);
    for seq in 1..=3 {
        t.collect(seq, t.voucher, 60_000);
        t.collect(seq, t.g3x, 40_000);
    }
    t.confirm(t.stu, "2026-03-01"); // 5월 확정 · 적용 3월
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (3, 120_000), "소급 재원경정 120,000 (7)");
    assert!(p.proposals.iter().all(|x| x.from_fund_name == "초3 초과금"));
    t.apply_normal(&p, "retro", "done").unwrap();
    let u = t.usage(t.stu);
    assert_eq!(u.used_actual, 120_000, "자유수강권 사용액 120,000 증가");
    assert_eq!(u.voucher_total, 180_000, "이용권 불변");
    let s = t.db.read(|c| ledger::month_summary(c, t.m(1))).unwrap();
    assert_eq!(s.collection_total, 100_000, "원장 중복 차감 없음");
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|x| x.from_fund_name == "초3 초과금" && x.outstanding == 40_000 && x.status == "none"));
    assert_eq!(r.iter().map(|x| x.outstanding).sum::<i64>(), 120_000, "환불 대상 120,000");
    let (a, b, c3) = (r[0].transfer_id, r[1].transfer_id, r[2].transfer_id);
    let a_month = r[0].fiscal_month_id;
    let done = |tid: i64, amount: i64, confirmed: bool| VoucherRefundInput { transfer_id: tid, amount, status: "done".into(), processed_on: Some("2026-05-20".into()), collected_confirmed: confirmed, evidence: "수납 대장 5/20".into(), reason: "소급 환불".into() };
    // 8. 전액 환불
    let ra = t.db.write(|tx| refund_register(tx, &done(a, 40_000, true))).unwrap();
    // 9. 일부 환불
    t.db.write(|tx| refund_register(tx, &done(b, 15_000, true))).unwrap();
    // 10. 미납(수납 확인 없음)이면 완료 처리 차단
    assert!(t.db.write(|tx| refund_register(tx, &done(c3, 40_000, false))).unwrap_err().message.contains("수납"));
    // 11. 같은 경정에 중복 환불 → 미반환을 넘으면 차단
    assert!(t.db.write(|tx| refund_register(tx, &done(b, 30_000, true))).unwrap_err().message.contains("넘습니다"));
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    let st = |tid: i64| r.iter().find(|x| x.transfer_id == tid).unwrap();
    assert_eq!((st(a).status.as_str(), st(a).outstanding), ("complete", 0));
    assert_eq!((st(b).status.as_str(), st(b).outstanding), ("partial", 25_000));
    assert_eq!((st(c3).status.as_str(), st(c3).outstanding), ("none", 40_000));
    assert_eq!(t.db.read(|c| ledger::month_summary(c, t.m(1))).unwrap().collection_total, 100_000, "환불은 원장을 다시 차감하지 않는다");
    // 12. 경정 취소와 반환 이행 연결: 살아 있는 환불이 있으면 경정 취소 불가 → 환불 취소 뒤 경정 취소 → 환불 대상 '취소'
    assert!(t.db.write(|tx| adjustment::cancel(tx, a, "학생 선택 오류")).is_err());
    t.db.write(|tx| adjustment::cancel(tx, ra.id, "금액 입력 오류")).unwrap();
    t.db.write(|tx| adjustment::cancel(tx, a, "학생 선택 오류")).unwrap();
    let r = t.db.read(|c| refund_list(c, t.year_id)).unwrap();
    assert_eq!(r.iter().find(|x| x.transfer_id == a).unwrap().status, "cancelled");
    let u = t.usage(t.stu);
    assert_eq!(u.used_actual, 80_000, "취소된 경정만큼 사용액이 줄고 초3 초과금으로 돌아간다");
    assert_eq!(u.rows.iter().find(|x| x.fiscal_month_id == a_month).unwrap().g3x_actual, 40_000);
}

// ── C. 지원 한도 · 우선순위 (13~19) · 5장 혼합 재원 ───────────────────────

#[test]
fn p41_c_mixed_funds_priority_confirmation_and_limits() {
    let t = setup();
    t.policy("annual", Some(40_000), None, None, false);
    // 이용권 60,000 + 초3 초과금 30,000 + 일반 수익자 20,000 · 잔액 40,000
    t.collect(2, t.voucher, 60_000);
    t.collect(2, t.g3x, 30_000);
    t.collect(2, t.ben, 20_000);
    t.confirm(t.stu, "2026-03-01");
    let u = t.usage(t.stu);
    let r = row(&u, 2, t.coding);
    assert_eq!((r.free_expected, r.retro_g3x, r.retro_general), (40_000, 30_000, 10_000), "초3 초과금 우선 (13·14)");
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total, p.check_count, p.check_total), (1, 30_000, 1, 10_000));
    let g = p.proposals.iter().find(|x| x.state == "normal").unwrap();
    let gen = p.proposals.iter().find(|x| x.state == "check").unwrap();
    assert_eq!((g.from_fund_name.as_str(), gen.from_fund_name.as_str()), ("초3 초과금", "일반 수익자"));
    assert!(gen.reasons.iter().any(|s| s.contains("재원 확인")));
    // 담당자 확인 없이 일반 수익자를 등록하면 차단 (임의 전환 금지)
    let mut rows = t.rows_of(&p, "check");
    rows[0].confirmed = false;
    let err = t.apply_rows(rows, vec![]).unwrap_err();
    assert!(err.message.contains("재원 확인"), "{}", err.message);
    // 초3 초과금을 건너뛰고 일반 수익자만 등록하면 우선순위 위반
    let err = t.apply_rows(t.rows_of(&p, "check"), vec![]).unwrap_err();
    assert!(err.message.contains("우선순위"), "{}", err.message);
    assert_eq!(t.db.read(|c| batch_list(c, t.year_id)).unwrap().len(), 0, "아무것도 등록되지 않았다");
    // 둘 다 (일반 수익자는 확인 표시) → 자유 40,000 · 초3 초과금 0 · 일반 10,000 · 이용권 60,000
    let mut rows = t.rows_of(&p, "normal");
    rows.extend(t.rows_of(&p, "check"));
    t.apply_rows(rows, vec![]).unwrap();
    let u = t.usage(t.stu);
    let r = row(&u, 2, t.coding);
    assert_eq!((r.free_actual, r.g3x_actual, r.general_actual, r.voucher_actual, r.tuition), (40_000, 0, 10_000, 60_000, 110_000));
    assert_eq!((u.remaining, r.state.as_str()), (Some(0), "ok"), "한도 일부 소진 (15)");
    // 16. 한도 완전 소진: 예외 30,000 → 초3 초과금만, 일반 수익자는 제안되지 않고 일반 수익자로 남는다
    t.collect_for(t.stu2, 2, t.coding, t.voucher, 60_000);
    t.collect_for(t.stu2, 2, t.coding, t.g3x, 30_000);
    t.collect_for(t.stu2, 2, t.coding, t.ben, 20_000);
    t.collect_for(t.stu2, 2, t.coding, t.free, 5_000); // 19. 이미 자유수강권인 금액
    t.confirm(t.stu2, "2026-03-01");
    t.exception(t.stu2, 30_000);
    let p = t.preview(t.stu2);
    assert_eq!((p.normal_count, p.normal_total, p.check_count), (1, 25_000, 0), "30,000 − 이미 자유 5,000 = 25,000, 초3 초과금에서만 (19)");
    t.apply_normal(&p, "retro", "done").unwrap();
    let u = t.usage(t.stu2);
    let r = row(&u, 2, t.coding);
    assert_eq!((r.free_actual, r.g3x_actual, r.general_actual), (30_000, 5_000, 20_000));
    assert_eq!(t.fund_amt(&u, 2, t.coding, "일반 수익자"), 20_000, "한도 소진을 이유로 재원을 바꾸지 않는다");
    assert_eq!(u.remaining, Some(0));
}

// ── C. 제한없음 (17·18) · 시나리오 4 ──────────────────────────────────────

#[test]
fn p41_c_unlimited_still_proposes_remaining_beneficiary_amounts() {
    let t = setup();
    t.policy("unlimited", None, None, None, false);
    // 17. 일반 수익자 90,000 남아 있음 → 제안 90,000
    t.collect(1, t.ben, 90_000);
    t.confirm(t.stu, "2026-03-01");
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (1, 90_000));
    // 18. 중복 대상자 초3 초과금 40,000 → 제안 40,000, 이용권 60,000 유지, 한도 오류 없음
    t.collect_for(t.stu2, 1, t.coding, t.voucher, 60_000);
    t.collect_for(t.stu2, 1, t.coding, t.g3x, 40_000);
    t.confirm(t.stu2, "2026-03-01");
    let p2 = t.preview(t.stu2);
    assert_eq!((p2.normal_count, p2.normal_total), (1, 40_000));
    assert_eq!(p2.proposals[0].from_fund_name, "초3 초과금");
    assert!(p2.usages[0].blocked.is_empty() && !p2.usages[0].warnings.iter().any(|w| w.contains("한도")), "한도 오류 없음: {:?}", p2.usages[0].warnings);
    t.apply_normal(&p, "retro", "done").unwrap();
    t.apply_normal(&p2, "retro", "done").unwrap();
    let u2 = t.usage(t.stu2);
    assert_eq!(t.fund_amt(&u2, 1, t.coding, "방과후이용권"), 60_000);
    assert_eq!(t.fund_amt(&u2, 1, t.coding, "자유수강권"), 40_000);
    assert_eq!(t.fund_amt(&u2, 1, t.coding, "초3 초과금"), 0);
    assert_eq!((u2.limit_total, u2.remaining), (None, None));
    assert_eq!(t.usage(t.stu).used_actual, 90_000);
    assert_eq!(t.preview(t.stu).normal_count + t.preview(t.stu2).normal_count, 0, "처리된 금액은 다시 제안하지 않는다");
}

// ── D. 학기 설정 (20~26) ───────────────────────────────────────────────────

#[test]
fn p41_d_semester_dates_defaults_validation_and_preservation() {
    let t = setup();
    // 20. 기본값
    let p = t.policy("semester", None, Some(200_000), Some(200_000), true);
    assert!(p.semester_default);
    assert_eq!((p.semester1_from.as_str(), p.semester1_to.as_str(), p.semester2_from.as_str(), p.semester2_to.as_str()), ("2026-03-01", "2026-08-31", "2026-09-01", "2027-02-28"));
    assert!(p.uncovered_months.is_empty() && p.all_items_eligible);
    // 21. 변경: 1학기 3/1~7/31 · 2학기 9/1~1/31 → 8월·2월 미지원 기간 경고
    let mut i = t.policy_input("semester", None, Some(100_000), Some(100_000), true);
    i.semester1_from = Some("2026-03-01".into());
    i.semester1_to = Some("2026-07-31".into());
    i.semester2_from = Some("2026-09-01".into());
    i.semester2_to = Some("2027-01-31".into());
    i.confirm_gap = true; // P5: 공백 달은 명시적으로 확인해야 저장된다
    let pv = t.db.read(|c| policy_preview(c, &i)).unwrap();
    assert!(pv.blocked.is_empty(), "{:?}", pv.blocked);
    assert_eq!(pv.uncovered_months, vec!["8월 (2026-08)".to_string(), "2월 (2027-02)".to_string()]);
    assert!(pv.warnings.iter().any(|x| x.contains("0원으로 계산")));
    let p = t.db.write(|tx| policy_save(tx, &i)).unwrap();
    assert!(!p.semester_default);
    assert_eq!(p.uncovered_months.len(), 2);
    t.confirm(t.stu, "2026-03-01");
    t.collect(5, t.ben, 60_000); // 7월
    t.collect(6, t.ben, 50_000); // 8월
    t.collect(7, t.ben, 120_000); // 9월
    let u = t.usage(t.stu);
    assert_eq!((row(&u, 5, t.coding).semester, row(&u, 5, t.coding).free_expected), (1, 60_000));
    assert_eq!((row(&u, 6, t.coding).semester, row(&u, 6, t.coding).free_expected, row(&u, 6, t.coding).state.as_str()), (0, 0, "ok"), "8월은 미지원 기간");
    assert_eq!(row(&u, 7, t.coding).free_expected, 120_000, "24. 1학기 잔액 40,000 이월 → 2학기 140,000");
    assert!(u.notes.iter().any(|n| n.contains("이월")) && u.notes.iter().any(|n| n.contains("미지원 기간")));
    // 25. 이월 불허
    let mut j = i.clone();
    j.carry_over = false;
    t.db.write(|tx| policy_save(tx, &j)).unwrap();
    assert_eq!(row(&t.usage(t.stu), 7, t.coding).free_expected, 100_000);
    // 22. 겹침 차단 · 23. 학년도 범위 이탈 차단 · 시작>종료 차단
    let mut bad = i.clone();
    bad.semester1_to = Some("2026-09-30".into());
    assert!(t.db.read(|c| policy_preview(c, &bad)).unwrap().blocked.iter().any(|b| b.contains("겹칩")));
    assert_eq!(t.db.write(|tx| policy_save(tx, &bad)).unwrap_err().code, "INVALID");
    let mut out = i.clone();
    out.semester2_to = Some("2027-03-31".into());
    assert!(t.db.read(|c| policy_preview(c, &out)).unwrap().blocked.iter().any(|b| b.contains("학년도 범위")));
    let mut rev = i.clone();
    rev.semester1_from = Some("2026-08-01".into());
    assert!(t.db.read(|c| policy_preview(c, &rev)).unwrap().blocked.iter().any(|b| b.contains("늦습니다")));
    let mut neg = i.clone();
    neg.semester2_limit = Some(-1);
    assert_eq!(t.db.write(|tx| policy_save(tx, &neg)).unwrap_err().code, "INVALID");
    // 26. 정책 변경 뒤 기존 거래 보존
    let p = t.preview(t.stu);
    t.apply_normal(&p, "retro", "done").unwrap();
    let before: (i64, i64) = t.db.read(|c| Ok(c.query_row("SELECT count(*), coalesce(sum(amount), 0) FROM ledger_entry WHERE flow = 'transfer' AND state = 'active'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    t.policy("annual", Some(50_000), None, None, false);
    let after: (i64, i64) = t.db.read(|c| Ok(c.query_row("SELECT count(*), coalesce(sum(amount), 0) FROM ledger_entry WHERE flow = 'transfer' AND state = 'active'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!(before, after, "정책을 바꿔도 완료된 경정은 그대로");
    assert_eq!(after.1, 0);
    let h = t.db.read(|c| policy_history(c, t.year_id)).unwrap();
    assert_eq!(h.len(), 4, "정책 이력 보존");
    assert!(h[1].uncovered_months.len() == 2, "과거 정책의 학기 기준을 그대로 확인할 수 있다");
    assert!(t.usage(t.stu).over_total > 0, "새 한도 50,000 기준으로는 초과 사용이 확인 대상으로 보인다 (자동 되돌림 없음)");
}

// ── E. 지원 대상 항목 (27~30) ─────────────────────────────────────────────

#[test]
fn p41_e_eligible_items_setting_excludes_and_preserves_done_transfers() {
    let t = setup();
    let mut i = t.policy_input("annual", Some(600_000), None, None, false);
    i.eligible_item_ids = Some(vec![t.instr]);
    let p = t.db.write(|tx| policy_save(tx, &i)).unwrap();
    assert!(!p.all_items_eligible);
    assert_eq!(p.eligible_item_names, vec!["강사료".to_string()]);
    let mut empty = i.clone();
    empty.eligible_item_ids = Some(vec![]);
    assert_eq!(t.db.write(|tx| policy_save(tx, &empty)).unwrap_err().code, "INVALID");
    t.confirm(t.stu, "2026-03-01");
    t.collect(2, t.ben, 30_000);
    t.db.write(|tx| {
        ledger::insert(tx, &NewEntry {
            school_year_id: t.year_id, fiscal_month_id: t.m(2), program_id: Some(t.coding), program_group_id: None, student_id: Some(t.stu),
            fund_type_id: Some(t.ben), amount_item_id: t.textbook, flow: "collection".into(), entry_kind: "initial_collect".into(), amount: 10_000,
            stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
        }).map(|_| ())
    }).unwrap();
    let u = t.usage(t.stu);
    let tb = u.rows.iter().find(|r| r.amount_item_id == t.textbook).unwrap();
    assert_eq!((tb.eligible_item, tb.state.as_str(), tb.free_expected), (false, "excluded", 0), "28. 미지원 항목 제외");
    let ins = u.rows.iter().find(|r| r.amount_item_id == t.instr).unwrap();
    assert_eq!((ins.eligible_item, ins.state.as_str(), ins.free_expected), (true, "retro", 30_000), "27. 지원 항목 포함");
    assert_eq!(u.used_expected, 30_000);
    assert_eq!(t.preview(t.stu).normal_total, 30_000);
    // 29. 설정 변경: 모든 항목 → 교재비도 경정 대상
    let all = t.policy("annual", Some(600_000), None, None, false);
    assert!(all.all_items_eligible);
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.normal_total), (2, 40_000));
    t.apply_normal(&p, "retro", "done").unwrap();
    // 다시 강사료만 → 영향 미리보기에 교재비 경정 1건 10,000 이 보이고, 저장해도 완료된 경정은 그대로 (30)
    let pv = t.db.read(|c| policy_preview(c, &i)).unwrap();
    assert_eq!((pv.affected_transfer_count, pv.affected_transfer_total), (1, 10_000));
    assert!(pv.warnings.iter().any(|x| x.contains("자동으로 취소·수정하지 않습니다")));
    t.db.write(|tx| policy_save(tx, &i)).unwrap();
    let done: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM adjustment WHERE kind = 'fund_transfer' AND status = 'done'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(done, 2);
    let u = t.usage(t.stu);
    let tb = u.rows.iter().find(|r| r.amount_item_id == t.textbook).unwrap();
    assert_eq!((tb.state.as_str(), tb.free_actual), ("excluded", 10_000), "기존 재원으로 남아 보인다");
    assert_eq!(t.preview(t.stu).proposals.iter().filter(|x| x.amount_item_id == t.textbook).count(), 0);
}

// ── F. 여러 프로그램 (31~35) · 시나리오 5 ─────────────────────────────────

#[test]
fn p41_f_g3x_across_programs_needs_manual_split_and_preserves_totals() {
    let t = setup();
    t.policy("annual", Some(50_000), None, None, false);
    t.collect_for(t.stu, 3, t.art, t.voucher, 60_000);
    t.collect_for(t.stu, 3, t.art, t.g3x, 30_000);
    t.collect_for(t.stu, 3, t.coding, t.voucher, 60_000);
    t.collect_for(t.stu, 3, t.coding, t.g3x, 40_000);
    t.confirm(t.stu, "2026-03-01");
    let u = t.usage(t.stu);
    assert!(u.needs_decision, "32. 한도 부족 → 배분 결정 필요");
    assert!(u.dual);
    let p = t.preview(t.stu);
    assert_eq!((p.normal_count, p.check_count), (0, 0), "확인 전 자동 경정 금지");
    assert_eq!(p.proposals.iter().filter(|x| x.state == "decide").count(), 2);
    let d = |a: i64, c: i64| vec![
        VoucherDecision { student_id: t.stu, fiscal_month_id: t.m(3), program_id: t.art, amount_item_id: t.instr, free_amount: a },
        VoucherDecision { student_id: t.stu, fiscal_month_id: t.m(3), program_id: t.coding, amount_item_id: t.instr, free_amount: c },
    ];
    // 34. 프로그램별 지원 대상 금액 초과 (미술 40,000 > 30,000) · 35. 전체 한도 초과 (70,000 > 50,000) → 여전히 결정 필요
    for bad in [d(40_000, 10_000), d(30_000, 40_000)] {
        let u = t.db.read(|c| usage(c, t.year_id, t.stu, &bad)).unwrap();
        assert!(u.needs_decision);
    }
    // 33. 수동 배분 미술 10,000 · 코딩 40,000 → 정상 경정 2건, 전부 초3 초과금에서
    let ok = d(10_000, 40_000);
    let p = t.db.read(|c| retro_preview(c, t.year_id, &[t.stu], &ok)).unwrap();
    assert_eq!((p.normal_count, p.normal_total), (2, 50_000));
    assert!(p.proposals.iter().all(|x| x.from_fund_name == "초3 초과금"));
    t.apply_rows(t.rows_of(&p, "normal"), ok.clone()).unwrap();
    let u = t.db.read(|c| usage(c, t.year_id, t.stu, &ok)).unwrap();
    assert_eq!(u.used_actual, 50_000, "자유수강권 총 지원액 50,000");
    assert_eq!(u.g3x_total, 20_000, "초3 초과금 잔액 20,000");
    assert_eq!(u.voucher_total, 120_000);
    assert_eq!(u.rows.iter().map(|r| r.tuition).sum::<i64>(), 190_000, "전체 금액 보존 (31)");
    assert!(u.rows.iter().all(|r| r.state == "ok"));
    assert_eq!(u.remaining, Some(0));
}

// ── G. 회귀 (36~45) — 36·37·38·39·40·41·42·43·44 는 기존 P3·P4 시험이 그대로 덮는다. 45: 마이그레이션 ──

#[test]
fn p41_g_v6_policy_survives_v7_migration_with_defaults() {
    use crate::db::migrate;
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::run_up_to(&mut c, 6).unwrap();
    let y = year::create(&c, 2026).unwrap();
    c.execute("INSERT INTO voucher_policy (school_year_id, basis, annual_limit, note) VALUES (?1, 'annual', 600000, 'v6 정책')", [y.id]).unwrap();
    let audits_before: i64 = c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).unwrap();
    migrate::run(&mut c).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), migrate::latest_version());
    assert_eq!(migrate::latest_version(), 10);
    let p = policy_current(&c, y.id).unwrap().unwrap();
    assert_eq!((p.annual_limit, p.note.as_str()), (Some(600_000), "v6 정책"), "기존 정책 보존");
    assert!(p.semester_default && p.all_items_eligible, "기본 학기·모든 항목으로 해석 → 계산 결과 불변");
    assert_eq!(p.semester2_to, "2027-02-28");
    let audits_after: i64 = c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).unwrap();
    assert_eq!(audits_before, audits_after, "감사 이력 보존 (44)");
    let err = c.execute("DELETE FROM voucher_policy_item WHERE policy_id = 1", []).err();
    assert!(err.is_none() || format!("{err:?}").contains("VOUCHER_NO_DELETE"));
}

// ══════════════════════════════════════════════════════════════════════════
// P5 — 방과후이용권 대상 여부(24~29) · 학기 공백 명시 확인(35~38) · 합산 항목(31·33·34)
// ══════════════════════════════════════════════════════════════════════════

#[test]
fn p5_d_after_voucher_target_status_is_separate_from_actual_usage() {
    let t = setup();
    t.policy("annual", Some(100_000), None, None, false);
    // 24. 대상 여부 등록 — 근거·확인일 필수
    let bad = AfterVoucherTargetInput { school_year_id: t.year_id, student_id: t.stu, status: "eligible".into(), apply_from: Some("2026-03-01".into()), apply_to: None, confirmed_on: Some("2026-03-02".into()), evidence: String::new(), note: String::new(), reason: String::new() };
    assert_eq!(t.db.write(|tx| av_target_save(tx, &bad)).unwrap_err().code, "INVALID");
    let ok = AfterVoucherTargetInput { evidence: "교육청 대상자 명단".into(), ..bad.clone() };
    let a = t.db.write(|tx| av_target_save(tx, &ok)).unwrap();
    assert_eq!((a.status_label.as_str(), a.actual_total, a.logs.len()), ("대상자", 0, 1));
    // 26·시나리오 7: 실적 없는 대상자 → 중복 대상자 규칙 적용, 이용권 사용액 0 유지
    t.collect(2, t.g3x, 40_000);
    t.collect(2, t.ben, 20_000);
    t.confirm(t.stu, "2026-03-01");
    let u = t.usage(t.stu);
    assert!(u.dual && u.voucher_total == 0);
    assert_eq!(u.voucher_target_status, "eligible");
    let p = t.preview(t.stu);
    assert_eq!((p.normal_total, p.check_total), (40_000, 20_000), "29. 초3 초과금 우선, 일반 수익자는 재원 확인");
    assert!(u.warnings.iter().all(|w| !w.contains("대상 여부가")));
    // 25. 상태 변경 — 사유 필수, 이력 유지
    let chg = AfterVoucherTargetInput { status: "ineligible".into(), ..ok.clone() };
    assert_eq!(t.db.write(|tx| av_target_save(tx, &chg)).unwrap_err().code, "INVALID");
    let chg = AfterVoucherTargetInput { status: "ineligible".into(), reason: "명단 정정".into(), ..ok.clone() };
    let a = t.db.write(|tx| av_target_save(tx, &chg)).unwrap();
    assert_eq!(a.logs.len(), 2);
    assert_eq!(a.logs[1].to_status, "ineligible");
    assert!(!t.usage(t.stu).dual, "비대상자이고 실적도 없으면 중복 대상자가 아니다");
    // 27·28. 실적은 있으나 대상 미확인 → 목록에 '기록 없음' 으로 보이고, 중복 규칙은 적용되나 확인 안내
    t.collect_for(t.stu2, 2, t.coding, t.voucher, 60_000);
    t.collect_for(t.stu2, 2, t.coding, t.g3x, 40_000);
    t.confirm(t.stu2, "2026-03-01");
    let list = t.db.read(|c| av_target_list(c, t.year_id)).unwrap();
    let s2 = list.iter().find(|x| x.student_id == t.stu2).expect("실적만 있는 학생도 목록에");
    assert_eq!((s2.status.as_str(), s2.actual_total, s2.id), ("none", 60_000, 0));
    let u2 = t.usage(t.stu2);
    assert!(u2.dual);
    assert_eq!(u2.voucher_target_status, "none");
    assert!(u2.warnings.iter().any(|w| w.contains("대상 여부")), "{:?}", u2.warnings);
    // 실적이 있어도 대상자 기록은 자동 생성되지 않는다
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM after_voucher_target", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 1);
}

#[test]
fn p5_f_semester_gap_requires_explicit_confirmation() {
    let t = setup();
    let mut i = t.policy_input("semester", None, Some(100_000), Some(100_000), true);
    i.semester1_from = Some("2026-03-01".into());
    i.semester1_to = Some("2026-07-31".into());
    i.semester2_from = Some("2026-09-01".into());
    i.semester2_to = Some("2027-02-28".into());
    // 35·36. 공백 감지 · 확인 전 저장 차단
    let pv = t.db.read(|c| policy_preview(c, &i)).unwrap();
    assert!(pv.gap_needs_confirm);
    assert_eq!(pv.uncovered_months, vec!["8월 (2026-08)".to_string()]);
    assert!(pv.blocked.iter().any(|b| b.contains("8월") && b.contains("0원으로 계산") && b.contains("유지하시겠습니까")));
    assert_eq!(t.db.write(|tx| policy_save(tx, &i)).unwrap_err().code, "INVALID");
    assert!(t.db.read(|c| policy_history(c, t.year_id)).unwrap().is_empty());
    // 37. 확인 후 저장 · 38. 이력 보존
    i.confirm_gap = true;
    let p = t.db.write(|tx| policy_save(tx, &i)).unwrap();
    assert!(p.gap_confirmed);
    assert_eq!(p.uncovered_months.len(), 1);
    let j = t.policy("annual", Some(600_000), None, None, false);
    assert!(!j.gap_confirmed);
    let h = t.db.read(|c| policy_history(c, t.year_id)).unwrap();
    assert_eq!(h.len(), 2);
    assert!(h[1].gap_confirmed && h[1].superseded_at.is_some());
    // 공백 없는 학기 설정은 확인 없이 저장된다
    let k = t.policy_input("semester", None, Some(1), Some(1), false);
    assert!(!t.db.read(|c| policy_preview(c, &k)).unwrap().gap_needs_confirm);
}
