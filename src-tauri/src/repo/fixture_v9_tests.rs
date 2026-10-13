//! P7-2 — 자동 업데이트 실측용 **v9 가명 자료 DB** 생성기 (저장소 밖 파일에 쓴다, 환경변수로만 돈다).
//!
//!   ASACCOUNTING_V9_OUT="C:/.../asaccounting.db" cargo test --lib p72_make_v9_fixture_db -- --ignored --nocapture
//!
//! v0.1.0(스키마 v9) 앱이 그대로 열 수 있는 DB: 학생·프로그램(세부반 포함)·징수·강사료·교재재료비·환불·추가징수·재원경정·
//! 자유수강권 정책/대상·이용권 대상·월 확정(지문 형식 1)·감사 이력·백업 설정. 가명·가상 금액만 쓴다.

use std::path::PathBuf;

use rusqlite::{params, Connection};

use crate::db::migrate;
use crate::model::*;
use crate::repo::{adjustment, catalog, ledger, program, setting, voucher, year};

#[test]
#[ignore]
fn p72_make_v9_fixture_db() {
    let Ok(out) = std::env::var("ASACCOUNTING_V9_OUT") else {
        eprintln!("ASACCOUNTING_V9_OUT 없음 — 건너뜀");
        return;
    };
    let path = PathBuf::from(out);
    if path.exists() {
        std::fs::remove_file(&path).unwrap();
    }
    let mut c = Connection::open(&path).unwrap();
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::run_up_to(&mut c, 9).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), 9);

    let y = year::create(&c, 2026).unwrap();
    let months = year::months(&c, y.id).unwrap();
    let m = |ym: &str| months.iter().find(|x| x.ym == ym).unwrap().id;
    let (jun, sep) = (m("2026-06"), m("2026-09"));
    let cat = catalog::all(&c).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    let (ben, free, av) = (fund("beneficiary"), fund("free_voucher"), fund("after_voucher"));
    let (instr, tm) = (item("instructor"), item("textbook_material"));

    let robot = program::create(&c, y.id, "로봇과학").unwrap();
    let robot_a = program::create(&c, y.id, "로봇과학A").unwrap();
    let robot_b = program::create(&c, y.id, "로봇과학B").unwrap();
    let coding = program::create(&c, y.id, "코딩").unwrap();
    let art = program::create(&c, y.id, "미술").unwrap();
    let sa = voucher::student_create(&c, &StudentCreateInput { school_year_id: y.id, grade: 3, class_name: "1".into(), student_no: "1".into(), name: "가명A".into() }).unwrap().id;
    let sb = voucher::student_create(&c, &StudentCreateInput { school_year_id: y.id, grade: 4, class_name: "2".into(), student_no: "7".into(), name: "가명B".into() }).unwrap().id;
    let sc = voucher::student_create(&c, &StudentCreateInput { school_year_id: y.id, grade: 5, class_name: "3".into(), student_no: "12".into(), name: "가명C".into() }).unwrap().id;

    let e = |month: i64, p: i64, s: Option<i64>, f: i64, it: i64, flow: &str, kind: &str, amount: i64, party: &str| {
        ledger::insert(&c, &NewEntry { school_year_id: y.id, fiscal_month_id: month, program_id: Some(p), program_group_id: None, student_id: s, fund_type_id: Some(f), amount_item_id: it, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: Some("2026-09-25".into()), counterparty: Some(party.into()), upload_file_id: None, source_row_id: None, reason: None }).unwrap().id
    };
    // 6월 (확정할 달): 코딩 징수·지급
    e(jun, coding, Some(sa), ben, instr, "collection", "initial_collect", 30_000, "");
    e(jun, coding, Some(sb), free, instr, "collection", "initial_collect", 30_000, "");
    e(jun, coding, None, ben, instr, "expenditure", "instructor_pay", 60_000, "강사 가명");
    // 9월: 세부반 징수 + 통합 지급 (매핑 추천이 나오도록) · 코딩 · 미술
    e(sep, robot_a, Some(sa), ben, instr, "collection", "initial_collect", 50_000, "");
    e(sep, robot_a, Some(sb), av, instr, "collection", "initial_collect", 50_000, "");
    e(sep, robot_b, Some(sc), ben, instr, "collection", "initial_collect", 50_000, "");
    e(sep, robot_a, Some(sa), ben, tm, "collection", "initial_collect", 20_000, "");
    e(sep, robot_b, Some(sc), ben, tm, "collection", "initial_collect", 20_000, "");
    e(sep, robot, None, ben, instr, "expenditure", "instructor_pay", 100_000, "강사 가명1");
    e(sep, robot, None, av, instr, "expenditure", "instructor_pay", 50_000, "강사 가명1");
    e(sep, robot, None, ben, tm, "expenditure", "vendor_pay", 40_000, "업체 가명");
    e(sep, coding, Some(sa), ben, instr, "collection", "initial_collect", 30_000, "");
    e(sep, coding, Some(sb), free, instr, "collection", "initial_collect", 30_000, "");
    e(sep, coding, None, ben, instr, "expenditure", "instructor_pay", 60_000, "강사 가명2");
    e(sep, art, Some(sc), ben, instr, "collection", "initial_collect", 25_000, "");
    e(sep, art, Some(sc), ben, instr, "collection", "extra_collect", 5_000, "");
    e(sep, art, Some(sc), ben, instr, "collection", "refund", -10_000, "");
    e(sep, art, None, ben, instr, "expenditure", "instructor_pay", 20_000, "강사 가명3");
    // 재원경정 (코딩 9월, 가명A 수익자 → 자유수강권 10,000) · 환불 정정 (미술 가명C 5,000)
    adjustment::create(&c, &AdjustmentInput {
        school_year_id: y.id, fiscal_month_id: sep, kind: "fund_transfer".into(), program_id: coding, student_id: Some(sa),
        from_fund_id: Some(ben), to_fund_id: Some(free), fund_type_id: None, amount_item_id: instr, amount: 10_000,
        decided_on: Some("2026-09-20".into()), processed_on: Some("2026-09-20".into()), status: "done".into(),
        reason: "가명 시험 — 자유수강권 소급".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
    }).unwrap();
    adjustment::create(&c, &AdjustmentInput {
        school_year_id: y.id, fiscal_month_id: sep, kind: "refund".into(), program_id: art, student_id: Some(sc),
        from_fund_id: None, to_fund_id: None, fund_type_id: Some(ben), amount_item_id: instr, amount: 5_000,
        decided_on: Some("2026-09-28".into()), processed_on: Some("2026-09-28".into()), status: "done".into(),
        reason: "가명 시험 — 중도 환불".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
    }).unwrap();
    // 자유수강권 정책·대상 · 이용권 대상
    voucher::policy_save(&c, &VoucherPolicyInput { school_year_id: y.id, basis: "annual".into(), annual_limit: Some(600_000), semester1_limit: None, semester2_limit: None, carry_over: false, note: "가명 시험".into(), ..Default::default() }).unwrap();
    voucher::target_save(&c, &VoucherTargetInput { school_year_id: y.id, student_id: sb, status: "confirmed".into(), confirmed_on: Some("2026-03-02".into()), apply_from: Some("2026-03-01".into()), apply_to: None, transfer_in: false, transfer_in_on: None, note: "".into(), reason: "가명 시험".into() }).unwrap();
    voucher::av_target_save(&c, &AfterVoucherTargetInput { school_year_id: y.id, student_id: sb, status: "eligible".into(), apply_from: Some("2026-03-01".into()), apply_to: None, confirmed_on: Some("2026-03-02".into()), evidence: "가명 근거".into(), note: "".into(), reason: "가명 시험".into() }).unwrap();
    // 6월 확정 — v9 시절 형식(지문 1)
    let (fp, snap) = year::month_fingerprint_v(&c, jun, 1).unwrap();
    c.execute("UPDATE fiscal_month SET status = 'closed' WHERE id = ?1", [jun]).unwrap();
    c.execute("INSERT INTO month_close (fiscal_month_id, action, totals_json, note, fingerprint, snapshot_json) VALUES (?1, 'close', '{}', '6월 확정(가명)', ?2, ?3)", params![jun, fp, snap.to_string()]).unwrap();
    crate::repo::audit::write(&c, "month.close", "fiscal_month", Some(jun), "6월 확정", serde_json::json!({})).unwrap();
    // 설정
    setting::set(&c, "backup.keep", "7").unwrap();
    setting::set(&c, "backup.auto", "1").unwrap();
    setting::set(&c, "backup.encrypt", "dpapi").unwrap();
    c.pragma_update(None, "wal_checkpoint", "TRUNCATE").unwrap();
    drop(c);
    println!("v9 가명 DB 생성: {}", path.display());
}
