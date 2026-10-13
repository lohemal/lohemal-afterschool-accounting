//! 배분 시나리오 A~D·N (지시서 12장). 가명 자료만 쓴다.

use rusqlite::params;

use super::*;
use crate::db::Db;
use crate::excel::parse::StudentKey;
use crate::model::NewEntry;
use crate::repo::{catalog, ledger, program, student, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    fund_ben: i64,
    fund_voucher: i64,
    item_instructor: i64,
    roster_file: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let sep = db.read(|c| year::months(c, y.id)).unwrap().iter().find(|m| m.ym == "2026-09").unwrap().id;
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    // 참고 명단(roster_link) 파일 껍데기
    let roster_file = db
        .write(|tx| {
            tx.execute(
                "INSERT INTO upload_file (school_year_id, file_name, sha256, byte_size, format_code, status, data_kind, fiscal_month_id, applied_at)
                 VALUES (?1, 'roster.xlsx', ?2, 1, 'roster_manager', 'applied', 'roster_link', ?3, datetime('now'))",
                params![y.id, "a".repeat(64), sep],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap();
    T { db, year_id: y.id, sep, fund_ben: fund("beneficiary"), fund_voucher: fund("after_voucher"), item_instructor: item("instructor"), roster_file }
}

fn key(name: &str, grade: i32, class: &str) -> StudentKey {
    StudentKey { grade: Some(grade), class_name: class.into(), student_no: "1".into(), name: name.into() }
}

impl T {
    fn student(&self, name: &str, grade: i32, class: &str) -> i64 {
        self.db.write(|tx| student::upsert(tx, self.year_id, &key(name, grade, class))).unwrap().unwrap()
    }
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    /// 미배분 징수 거래 (프로그램 없음)
    fn collect(&self, student_id: Option<i64>, amount: i64, fund: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(
                    tx,
                    &NewEntry {
                        school_year_id: self.year_id, fiscal_month_id: self.sep, program_id: None, program_group_id: None,
                        student_id, fund_type_id: Some(fund), amount_item_id: self.item_instructor,
                        flow: "collection".into(), entry_kind: "initial_collect".into(), amount, stage: None, processed_on: None,
                        counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
                    },
                )
                .map(|e| e.id)
            })
            .unwrap()
    }
    /// 참고 명단 행 (학생·프로그램·강사료 금액)
    fn roster(&self, student_id: Option<i64>, name: &str, program_id: i64, amount: i64) {
        self.db
            .write(|tx| {
                tx.execute(
                    "INSERT INTO upload_row (upload_file_id, seq, sheet_name, row_no, program_text, program_id, student_name, student_id,
                                             amount_item_code, entry_kind, flow, amount, status, included)
                     VALUES (?1, (SELECT coalesce(max(seq),0)+1 FROM upload_row WHERE upload_file_id = ?1), 'r', 1, '', ?2, ?3, ?4, 'instructor', 'initial_collect', 'collection', ?5, 'ok', 0)",
                    params![self.roster_file, program_id, name, student_id, amount],
                )?;
                Ok(())
            })
            .unwrap()
    }
    fn board(&self) -> AllocationBoard {
        self.db.read(|c| pending(c, self.sep)).unwrap()
    }
    fn auto(&self) -> AutoAllocateResult {
        self.db.write(|tx| auto_allocate(tx, self.sep)).unwrap()
    }
    fn sum_alloc(&self, program_id: i64) -> i64 {
        self.db
            .read(|c| Ok(c.query_row("SELECT coalesce(sum(a.amount),0) FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id WHERE a.program_id = ?1 AND a.status = 'active' AND e.state = 'active'", [program_id], |r| r.get(0))?))
            .unwrap()
    }
}

#[test]
fn scenario_a_split_across_two_programs_without_new_collection() {
    let t = setup();
    let s = t.student("가명A", 1, "가람");
    let coding = t.program("코딩");
    let art = t.program("미술");
    let e = t.collect(Some(s), 60_000, t.fund_ben);
    t.roster(Some(s), "가명A", coding, 30_000);
    t.roster(Some(s), "가명A", art, 30_000);
    let b = t.board();
    assert_eq!(b.entries[0].plan, "auto");
    assert_eq!(b.unallocated_total, 60_000);
    let r = t.auto();
    assert_eq!((r.entries_allocated, r.lines, r.skipped), (1, 2, 0));
    assert_eq!(t.sum_alloc(coding), 30_000);
    assert_eq!(t.sum_alloc(art), 30_000);
    assert_eq!(t.board().unallocated_total, 0);
    // 원장 징수액은 그대로 60,000 — 배분은 새 징수가 아니다.
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    assert_eq!(s.collection_total, 60_000);
    let allocs = t.db.read(|c| list_for_entry(c, e)).unwrap();
    assert!(allocs.iter().all(|a| a.method == "auto_split" && a.source_row_id.is_some()));
    // 두 번 돌려도 중복 배분하지 않는다.
    let r2 = t.auto();
    assert_eq!(r2.entries_allocated, 0);
    assert_eq!(t.sum_alloc(coding) + t.sum_alloc(art), 60_000);
}

#[test]
fn scenario_b_partial_manual_leaves_remainder_unallocated() {
    let t = setup();
    let s = t.student("가명B", 2, "나리");
    let a = t.program("프로그램A");
    let e = t.collect(Some(s), 100_000, t.fund_ben);
    t.roster(Some(s), "가명B", a, 100_000);
    t.db.write(|tx| manual(tx, e, &[(a, 60_000)], "확인")).unwrap();
    let b = t.board();
    assert_eq!(b.entries[0].allocated, 60_000);
    assert_eq!(b.entries[0].remaining, 40_000);
    assert_eq!(b.unallocated_total, 40_000);
    assert_eq!(b.entries[0].plan, "manual", "일부 배분된 거래는 자동으로 손대지 않는다");
    assert_eq!(t.auto().entries_allocated, 0);
}

#[test]
fn scenario_c_over_allocation_is_blocked_and_nothing_changes() {
    let t = setup();
    let s = t.student("가명C", 3, "다솜");
    let a = t.program("프로그램A");
    let bprog = t.program("프로그램B");
    let e = t.collect(Some(s), 100_000, t.fund_ben);
    assert_eq!(t.db.write(|tx| manual(tx, e, &[(a, 120_000)], "x")).unwrap_err().code, "INVALID");
    assert_eq!(t.db.write(|tx| manual(tx, e, &[(a, 70_000), (bprog, 50_000)], "x")).unwrap_err().code, "INVALID");
    t.db.write(|tx| manual(tx, e, &[(a, 70_000)], "x")).unwrap();
    assert_eq!(t.db.write(|tx| manual(tx, e, &[(bprog, 40_000)], "x")).unwrap_err().code, "INVALID");
    // 트리거도 막는다 (응용 코드를 거치지 않는 INSERT).
    let err = t.db.write(|tx| { tx.execute("INSERT INTO allocation (entry_id, program_id, amount, method) VALUES (?1, ?2, 40000, 'manual')", params![e, bprog])?; Ok(()) }).unwrap_err();
    assert!(err.detail.unwrap_or_default().contains("ALLOCATION_OVER"));
    // 부호가 다른 배분도 막는다.
    assert_eq!(t.db.write(|tx| manual(tx, e, &[(bprog, -10_000)], "x")).unwrap_err().code, "INVALID");
    assert_eq!(t.sum_alloc(a), 70_000);
    assert_eq!(t.sum_alloc(bprog), 0);
    // 배분은 고치거나 지울 수 없다.
    let id: i64 = t.db.read(|c| Ok(c.query_row("SELECT id FROM allocation", [], |r| r.get(0))?)).unwrap();
    assert!(t.db.write(|tx| { tx.execute("UPDATE allocation SET amount = 1 WHERE id = ?1", [id])?; Ok(()) }).is_err());
    assert!(t.db.write(|tx| { tx.execute("DELETE FROM allocation WHERE id = ?1", [id])?; Ok(()) }).is_err());
    // 취소하면 다시 배분할 수 있다.
    t.db.write(|tx| void(tx, id, "잘못 배분")).unwrap();
    assert_eq!(t.board().unallocated_total, 100_000);
}

#[test]
fn scenario_d_ambiguous_students_are_never_auto_allocated() {
    let t = setup();
    // 동명이인: 같은 이름, 다른 반 → 다른 학생 id. 징수 거래는 1학년 가람반 학생 것.
    let s1 = t.student("가명D", 1, "가람");
    let s2 = t.student("가명D", 1, "나리");
    let a = t.program("프로그램A");
    let bprog = t.program("프로그램B");
    let e1 = t.collect(Some(s1), 50_000, t.fund_ben);
    t.roster(Some(s2), "가명D", a, 50_000); // 참고 명단에는 다른 반의 동명이인만 있다
    // 학생 식별 불가 거래
    let e2 = t.collect(None, 30_000, t.fund_ben);
    // 여러 프로그램인데 금액 합이 맞지 않는 학생
    let s3 = t.student("가명E", 2, "가람");
    let e3 = t.collect(Some(s3), 60_000, t.fund_ben);
    t.roster(Some(s3), "가명E", a, 30_000);
    t.roster(Some(s3), "가명E", bprog, 25_000);
    // 여러 프로그램 + 재원 두 개(거래 둘) → 자동 금지
    let s4 = t.student("가명F", 3, "가람");
    let e4 = t.collect(Some(s4), 40_000, t.fund_voucher);
    let e5 = t.collect(Some(s4), 20_000, t.fund_ben);
    t.roster(Some(s4), "가명F", a, 30_000);
    t.roster(Some(s4), "가명F", bprog, 30_000);

    let b = t.board();
    let plan = |id: i64| b.entries.iter().find(|x| x.entry_id == id).unwrap().plan.clone();
    assert_eq!(plan(e1), "name_only", "이름만 같은 후보는 사용자 확인");
    assert_eq!(plan(e2), "no_student");
    assert_eq!(plan(e3), "manual");
    assert_eq!(plan(e4), "manual");
    assert_eq!(plan(e5), "manual");
    assert_eq!(b.no_student, 1);
    assert_eq!(b.need_user, 4);
    let cand = &b.entries.iter().find(|x| x.entry_id == e1).unwrap().candidates;
    assert_eq!(cand.len(), 1);
    assert!(!cand[0].same_student);
    let r = t.auto();
    assert_eq!(r.entries_allocated, 0);
    assert_eq!(r.skipped, 5);
    assert_eq!(t.board().unallocated_total, 200_000);
}

#[test]
fn single_program_student_is_allocated_even_if_reference_amount_differs() {
    let t = setup();
    let s = t.student("가명G", 4, "가람");
    let a = t.program("프로그램A");
    // 이용권 42,000 + 초과 5,000 두 거래, 참고 명단은 42,000 한 줄
    t.collect(Some(s), 42_000, t.fund_voucher);
    t.collect(Some(s), 5_000, t.fund_ben);
    t.roster(Some(s), "가명G", a, 42_000);
    let r = t.auto();
    assert_eq!(r.entries_allocated, 2);
    assert_eq!(t.sum_alloc(a), 47_000);
    let b = t.board();
    assert!(b.entries.iter().all(|e| e.plan == "done"));
    let note: String = t.db.read(|c| Ok(c.query_row("SELECT reason FROM allocation WHERE amount = 5000", [], |r| r.get(0))?)).unwrap();
    assert!(note.contains("다름"), "{note}");
}

#[test]
fn scenario_n_closed_month_blocks_allocation_changes() {
    let t = setup();
    let s = t.student("가명H", 5, "가람");
    let a = t.program("프로그램A");
    let e = t.collect(Some(s), 10_000, t.fund_ben);
    t.roster(Some(s), "가명H", a, 10_000);
    t.db.write(|tx| year::month_close(tx, t.sep, "확정")).unwrap();
    assert_eq!(t.db.write(|tx| auto_allocate(tx, t.sep)).unwrap_err().code, "INVALID");
    assert_eq!(t.db.write(|tx| manual(tx, e, &[(a, 10_000)], "x")).unwrap_err().code, "INVALID");
    t.db.write(|tx| year::month_reopen(tx, t.sep, "배분 필요")).unwrap();
    t.db.write(|tx| manual(tx, e, &[(a, 10_000)], "x")).unwrap();
    let id: i64 = t.db.read(|c| Ok(c.query_row("SELECT id FROM allocation", [], |r| r.get(0))?)).unwrap();
    t.db.write(|tx| year::month_close(tx, t.sep, "다시 확정")).unwrap();
    assert_eq!(t.db.write(|tx| void(tx, id, "x")).unwrap_err().code, "INVALID");
}

#[test]
fn voided_entries_drop_their_allocations_from_the_board() {
    let t = setup();
    let s = t.student("가명I", 6, "가람");
    let a = t.program("프로그램A");
    let e = t.collect(Some(s), 10_000, t.fund_ben);
    t.roster(Some(s), "가명I", a, 10_000);
    t.auto();
    assert_eq!(t.sum_alloc(a), 10_000);
    t.db.write(|tx| ledger::void(tx, e, "input_error", "취소")).unwrap();
    assert_eq!(t.sum_alloc(a), 0, "취소된 거래의 배분은 유효하지 않다");
    assert!(t.board().entries.is_empty());
}
