//! 대조 시나리오 E~M (지시서 12장). 가명·가상 금액만 쓴다.

use super::*;
use crate::db::Db;
use crate::excel::fixtures_dir;
use crate::model::{NewEntry, PreviewRequest, ProgramMapInput};
use crate::repo::{allocation, catalog, ledger, program, upload, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    fund_ben: i64,
    fund_voucher: i64,
    fund_g3: i64,
    fund_custom: i64,
    item_instructor: i64,
    item_textbook: i64,
    item_material: i64,
    item_tm: i64,
    item_admin: i64,
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
        fund_ben: fund("beneficiary"), fund_voucher: fund("after_voucher"), fund_g3: fund("g3_support"), fund_custom: fund("custom_support"),
        item_instructor: item("instructor"), item_textbook: item("textbook"), item_material: item("material"),
        item_tm: item("textbook_material"), item_admin: item("admin"),
    }
}

impl T {
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    fn entry(&self, program: Option<i64>, group: Option<i64>, fund: Option<i64>, item: i64, flow: &str, kind: &str, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: self.sep, program_id: program, program_group_id: group, student_id: None,
                    fund_type_id: fund, amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None,
                    counterparty: Some("가명".into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn collect(&self, program: i64, fund: i64, item: i64, amount: i64) -> i64 {
        self.entry(Some(program), None, Some(fund), item, "collection", "initial_collect", amount)
    }
    fn pay(&self, program: i64, fund: i64, item: i64, amount: i64) -> i64 {
        self.entry(Some(program), None, Some(fund), item, "expenditure", if item == self.item_instructor { "instructor_pay" } else { "vendor_pay" }, amount)
    }
    fn recon(&self) -> MonthRecon {
        self.db.read(|c| month(c, self.sep)).unwrap()
    }
    fn row<'a>(&self, r: &'a MonthRecon, name: &str, set: &str) -> &'a ReconRow {
        r.rows.iter().find(|x| x.unit_name == name && x.item_set == set).unwrap_or_else(|| panic!("row {name}/{set} not found: {:?}", r.rows.iter().map(|x| (x.unit_name.clone(), x.item_set.clone())).collect::<Vec<_>>()))
    }
}

#[test]
fn scenario_e_f_match_and_mismatch() {
    let t = setup();
    let a = t.program("코딩");
    let b = t.program("로봇과학");
    t.collect(a, t.fund_ben, t.item_instructor, 500_000);
    t.pay(a, t.fund_ben, t.item_instructor, 500_000);
    t.collect(b, t.fund_ben, t.item_instructor, 500_000);
    t.pay(b, t.fund_ben, t.item_instructor, 480_000);
    let r = t.recon();
    let ra = t.row(&r, "코딩", "instructor");
    assert_eq!((ra.status.as_str(), ra.basis, ra.expense, ra.diff), ("ok", 500_000, 500_000, 0));
    let rb = t.row(&r, "로봇과학", "instructor");
    assert_eq!((rb.status.as_str(), rb.diff), ("mismatch", 20_000));
    assert_eq!(rb.status_label, "불일치");
    assert_eq!(r.mismatch_count, 1);
    assert_eq!(r.comparable_basis, 1_000_000);
    assert_eq!(r.comparable_expense, 980_000);
    assert_eq!(r.rows[0].unit_name, "로봇과학", "불일치가 정상보다 앞에 온다");
}

#[test]
fn scenario_g_missing_data_is_never_ok() {
    let t = setup();
    let a = t.program("미술");
    t.pay(a, t.fund_ben, t.item_tm, 500_000);
    let r = t.recon();
    let ra = t.row(&r, "미술", "textbook_material");
    assert_eq!(ra.status, "missing");
    assert_eq!(ra.expense, 500_000);
    assert_eq!(ra.basis, 0);
    assert!(ra.hints.iter().any(|h| h.contains("0원으로 보지 않음")));
    // 징수만 있고 지출이 없어도 자료 부족
    let b = t.program("체스");
    t.collect(b, t.fund_ben, t.item_instructor, 100_000);
    let r = t.recon();
    assert_eq!(t.row(&r, "체스", "instructor").status, "missing");
    assert_eq!(r.comparable_basis, 0);
}

#[test]
fn textbook_and_material_are_compared_as_one_set_and_admin_is_unverifiable() {
    let t = setup();
    let a = t.program("로봇과학");
    t.collect(a, t.fund_ben, t.item_textbook, 100_000);
    t.collect(a, t.fund_ben, t.item_material, 300_000);
    t.collect(a, t.fund_ben, t.item_admin, 12_000);
    t.pay(a, t.fund_ben, t.item_tm, 400_000);
    let r = t.recon();
    let tm = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((tm.status.as_str(), tm.basis, tm.expense, tm.diff), ("ok", 400_000, 400_000, 0));
    let ad = t.row(&r, "로봇과학", "admin");
    assert_eq!(ad.status, "unverifiable");
    assert_eq!(ad.basis, 12_000);
}

#[test]
fn scenario_h_group_is_verified_as_a_whole_and_members_stay_unconfirmed() {
    let t = setup();
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    let gid = t.db.write(|tx| program::group_create(tx, t.year_id, "우쿨렐레&통기타", &[uk, gt])).unwrap();
    t.collect(uk, t.fund_ben, t.item_instructor, 300_000);
    t.collect(gt, t.fund_ben, t.item_instructor, 200_000);
    t.entry(None, Some(gid), Some(t.fund_ben), t.item_instructor, "expenditure", "instructor_pay", 500_000);
    let r = t.recon();
    let g = t.row(&r, "우쿨렐레&통기타 (그룹)", "instructor");
    assert_eq!((g.status.as_str(), g.basis, g.expense, g.diff), ("ok", 500_000, 500_000, 0));
    assert_eq!(t.row(&r, "우쿨렐레", "instructor").status, "group_only");
    assert_eq!(t.row(&r, "통기타", "instructor").status, "group_only");
    assert!(t.row(&r, "우쿨렐레", "instructor").hints.iter().any(|h| h.contains("그룹")));
    // 대시보드 합계에 그룹과 개별이 겹쳐 더해지지 않는다: 비교 가능 기준액은 그룹 한 번뿐.
    assert_eq!(r.comparable_basis, 500_000);
    assert_eq!(r.collection_total, 500_000);
    // 그룹 지급이 어긋나면 그룹이 불일치
    t.entry(None, Some(gid), Some(t.fund_ben), t.item_instructor, "expenditure", "instructor_pay", 10_000);
    let r = t.recon();
    assert_eq!(t.row(&r, "우쿨렐레&통기타 (그룹)", "instructor").diff, -10_000);
    assert_eq!(t.row(&r, "우쿨렐레&통기타 (그룹)", "instructor").status, "mismatch");
}

#[test]
fn scenario_m_merged_funds_compare_once_and_custom_support_is_not_compared() {
    let t = setup();
    let a = t.program("코딩");
    // 징수는 '초3 지원금'(통합된 재원 id)으로, 지급은 '방과후이용권'으로 들어와도 같은 재원이다.
    t.collect(a, t.fund_g3, t.item_instructor, 315_000);
    t.pay(a, t.fund_voucher, t.item_instructor, 315_000);
    // 맞춤형지원은 지급에만 있다 → 비교하지 않고 대조 자료 없음
    t.pay(a, t.fund_custom, t.item_instructor, 400_000);
    let r = t.recon();
    let ra = t.row(&r, "코딩", "instructor");
    assert_eq!(ra.status, "ok", "{:?}", ra.funds);
    let v = ra.funds.iter().find(|f| f.fund_name == "방과후이용권").unwrap();
    assert_eq!((v.basis, v.expense, v.diff), (Some(315_000), Some(315_000), Some(0)));
    assert!(!ra.funds.iter().any(|f| f.fund_name == "초3 지원금"), "통합된 이름으로 한 번만");
    let c = ra.funds.iter().find(|f| f.fund_name == "맞춤형지원").unwrap();
    assert_eq!((c.basis, c.expense, c.state.as_str()), (None, Some(400_000), "no_basis"));
    assert!(ra.hints.iter().any(|h| h.contains("맞춤형지원")));
    assert_eq!(ra.expense, 715_000, "표시 합계에는 들어가되");
    assert_eq!(ra.diff, 0, "차액은 비교 가능한 재원끼리만");
    assert_eq!(ra.status_label, "비교 가능 재원 정상 · 맞춤형지원 미검증");
    assert_eq!(r.comparable_expense, 315_000, "비교 가능 금액에는 빠진다");
}

#[test]
fn unmapped_fund_blocks_fund_level_comparison() {
    let t = setup();
    let a = t.program("한자");
    t.entry(Some(a), None, None, t.item_instructor, "collection", "initial_collect", 50_000);
    t.pay(a, t.fund_ben, t.item_instructor, 50_000);
    let r = t.recon();
    assert_eq!(t.row(&r, "한자", "instructor").status, "unmapped");
}

#[test]
fn unallocated_collection_marks_programs_as_pending_allocation() {
    let t = setup();
    let a = t.program("코딩");
    t.collect(a, t.fund_ben, t.item_instructor, 100_000);
    t.pay(a, t.fund_ben, t.item_instructor, 100_000);
    // 프로그램 없는 징수 거래 하나가 남아 있으면 같은 항목의 프로그램은 배분 미확정
    let e = t.entry(None, None, Some(t.fund_ben), t.item_instructor, "collection", "initial_collect", 30_000);
    let r = t.recon();
    assert_eq!(t.row(&r, "코딩", "instructor").status, "unallocated");
    assert_eq!(r.unallocated_total, 30_000);
    // 다른 항목(교재비)은 영향이 없다
    t.collect(a, t.fund_ben, t.item_tm, 10_000);
    t.pay(a, t.fund_ben, t.item_tm, 10_000);
    let r = t.recon();
    assert_eq!(t.row(&r, "코딩", "textbook_material").status, "ok");
    // 배분하면 기준액에 들어가고 상태가 바뀐다
    let b = t.program("미술");
    t.db.write(|tx| allocation::manual(tx, e, &[(b, 30_000)], "확인")).unwrap();
    let r = t.recon();
    assert_eq!(r.unallocated_total, 0);
    assert_eq!(t.row(&r, "코딩", "instructor").status, "ok");
    let rb = t.row(&r, "미술", "instructor");
    assert_eq!((rb.status.as_str(), rb.basis, rb.allocated_in), ("missing", 30_000, 30_000));
}

#[test]
fn scenario_k_note_survives_and_needs_recheck_when_data_changes() {
    let t = setup();
    let a = t.program("로봇과학");
    t.collect(a, t.fund_ben, t.item_tm, 1_080_000);
    t.pay(a, t.fund_ben, t.item_tm, 1_260_000);
    let r = t.recon();
    let row = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((row.status.as_str(), row.diff), ("mismatch", -180_000));
    assert_eq!(row.note_status, "none");
    assert_eq!(r.todo_count, 1);
    let n = t.db.write(|tx| note_save(tx, t.sep, "program", a, "textbook_material", "explained", "9월 전환 기준 차이, 담당자 확인")).unwrap();
    assert_eq!(n.fingerprint, row.fingerprint);
    let r = t.recon();
    let row = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!((row.note_status.as_str(), row.needs_recheck, row.status.as_str()), ("explained", false, "mismatch"), "사유 확인이 검증 결과를 바꾸지 않는다");
    assert_eq!(r.todo_count, 0);
    // 환불이 들어와 차액이 바뀌면 재확인 필요. 메모는 남는다.
    t.entry(Some(a), None, Some(t.fund_ben), t.item_material, "collection", "refund", -30_000);
    let r = t.recon();
    let row = t.row(&r, "로봇과학", "textbook_material");
    assert_eq!(row.diff, -210_000);
    assert!(row.needs_recheck);
    assert_eq!(row.note_status, "explained");
    assert_eq!(r.todo_count, 1);
    let saved = t.db.read(|c| notes(c, t.sep, "program", a, "textbook_material")).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].reason.contains("담당자 확인"));
    // 새 확인을 적으면 이전 메모는 그대로 남고 둘이 된다.
    t.db.write(|tx| note_save(tx, t.sep, "program", a, "textbook_material", "investigating", "환불 반영 뒤 재조사")).unwrap();
    assert_eq!(t.db.read(|c| notes(c, t.sep, "program", a, "textbook_material")).unwrap().len(), 2);
    assert!(!t.row(&t.recon(), "로봇과학", "textbook_material").needs_recheck);
    assert_eq!(t.db.write(|tx| note_save(tx, t.sep, "program", a, "textbook_material", "ok", "")).unwrap_err().code, "INVALID");
}

#[test]
fn scenario_l_detail_traces_entries_to_source_rows_and_keeps_voided_history() {
    let t = setup();
    // 실제 업로드로 거래를 만들어 원본 행까지 잇는다.
    t.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: t.year_id, alias: "코딩".into(), target: "new_program".into(), program_id: None, group_id: None, new_name: None, member_ids: None })).unwrap();
    for n in ["로봇과학", "항공드론", "우쿨렐레&통기타"] {
        t.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: t.year_id, alias: n.into(), target: "new_program".into(), program_id: None, group_id: None, new_name: None, member_ids: None })).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let req = PreviewRequest { path: fixtures_dir().join("instructor_pay.xlsx").display().to_string(), school_year_id: t.year_id, fiscal_month_id: t.sep, format_code: "instructor_pay".into(), data_kind: "instructor_pay".into(), overrides: vec![], replace_file_id: None, note: None };
    let d = dir.path().to_path_buf();
    let r = t.db.write(|tx| upload::commit(tx, &d, &req)).unwrap();
    let coding = t.db.read(|c| program::resolve(c, t.year_id, "코딩")).unwrap().unwrap();
    let det = t.db.read(|c| detail(c, t.sep, "program", coding, "instructor")).unwrap();
    assert_eq!(det.row.status, "missing");
    assert_eq!(det.expenditure.len(), 4, "코딩 행의 재원 4칸");
    let e = &det.expenditure[0];
    assert_eq!((e.file_name.as_str(), e.version_no, e.sheet.as_str(), e.row_no), ("instructor_pay.xlsx", 1, "강사료내역", 4));
    assert_eq!(e.party, "강사가명1");
    assert_eq!(e.kind_label, "강사료 지급");
    assert!(e.source_row_id.is_some());
    let src = t.db.read(|c| source_row(c, e.source_row_id.unwrap())).unwrap();
    assert_eq!(src.cells[1], "코딩");
    assert_eq!(src.row_no, 4);
    assert!(!src.stored_name.is_empty());
    assert!(det.parties.iter().any(|p| p.name == "강사가명1" && p.total == 995_000));
    // 파일을 취소하면 거래는 이력으로만 남고 합계에서 빠진다.
    t.db.write(|tx| upload::cancel(tx, r.file.id, "잘못 올림")).unwrap();
    let det = t.db.read(|c| detail(c, t.sep, "program", coding, "instructor")).unwrap();
    assert_eq!(det.row.expense, 0);
    assert_eq!(det.expenditure.len(), 4);
    assert!(det.expenditure.iter().all(|e| e.state == "voided" && e.file_status == "cancelled"));
    assert!(det.parties.is_empty());
}

#[test]
fn scenario_i_j_replace_and_restore_recompute_allocations_and_diffs() {
    use crate::excel::parse::StudentKey;
    use crate::repo::student;
    let t = setup();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_path_buf();
    // 1. 최초 징수 v1 (징수명단 — 프로그램 없음)
    let v1 = t.db.write(|tx| upload::commit(tx, &d, &req(&t, "collect_initial.xlsx", "collect_initial", "initial_collect", None))).unwrap();
    // 2. 프로그램별 수강생명단(roster_link) → 자동 배분
    for n in ["코딩A", "코딩B", "로봇과학A", "항공드론A"] {
        t.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: t.year_id, alias: n.into(), target: "new_program".into(), program_id: None, group_id: None, new_name: None, member_ids: None })).unwrap();
    }
    t.db.write(|tx| upload::commit(tx, &d, &req(&t, "roster_manager.xlsx", "roster_manager", "roster_link", None))).unwrap();
    let a = t.db.write(|tx| allocation::auto_allocate(tx, t.sep)).unwrap();
    assert!(a.entries_allocated > 0);
    let board = t.db.read(|c| allocation::pending(c, t.sep)).unwrap();
    let unallocated_v1 = board.unallocated_total;
    assert!(unallocated_v1 > 0, "참고 명단에 없는 학생(가명F·G)의 금액은 미배분으로 남는다");
    let coding = t.db.read(|c| program::resolve(c, t.year_id, "코딩A")).unwrap().unwrap();
    // 3. 강사료 지급 (코딩A 에 직접)
    t.pay(coding, t.fund_ben, t.item_instructor, 70_000);
    // 4. 차액 분석
    let r1 = t.recon();
    let row1 = t.row(&r1, "코딩A", "instructor");
    assert_eq!(row1.status, "unallocated", "미배분이 남아 있는 동안은 배분 미확정");
    assert_eq!(row1.allocated_in, 70_000, "가명A(수익자)·가명B(이용권) 강사료 35,000 × 2");
    let allocs_before: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM allocation WHERE status = 'active'", [], |r| r.get(0))?)).unwrap();
    // 5. 최초 징수명단을 v2 로 교체 → 기존 배분은 취소된 거래를 가리키므로 효력이 없다
    let v2 = t.db.write(|tx| upload::commit(tx, &d, &req(&t, "collect_initial_v2.xlsx", "collect_initial", "initial_collect", Some(v1.file.id)))).unwrap();
    let active_allocs: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM allocation WHERE status = 'active'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(active_allocs, 0, "교체된 판의 배분은 함께 접힌다");
    let total_allocs: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM allocation", [], |r| r.get(0))?)).unwrap();
    assert_eq!(total_allocs, allocs_before, "배분 기록은 지우지 않는다");
    let r2 = t.recon();
    assert_eq!(t.row(&r2, "코딩A", "instructor").allocated_in, 0);
    assert_eq!(r2.unallocated_total, 479_400, "v2 전액이 다시 미배분");
    // 6. 재배분 (규칙 재사용은 자동이 아니라 다시 실행)
    t.db.write(|tx| allocation::auto_allocate(tx, t.sep)).unwrap();
    let r3 = t.recon();
    assert_eq!(t.row(&r3, "코딩A", "instructor").allocated_in, 65_000, "v2 에서 가명A 강사료가 30,000");
    // 8. v1 복원 → 배분 다시, 중복 없음
    t.db.write(|tx| upload::restore(tx, v1.file.id, "v2 오류")).unwrap();
    assert_eq!(t.recon().unallocated_total, 484_400);
    t.db.write(|tx| allocation::auto_allocate(tx, t.sep)).unwrap();
    let r4 = t.recon();
    assert_eq!(t.row(&r4, "코딩A", "instructor").allocated_in, 70_000);
    assert_eq!(r4.unallocated_total, unallocated_v1);
    assert_eq!(r4.collection_total, 484_400);
    let live: i64 = t.db.read(|c| Ok(c.query_row("SELECT coalesce(sum(a.amount),0) FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id WHERE a.status='active' AND e.state='active'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(live, 484_400 - unallocated_v1, "살아 있는 배분 합 = 배분된 징수액, 중복 없음");
    let _ = v2;
    // 학생 식별이 같은 키로 이어진다
    let sid = t.db.write(|tx| student::upsert(tx, t.year_id, &StudentKey { grade: Some(1), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() })).unwrap();
    assert!(sid.is_some());
}

fn req(t: &T, file: &str, format: &str, kind: &str, replace: Option<i64>) -> PreviewRequest {
    PreviewRequest {
        path: fixtures_dir().join(file).display().to_string(),
        school_year_id: t.year_id,
        fiscal_month_id: t.sep,
        format_code: format.into(),
        data_kind: kind.into(),
        overrides: vec![],
        replace_file_id: replace,
        note: None,
    }
}
