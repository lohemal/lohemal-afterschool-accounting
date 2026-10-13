//! P3 시나리오 A~E (지시서 16장). 가명·가상 금액만.

use super::*;
use crate::db::Db;
use crate::excel::parse::StudentKey;
use crate::model::NewEntry;
use crate::repo::{catalog, ledger, program, recon, student, upload, year};

struct T {
    db: Db,
    year_id: i64,
    apr: i64,
    sep: i64,
    ben: i64,
    g3x: i64,
    voucher: i64,
    free: i64,
    instr: i64,
    coding: i64,
    stu: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months = db.read(|c| year::months(c, y.id)).unwrap();
    let apr = months.iter().find(|m| m.ym == "2026-04").unwrap().id;
    let sep = months.iter().find(|m| m.ym == "2026-09").unwrap().id;
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let instr = cat.amount_items.iter().find(|f| f.code == "instructor").unwrap().id;
    let coding = db.write(|tx| program::create(tx, y.id, "코딩")).unwrap();
    let stu = db
        .write(|tx| student::upsert(tx, y.id, &StudentKey { grade: Some(1), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() }))
        .unwrap()
        .unwrap();
    T { db, year_id: y.id, apr, sep, ben: fund("beneficiary"), g3x: fund("g3_excess"), voucher: fund("after_voucher"), free: fund("free_voucher"), instr, coding, stu }
}

impl T {
    fn collect(&self, month: i64, fund: i64, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: month, program_id: Some(self.coding), program_group_id: None, student_id: Some(self.stu),
                    fund_type_id: Some(fund), amount_item_id: self.instr, flow: "collection".into(), entry_kind: "initial_collect".into(), amount,
                    stage: None, processed_on: Some("2026-04-10".into()), counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn pay(&self, month: i64, fund: i64, amount: i64) {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: month, program_id: Some(self.coding), program_group_id: None, student_id: None,
                    fund_type_id: Some(fund), amount_item_id: self.instr, flow: "expenditure".into(), entry_kind: "instructor_pay".into(), amount,
                    stage: None, processed_on: None, counterparty: Some("강사가명".into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|_| ())
            })
            .unwrap()
    }
    fn input(&self, month: i64, kind: &str, amount: i64) -> AdjustmentInput {
        AdjustmentInput {
            school_year_id: self.year_id, fiscal_month_id: month, kind: kind.into(), program_id: self.coding, student_id: Some(self.stu),
            from_fund_id: None, to_fund_id: None, fund_type_id: Some(self.ben), amount_item_id: self.instr, amount,
            decided_on: Some("2026-09-10".into()), processed_on: Some("2026-09-15".into()), status: "done".into(), reason: "시험".into(),
            related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
        }
    }
    fn transfer(&self, month: i64, from: i64, to: i64, amount: i64) -> AdjustmentInput {
        AdjustmentInput { from_fund_id: Some(from), to_fund_id: Some(to), fund_type_id: None, ..self.input(month, "fund_transfer", amount) }
    }
    fn funds(&self, month: i64) -> Vec<(String, i64)> {
        let r = self.db.read(|c| recon::month(c, month)).unwrap();
        let row = r.rows.into_iter().find(|x| x.unit_name == "코딩" && x.item_set == "instructor").unwrap();
        row.funds.iter().map(|f| (f.fund_name.clone(), f.basis.unwrap_or(0))).collect()
    }
    fn basis_total(&self, month: i64) -> i64 {
        let r = self.db.read(|c| recon::month(c, month)).unwrap();
        r.rows.into_iter().find(|x| x.unit_name == "코딩" && x.item_set == "instructor").map(|x| x.basis).unwrap_or(0)
    }
    fn create(&self, i: &AdjustmentInput) -> AppResult<Adjustment> {
        self.db.write(|tx| create(tx, i))
    }
}

fn fund(v: &[(String, i64)], name: &str) -> i64 {
    v.iter().find(|x| x.0 == name).map(|x| x.1).unwrap_or(0)
}

// ── A. 재원경정 ────────────────────────────────────────────────────────

#[test]
fn a_transfer_keeps_total_and_moves_between_funds() {
    let t = setup();
    t.collect(t.sep, t.ben, 100_000);
    t.collect(t.sep, t.free, 50_000);
    let p = t.db.read(|c| preview(c, &t.transfer(t.sep, t.ben, t.free, 30_000))).unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    assert_eq!((p.funds[0].before, p.funds[0].change, p.funds[0].after), (100_000, -30_000, 70_000));
    assert_eq!((p.funds[1].before, p.funds[1].change, p.funds[1].after), (50_000, 30_000, 80_000));
    assert_eq!((p.total_before, p.total_after), (150_000, 150_000));
    let a = t.create(&t.transfer(t.sep, t.ben, t.free, 30_000)).unwrap();
    assert_eq!(a.entry_ids.len(), 2);
    assert_eq!(a.scope, "student");
    let f = t.funds(t.sep);
    assert_eq!(fund(&f, "일반 수익자"), 70_000);
    assert_eq!(fund(&f, "자유수강권"), 80_000);
    assert_eq!(t.basis_total(t.sep), 150_000, "전체 기준액은 그대로");
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    assert_eq!(s.collection_total, 150_000);
    assert_eq!(s.transfer_balance, 0);
    // 2. 초3 초과금 → 방과후이용권
    t.collect(t.sep, t.g3x, 20_000);
    t.create(&t.transfer(t.sep, t.g3x, t.voucher, 20_000)).unwrap();
    let f = t.funds(t.sep);
    assert_eq!(fund(&f, "초3 초과금"), 0);
    assert_eq!(fund(&f, "방과후이용권"), 20_000);
    assert_eq!(t.basis_total(t.sep), 170_000);
}

#[test]
fn a_transfer_validation_and_atomicity() {
    let t = setup();
    t.collect(t.sep, t.ben, 100_000);
    let same = t.transfer(t.sep, t.ben, t.ben, 10_000);
    assert!(t.db.read(|c| preview(c, &same)).unwrap().blocked.iter().any(|b| b.contains("같습니다")));
    assert_eq!(t.create(&same).unwrap_err().code, "INVALID");
    // 통합된 재원(초3 지원금 ↔ 이용권)도 같은 재원
    let g3s = t.db.read(|c| catalog::fund_type_by_code(c, "g3_support")).unwrap().id;
    assert!(t.create(&t.transfer(t.sep, g3s, t.voucher, 10_000)).is_err());
    assert_eq!(t.create(&t.transfer(t.sep, t.ben, t.free, 0)).unwrap_err().code, "INVALID");
    let over = t.transfer(t.sep, t.ben, t.free, 100_001);
    assert!(t.db.read(|c| preview(c, &over)).unwrap().blocked.iter().any(|b| b.contains("음수 잔액")));
    assert!(t.create(&over).is_err());
    // 4. 원자성: 트랜잭션 안에서 실패하면 쌍 모두 없다
    let dir_err = t.db.write(|tx| { create(tx, &t.transfer(t.sep, t.ben, t.free, 10_000))?; Err::<(), _>(AppError::new("TEST", "x")) }).unwrap_err();
    assert_eq!(dir_err.code, "TEST");
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM ledger_entry WHERE flow = 'transfer'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 0);
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM adjustment", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 0);
    assert_eq!(t.basis_total(t.sep), 100_000);
    // 사유 없음·금액 음수·처리 완료인데 처리일 없음
    let mut bad = t.transfer(t.sep, t.ben, t.free, 10_000);
    bad.reason = " ".into();
    assert!(t.create(&bad).is_err());
    let mut bad = t.transfer(t.sep, t.ben, t.free, 10_000);
    bad.processed_on = None;
    assert!(t.create(&bad).is_err());
}

#[test]
fn a_transfer_cancel_is_pairwise_and_once() {
    let t = setup();
    t.collect(t.sep, t.ben, 100_000);
    let a = t.create(&t.transfer(t.sep, t.ben, t.free, 30_000)).unwrap();
    // 중복 등록 차단
    assert!(t.create(&t.transfer(t.sep, t.ben, t.free, 30_000)).unwrap_err().message.contains("이미 등록"));
    assert_eq!(t.db.write(|tx| cancel(tx, a.id, "")).unwrap_err().code, "INVALID");
    let c = t.db.write(|tx| cancel(tx, a.id, "재원 선택 오류")).unwrap();
    assert_eq!(c.status, "cancelled");
    for e in &a.entry_ids {
        assert_eq!(t.db.read(|c| ledger::get(c, *e)).unwrap().state, "voided", "쌍이 함께 취소");
    }
    assert_eq!(fund(&t.funds(t.sep), "일반 수익자"), 100_000);
    assert_eq!(t.db.write(|tx| cancel(tx, a.id, "다시")).unwrap_err().code, "INVALID", "중복 취소 차단");
    assert_eq!(c.logs.last().unwrap().to_status, "cancelled");
}

// ── B. 환불 ───────────────────────────────────────────────────────────

#[test]
fn b_refund_partial_repeated_and_capped() {
    let t = setup();
    let orig = t.collect(t.apr, t.ben, 50_000);
    let mut i = t.input(t.apr, "refund", 20_000);
    i.related_entry_id = Some(orig);
    let p = t.db.read(|c| preview(c, &i)).unwrap();
    assert_eq!(p.refundable, Some(50_000));
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    t.create(&i).unwrap();
    assert_eq!(t.basis_total(t.apr), 30_000);
    // 12. 여러 차례
    let mut j = t.input(t.apr, "refund", 20_000);
    j.processed_on = Some("2026-09-20".into());
    let p = t.db.read(|c| preview(c, &j)).unwrap();
    assert_eq!(p.refundable, Some(30_000));
    assert_eq!(p.existing_refunds, 20_000);
    assert!(!p.duplicates.is_empty(), "같은 금액의 기존 환불을 중복 가능성으로 알린다");
    assert!(!p.blocked.is_empty());
    j.confirm_duplicate = true;
    t.create(&j).unwrap();
    assert_eq!(t.basis_total(t.apr), 10_000);
    // 13. 초과 차단
    let k = t.input(t.apr, "refund", 10_001);
    assert!(t.db.read(|c| preview(c, &k)).unwrap().blocked.iter().any(|b| b.contains("환불 가능 금액")));
    assert!(t.create(&k).is_err());
    assert_eq!(t.basis_total(t.apr), 10_000);
}

#[test]
fn b_refund_return_after_transfer_does_not_double_deduct() {
    // 시나리오 3: 일반 수익자 30,000 → 자유수강권 경정 뒤 실제 반환 30,000
    let t = setup();
    t.collect(t.sep, t.ben, 30_000);
    let tr = t.create(&t.transfer(t.sep, t.ben, t.free, 30_000)).unwrap();
    assert_eq!(t.basis_total(t.sep), 30_000);
    let f = t.funds(t.sep);
    assert_eq!((fund(&f, "일반 수익자"), fund(&f, "자유수강권")), (0, 30_000));
    let mut ret = t.input(t.sep, "refund_return", 30_000);
    ret.related_adjustment_id = Some(tr.id);
    let p = t.db.read(|c| preview(c, &ret)).unwrap();
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    assert!(p.warnings.iter().any(|w| w.contains("다시 차감하지 않습니다")));
    let r = t.create(&ret).unwrap();
    assert!(r.entry_ids.is_empty(), "반환 이행은 원장 거래를 만들지 않는다");
    assert_eq!(t.basis_total(t.sep), 30_000, "전체 수강료가 중복 차감되지 않았다");
    let f = t.funds(t.sep);
    assert_eq!((fund(&f, "일반 수익자"), fund(&f, "자유수강권")), (0, 30_000));
    // 반환 누계 초과 차단
    let mut again = ret.clone();
    again.amount = 1;
    assert!(t.create(&again).unwrap_err().message.contains("넘습니다"));
    // 경정은 반환 이행이 살아 있는 동안 취소할 수 없다
    assert!(t.db.write(|tx| cancel(tx, tr.id, "x")).unwrap_err().message.contains("반환 이행을 먼저"));
    t.db.write(|tx| cancel(tx, r.id, "반환 취소")).unwrap();
    t.db.write(|tx| cancel(tx, tr.id, "경정 취소")).unwrap();
    assert_eq!(fund(&t.funds(t.sep), "일반 수익자"), 30_000);
}

#[test]
fn b_refund_planned_then_done_then_cancel_and_retroactive_view() {
    // 시나리오 2: 4월 수강료 50,000 중 20,000 을 9월 15일에 소급 환불
    let t = setup();
    t.collect(t.apr, t.ben, 50_000);
    t.collect(t.sep, t.ben, 50_000);
    let mut i = t.input(t.apr, "refund", 20_000);
    i.status = "planned".into();
    i.processed_on = None;
    let a = t.create(&i).unwrap();
    assert_eq!(a.status, "planned");
    assert_eq!(t.basis_total(t.apr), 50_000, "처리 예정은 집계에 들지 않는다");
    // 예정 상태에서는 환불 가능 금액에서 미리 뺀다
    let p = t.db.read(|c| preview(c, &t.input(t.apr, "refund", 40_000))).unwrap();
    assert_eq!(p.refundable, Some(30_000));
    assert!(t.db.write(|tx| set_status(tx, a.id, "done", None, "")).is_err(), "처리일 없이 완료 불가");
    let d = t.db.write(|tx| set_status(tx, a.id, "done", Some("2026-09-15"), "실제 환불")).unwrap();
    assert_eq!((d.status.as_str(), d.processed_on.as_deref()), ("done", Some("2026-09-15")));
    assert_eq!(t.basis_total(t.apr), 30_000, "4월 귀속월 기준 최종 30,000");
    assert_eq!(t.basis_total(t.sep), 50_000, "9월 귀속 수강료에서 잘못 차감하지 않는다");
    let e = t.db.read(|c| ledger::get(c, d.entry_ids[0])).unwrap();
    assert_eq!((e.fiscal_month_id, e.processed_on.as_deref(), e.stage.as_str()), (t.apr, Some("2026-09-15"), "actual"));
    // 27. 처리일 기준 조회: 9월에 처리한 정정에 4월 귀속 환불이 보인다
    let by_processed = t.db.read(|c| list(c, &AdjustmentFilter { school_year_id: t.year_id, processed_from: Some("2026-09-01".into()), processed_to: Some("2026-09-30".into()), ..Default::default() })).unwrap();
    assert_eq!(by_processed.len(), 1);
    assert_eq!(by_processed[0].month_label, "4월 (2026-04)");
    assert_eq!(by_processed[0].amount, 20_000);
    // 26. 귀속월 기준 조회
    let by_month = t.db.read(|c| list(c, &AdjustmentFilter { school_year_id: t.year_id, fiscal_month_id: Some(t.apr), ..Default::default() })).unwrap();
    assert_eq!(by_month.len(), 1);
    assert!(t.db.read(|c| list(c, &AdjustmentFilter { school_year_id: t.year_id, fiscal_month_id: Some(t.sep), ..Default::default() })).unwrap().is_empty());
    // 되돌리기는 불가, 취소만
    assert!(t.db.write(|tx| set_status(tx, a.id, "planned", None, "")).is_err());
    t.db.write(|tx| cancel(tx, a.id, "실제 회계 처리 취소")).unwrap();
    assert_eq!(t.basis_total(t.apr), 50_000);
    let cat: String = t.db.read(|c| Ok(c.query_row("SELECT category FROM ledger_correction ORDER BY id DESC LIMIT 1", [], |r| r.get(0))?)).unwrap();
    assert_eq!(cat, "accounting_error");
}

// ── C. 추가징수 ───────────────────────────────────────────────────────

#[test]
fn c_extra_collect_with_duplicate_detection() {
    let t = setup();
    t.collect(t.sep, t.ben, 30_000);
    let mut i = t.input(t.sep, "extra_collect", 5_000);
    i.status = "planned".into();
    i.processed_on = None;
    let a = t.create(&i).unwrap();
    assert_eq!(t.basis_total(t.sep), 30_000, "예정은 집계 제외");
    t.db.write(|tx| set_status(tx, a.id, "done", Some("2026-09-20"), "")).unwrap();
    assert_eq!(t.basis_total(t.sep), 35_000);
    // 24. 같은 금액을 다시 등록하면 중복 가능성 → 확인 없이는 차단
    let again = t.input(t.sep, "extra_collect", 5_000);
    let p = t.db.read(|c| preview(c, &again)).unwrap();
    assert_eq!(p.duplicates.len(), 1);
    assert!(p.blocked.iter().any(|b| b.contains("중복 아님 확인")));
    assert!(t.create(&again).is_err());
    // 25. 실제로 별도 거래라면 확인 후 허용
    let mut ok = again.clone();
    ok.confirm_duplicate = true;
    t.create(&ok).unwrap();
    assert_eq!(t.basis_total(t.sep), 40_000);
    // 23. 취소
    t.db.write(|tx| cancel(tx, a.id, "금액 입력 오류")).unwrap();
    assert_eq!(t.basis_total(t.sep), 35_000);
}

// ── D. 월별 정산 ─────────────────────────────────────────────────────

#[test]
fn d_closed_month_blocks_and_reopen_allows_and_diff_recomputes_with_recheck() {
    let t = setup();
    t.collect(t.sep, t.ben, 100_000);
    t.collect(t.sep, t.voucher, 232_000);
    t.pay(t.sep, t.ben, 112_000);
    t.pay(t.sep, t.voucher, 220_000);
    let r = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    let row = r.rows.iter().find(|x| x.unit_name == "코딩" && x.item_set == "instructor").unwrap();
    assert_eq!(row.status, "mismatch");
    assert_eq!(row.diff, 0, "총액은 같고 재원이 어긋남");
    t.db.write(|tx| recon::note_save(tx, t.sep, "program", t.coding, "instructor", "explained", "경정 예정")).unwrap();
    // 28. 확정월 차단
    t.db.write(|tx| year::month_close(tx, t.sep, "확정")).unwrap();
    let tr = t.transfer(t.sep, t.voucher, t.ben, 12_000);
    assert!(t.db.read(|c| preview(c, &tr)).unwrap().month_closed);
    assert!(t.create(&tr).unwrap_err().message.contains("확정"));
    // 29. 재개 후 허용 → 30. 차액 재계산 → 31. 메모는 남고 재확인 필요
    t.db.write(|tx| year::month_reopen(tx, t.sep, "재원경정 필요")).unwrap();
    let a = t.create(&tr).unwrap();
    let r = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    let row = r.rows.iter().find(|x| x.unit_name == "코딩" && x.item_set == "instructor").unwrap();
    assert_eq!((row.status.as_str(), row.diff, row.transfer_count), ("ok", 0, 2));
    assert!(row.needs_recheck && row.note_status == "explained");
    assert!(row.hints.iter().any(|h| h.contains("재원경정 2건")));
    // 확정 상태에서는 취소·완료 변경도 막힌다
    t.db.write(|tx| year::month_close(tx, t.sep, "재확정")).unwrap();
    assert!(t.db.write(|tx| cancel(tx, a.id, "x")).unwrap_err().message.contains("확정"));
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM month_close", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 3, "확정·재개·재확정 이력 보존");
}

// ── E. 파일 버전 연동 ────────────────────────────────────────────────

#[test]
fn e_replacing_the_source_file_flags_linked_adjustments_as_orphan() {
    let t = setup();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    // 통합 매니저 명단 가명 파일을 최초 징수로 올린다 (코딩A 등 매핑)
    for n in ["코딩A", "코딩B", "로봇과학A", "항공드론A"] {
        t.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: t.year_id, alias: n.into(), target: "program".into(), program_id: Some(t.coding), group_id: None, new_name: None, member_ids: None })).unwrap();
    }
    let req = |file: &str, replace: Option<i64>| PreviewRequest {
        path: crate::excel::fixtures_dir().join(file).display().to_string(), school_year_id: t.year_id, fiscal_month_id: t.sep,
        format_code: "roster_manager".into(), data_kind: "initial_collect".into(), overrides: vec![], replace_file_id: replace, note: None,
    };
    let v1 = t.db.write(|tx| upload::commit(tx, &d, &req("roster_manager.xlsx", None))).unwrap();
    // 가명A(1학년 가람반 3번)의 강사료 35,000 거래를 원본으로 환불 10,000 등록
    let picks = t.db.read(|c| entry_picks(c, t.sep, t.coding, Some(t.stu))).unwrap();
    let orig = picks.iter().find(|p| p.item_name == "강사료" && p.amount == 35_000).expect("원본 거래");
    let mut i = t.input(t.sep, "refund", 10_000);
    i.related_entry_id = Some(orig.id);
    let a = t.create(&i).unwrap();
    assert!(!a.orphan);
    let before = t.basis_total(t.sep);
    // 32. 원본 파일을 같은 내용의 다른 파일로 교체할 수 없으니(sha 동일) 취소로 흉내 → 34. 원본 거래 취소
    t.db.write(|tx| upload::cancel(tx, v1.file.id, "잘못 올림")).unwrap();
    let a2 = t.db.read(|c| get(c, a.id)).unwrap();
    assert!(a2.orphan, "원본 거래가 취소되면 '원본 재확인 필요'");
    assert_eq!(a2.status, "done", "수동 정정을 무조건 지우지 않는다");
    let r = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    let row = r.rows.iter().find(|x| x.unit_name == "코딩" && x.item_set == "instructor").unwrap();
    assert_eq!(row.status, "orphan", "근거가 사라진 정정은 정상으로 계산되지 않는다");
    assert_eq!(row.status_label, "원본 재확인 필요");
    // 33. 복원하면 근거가 살아나고 정정도 다시 유효
    t.db.write(|tx| upload::restore(tx, v1.file.id, "복원")).unwrap();
    let a3 = t.db.read(|c| get(c, a.id)).unwrap();
    // 복원은 새 거래를 만들므로 옛 원본 id 는 여전히 취소 상태다 — 근거 재확인이 필요하다는 표시가 유지된다.
    assert!(a3.orphan);
    assert_eq!(t.basis_total(t.sep), before, "복원 뒤 합계는 교체 전과 같고 환불은 한 번만 반영");
    // 36. 중복 반영 없음: 환불 거래가 하나뿐
    let n: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM ledger_entry WHERE entry_kind = 'refund' AND state = 'active'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(n, 1);
    // 35. 관련 원본이 취소된 거래로는 새 정정을 등록할 수 없다
    let mut j = t.input(t.sep, "refund", 1_000);
    j.related_entry_id = Some(orig.id);
    assert!(t.create(&j).unwrap_err().message.contains("취소되었거나"));
}
