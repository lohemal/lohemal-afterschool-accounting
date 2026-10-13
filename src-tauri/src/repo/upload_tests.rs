//! 업로드 · 되돌리기 시나리오 (지시서 12장). 가명 파일만 쓴다.

use std::path::PathBuf;

use super::*;
use crate::db::Db;
use crate::excel::fixtures_dir;
use crate::model::{PreviewRequest, ProgramMapInput, RowOverride};
use crate::repo::{ledger, program, year};

struct T {
    db: Db,
    dir: tempfile::TempDir,
    year_id: i64,
    /// 9월 회계월 id
    sep: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months = db.read(|c| year::months(c, y.id)).unwrap();
    let sep = months.iter().find(|m| m.ym == "2026-09").unwrap().id;
    T { db, dir: tempfile::tempdir().unwrap(), year_id: y.id, sep }
}

fn fx(name: &str) -> String {
    fixtures_dir().join(name).display().to_string()
}

impl T {
    fn req(&self, file: &str, format: &str, kind: &str) -> PreviewRequest {
        PreviewRequest {
            path: fx(file),
            school_year_id: self.year_id,
            fiscal_month_id: self.sep,
            format_code: format.into(),
            data_kind: kind.into(),
            overrides: vec![],
            replace_file_id: None,
            note: None,
        }
    }
    fn commit(&self, req: &PreviewRequest) -> AppResult<CommitResult> {
        let dir = self.dir.path().to_path_buf();
        self.db.write(|tx| commit(tx, &dir, req))
    }
    fn preview(&self, req: &PreviewRequest) -> Preview {
        self.db.read(|c| preview(c, req)).unwrap()
    }
    fn month_total(&self) -> (i64, i64, i64) {
        let s = self.db.read(|c| ledger::month_summary(c, self.sep)).unwrap();
        (s.collection_total, s.expenditure_total, s.rows.iter().map(|r| r.entry_count).sum())
    }
    fn all_entries(&self) -> (i64, i64) {
        self.db
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*), coalesce(sum(CASE WHEN state = 'active' THEN 1 ELSE 0 END), 0) FROM ledger_entry",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }
    fn map_new(&self, aliases: &[&str]) {
        for a in aliases {
            let input = ProgramMapInput {
                school_year_id: self.year_id,
                alias: a.to_string(),
                target: "new_program".into(),
                program_id: None,
                group_id: None,
                new_name: None,
                member_ids: None,
                parent_id: None,
            };
            self.db.write(|tx| program::map_save(tx, &input)).unwrap();
        }
    }
    fn file(&self, id: i64) -> UploadFile {
        self.db.read(|c| get(c, id)).unwrap()
    }
}

const INITIAL: &str = "collect_initial.xlsx";
const INITIAL_V2: &str = "collect_initial_v2.xlsx";

// ── 인식 · 미리보기 ───────────────────────────────────────────────────────

#[test]
fn inspect_detects_format_and_duplicates() {
    let t = setup();
    let ins = t.db.read(|c| inspect(c, t.year_id, &PathBuf::from(fx("instructor_pay.xlsx")))).unwrap();
    assert_eq!(ins.detected[0].code, "instructor_pay");
    assert_eq!(ins.suggested_month, Some(9));
    assert!(ins.duplicate_of.is_none());
    assert_eq!(ins.sheets[0].name, "강사료내역");
    assert_eq!(ins.sha256.len(), 64);
}

#[test]
fn preview_matches_commit_exactly() {
    let t = setup();
    let req = t.req(INITIAL, "collect_initial", "initial_collect");
    let p = t.preview(&req);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    assert_eq!(p.summary.collection_total, 484_400);
    assert_eq!(p.summary.included_count, p.rows.iter().filter(|r| r.included).count() as i64);
    assert!(p.summary.by_fund.iter().any(|l| l.name == "방과후이용권" && l.total == 154_300), "{:?}", p.summary.by_fund);
    assert!(p.summary.by_fund.iter().any(|l| l.name.starts_with("초3 초과") && l.total == 5_000));
    assert!(p.summary.by_program.iter().any(|l| l.name == "미배분"));
    let r = t.commit(&req).unwrap();
    assert_eq!(r.collection_total, 484_400);
    assert_eq!(r.entry_count, p.summary.included_count);
    assert_eq!(t.month_total(), (484_400, 0, r.entry_count));
    let f = t.file(r.file.id);
    assert_eq!((f.status.as_str(), f.version_no, f.lineage_id), ("applied", 1, Some(f.id)));
    assert_eq!(f.summary.collection_total, 484_400);
    assert!(t.dir.path().join("uploads").join(&f.stored_name).exists(), "원본이 보관된다");
    assert!(f.row_count > 0);
    // 학생이 식별되어 저장되었다.
    let students: i64 = t.db.read(|c| Ok(c.query_row("SELECT count(*) FROM student", [], |r| r.get(0))?)).unwrap();
    assert_eq!(students, 6);
}

#[test]
fn same_content_is_blocked_even_under_another_name() {
    let t = setup();
    let req = t.req(INITIAL, "collect_initial", "initial_collect");
    t.commit(&req).unwrap();
    // 같은 파일 다시
    let p = t.preview(&req);
    assert!(!p.can_commit);
    assert!(p.block_reasons.iter().any(|b| b.contains("같은 내용")), "{:?}", p.block_reasons);
    assert!(t.commit(&req).is_err());
    // 이름만 다른 복사본
    let copy = t.dir.path().join("복사본.xlsx");
    std::fs::copy(fx(INITIAL), &copy).unwrap();
    let mut req2 = req.clone();
    req2.path = copy.display().to_string();
    assert!(!t.preview(&req2).can_commit);
    let ins = t.db.read(|c| inspect(c, t.year_id, &copy)).unwrap();
    assert!(ins.duplicate_of.is_some());
    assert_eq!(t.month_total().0, 484_400, "두 배로 잡히지 않는다");
}

#[test]
fn second_initial_collection_in_same_month_needs_replace() {
    let t = setup();
    let v1 = t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    let mut req2 = t.req(INITIAL_V2, "collect_initial", "initial_collect");
    let p = t.preview(&req2);
    assert!(!p.can_commit);
    assert!(p.block_reasons.iter().any(|b| b.contains("교체")), "{:?}", p.block_reasons);
    assert_eq!(p.existing_applied.as_ref().map(|f| f.id), Some(v1.file.id));
    req2.replace_file_id = Some(v1.file.id);
    assert!(t.preview(&req2).can_commit);
    // 참고 스냅샷으로는 그냥 올릴 수 있고 거래를 만들지 않는다.
    let mut snap = t.req(INITIAL_V2, "collect_initial", "snapshot");
    snap.replace_file_id = None;
    let p = t.preview(&snap);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    assert_eq!(p.summary.included_count, 0);
    let r = t.commit(&snap).unwrap();
    assert_eq!(r.entry_count, 0);
    assert_eq!(t.month_total().0, 484_400);
}

// ── 시나리오 A · D: 교체와 복원 ─────────────────────────────────────────────

#[test]
fn scenario_a_replace_then_restore_previous_version() {
    let t = setup();
    let v1 = t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    let mut req2 = t.req(INITIAL_V2, "collect_initial", "initial_collect");
    req2.replace_file_id = Some(v1.file.id);
    let v2 = t.commit(&req2).unwrap();
    assert_eq!(v2.file.version_no, 2);
    assert_eq!(v2.file.lineage_id, v1.file.lineage_id);
    assert_eq!(t.month_total().0, 479_400);
    assert_eq!(t.file(v1.file.id).status, "replaced");
    assert_eq!(t.file(v1.file.id).replaced_by_id, Some(v2.file.id));
    assert_eq!(t.file(v1.file.id).entry_count, 0, "v1 거래는 전부 취소");

    // 복원 전 미리보기
    let imp = t.db.read(|c| impact(c, v1.file.id, "restore")).unwrap();
    assert!(imp.blocked.is_empty(), "{:?}", imp.blocked);
    assert_eq!(imp.current.as_ref().map(|f| f.id), Some(v2.file.id));
    assert_eq!(imp.before.collection_total, 479_400);
    assert_eq!(imp.after.collection_total, 484_400);

    let restored = t.db.write(|tx| restore(tx, v1.file.id, "v2 에 오류 발견")).unwrap();
    assert_eq!(restored.status, "applied");
    assert_eq!(t.file(v2.file.id).status, "replaced");
    assert_eq!(t.month_total().0, 484_400, "v1 기준 금액으로 정확히 복원");
    // 원본·이력 보존: v1 의 옛 거래(취소)와 새 거래(살아 있음)가 모두 남는다.
    let (all, active) = t.all_entries();
    assert_eq!(all, v1.entry_count * 2 + v2.entry_count);
    assert_eq!(active, v1.entry_count);
    let d = t.db.read(|c| detail(c, v1.file.id)).unwrap();
    assert_eq!(d.applies.iter().map(|a| a.action.as_str()).collect::<Vec<_>>(), ["apply", "replace", "restore"]);
    assert_eq!(d.versions.len(), 2);
    assert!(t.dir.path().join("uploads").join(&t.file(v2.file.id).stored_name).exists(), "v2 원본도 보존");
}

#[test]
fn scenario_d_flipping_versions_never_double_counts() {
    let t = setup();
    let v1 = t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    let mut req2 = t.req(INITIAL_V2, "collect_initial", "initial_collect");
    req2.replace_file_id = Some(v1.file.id);
    let v2 = t.commit(&req2).unwrap();
    let mut expect_all = v1.entry_count + v2.entry_count;
    for (target, total, n) in [(v1.file.id, 484_400, v1.entry_count), (v2.file.id, 479_400, v2.entry_count), (v1.file.id, 484_400, v1.entry_count), (v2.file.id, 479_400, v2.entry_count)] {
        t.db.write(|tx| restore(tx, target, "번갈아 복원")).unwrap();
        expect_all += n;
        let (coll, _, active_n) = t.month_total();
        assert_eq!(coll, total);
        assert_eq!(active_n, n);
        let (all, active) = t.all_entries();
        assert_eq!(all, expect_all, "모든 이력이 남는다");
        assert_eq!(active, n);
        let applied: i64 = t
            .db
            .read(|c| Ok(c.query_row("SELECT count(*) FROM upload_file WHERE status = 'applied'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(applied, 1, "적용 중인 판은 항상 하나");
    }
    // 적용 중인 판은 복원할 수 없다.
    assert_eq!(t.db.write(|tx| restore(tx, v2.file.id, "x")).unwrap_err().code, "INVALID");
}

// ── 시나리오 B · C: 다른 파일은 그대로 ───────────────────────────────────────

#[test]
fn scenario_b_cancelling_initial_keeps_refund_file_and_flags_dependents() {
    let t = setup();
    let ini = t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    t.map_new(&["로봇과학A", "바이올린B", "컴퓨터C"]);
    let rf = t.commit(&t.req("refund_extra_s1.xlsx", "refund_extra_s1", "extra_refund")).unwrap();
    assert_eq!(rf.collection_total, -64_050);
    assert_eq!(t.month_total().0, 484_400 - 64_050);

    let imp = t.db.read(|c| impact(c, ini.file.id, "cancel")).unwrap();
    assert!(imp.blocked.is_empty());
    assert_eq!(imp.before.collection_total, 484_400);
    assert_eq!(imp.after.entry_count, 0);
    assert_eq!(imp.dependents.len(), 1, "같은 학생의 환불 거래가 있는 파일");
    assert_eq!(imp.dependents[0].file_id, rf.file.id);
    assert!(imp.warnings.iter().any(|w| w.contains("연결")));

    t.db.write(|tx| cancel(tx, ini.file.id, "잘못된 징수명단")).unwrap();
    assert_eq!(t.month_total(), (-64_050, 0, rf.entry_count), "환불·추가징수 거래는 유지");
    assert_eq!(t.file(rf.file.id).status, "applied");
    assert_eq!(t.file(rf.file.id).entry_count, rf.entry_count);
    assert_eq!(t.file(ini.file.id).status, "cancelled");
    // 취소된 파일은 다시 적용할 수 있다.
    t.db.write(|tx| restore(tx, ini.file.id, "취소 자체가 실수")).unwrap();
    assert_eq!(t.month_total().0, 484_400 - 64_050);
}

#[test]
fn scenario_c_cancelling_refund_file_restores_only_refund_amounts() {
    let t = setup();
    t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    t.map_new(&["로봇과학A", "바이올린B", "컴퓨터C"]);
    let rf = t.commit(&t.req("refund_extra_s1.xlsx", "refund_extra_s1", "extra_refund")).unwrap();
    assert_eq!(t.month_total().0, 420_350);
    t.db.write(|tx| cancel(tx, rf.file.id, "환불 금액 오류")).unwrap();
    assert_eq!(t.month_total().0, 484_400, "환불 금액만 원상 복구");
    // 두 번 취소는 안 된다.
    assert_eq!(t.db.write(|tx| cancel(tx, rf.file.id, "다시")).unwrap_err().code, "INVALID");
    // 사유 없이는 안 된다.
    let ini_id: i64 = t.db.read(|c| Ok(c.query_row("SELECT id FROM upload_file WHERE status = 'applied'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(t.db.write(|tx| cancel(tx, ini_id, "  ")).unwrap_err().code, "INVALID");
}

// ── 시나리오 E: 도중 오류 → 전부 되돌림 ─────────────────────────────────────

#[test]
fn scenario_e_failure_inside_transaction_leaves_db_untouched() {
    let t = setup();
    let req = t.req(INITIAL, "collect_initial", "initial_collect");
    let dir = t.dir.path().to_path_buf();
    let err = t
        .db
        .write(|tx| {
            let r = commit(tx, &dir, &req)?;
            assert!(r.entry_count > 0);
            Err::<(), _>(AppError::new("TEST", "반영 직후 실패"))
        })
        .unwrap_err();
    assert_eq!(err.code, "TEST");
    let (files, rows, srcs): (i64, i64, i64) = t
        .db
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT count(*) FROM upload_file), (SELECT count(*) FROM upload_row), (SELECT count(*) FROM source_row)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!((files, rows, srcs), (0, 0, 0));
    assert_eq!(t.all_entries().0, 0);
    // 보관 파일은 고아로 남지만 시작 때 정리된다.
    let orphans = std::fs::read_dir(dir.join("uploads")).unwrap().count();
    assert_eq!(orphans, 1);
    let removed = t.db.read(|c| cleanup_orphans(c, &dir)).unwrap();
    assert_eq!(removed, 1);
    // 그 뒤 정상 반영은 된다.
    t.commit(&req).unwrap();
    assert_eq!(t.db.read(|c| cleanup_orphans(c, &dir)).unwrap(), 0, "쓰이는 파일은 지우지 않는다");

    // 복원 도중 실패도 마찬가지.
    let v1_id: i64 = t.db.read(|c| Ok(c.query_row("SELECT id FROM upload_file", [], |r| r.get(0))?)).unwrap();
    let mut req2 = t.req(INITIAL_V2, "collect_initial", "initial_collect");
    req2.replace_file_id = Some(v1_id);
    t.commit(&req2).unwrap();
    let before = t.all_entries();
    let err = t
        .db
        .write(|tx| {
            restore(tx, v1_id, "복원")?;
            Err::<(), _>(AppError::new("TEST", "복원 직후 실패"))
        })
        .unwrap_err();
    assert_eq!(err.code, "TEST");
    assert_eq!(t.all_entries(), before);
    assert_eq!(t.file(v1_id).status, "replaced");
    assert_eq!(t.month_total().0, 479_400);
}

// ── 시나리오 F: 확정된 달 ──────────────────────────────────────────────────

#[test]
fn scenario_f_closed_month_blocks_changes_until_reopened() {
    let t = setup();
    let v1 = t.commit(&t.req(INITIAL, "collect_initial", "initial_collect")).unwrap();
    t.db.write(|tx| year::month_close(tx, t.sep, "9월 정산 확정")).unwrap();
    let mut req2 = t.req(INITIAL_V2, "collect_initial", "initial_collect");
    req2.replace_file_id = Some(v1.file.id);
    let p = t.preview(&req2);
    assert!(!p.can_commit);
    assert!(p.block_reasons.iter().any(|b| b.contains("확정")), "{:?}", p.block_reasons);
    assert!(t.commit(&req2).is_err());
    assert_eq!(t.db.write(|tx| cancel(tx, v1.file.id, "x")).unwrap_err().code, "INVALID");
    let imp = t.db.read(|c| impact(c, v1.file.id, "cancel")).unwrap();
    assert!(imp.blocked.iter().any(|b| b.contains("확정")));
    assert_eq!(t.month_total().0, 484_400);
    // 재개 사유 없이는 재개되지 않는다.
    assert_eq!(t.db.write(|tx| year::month_reopen(tx, t.sep, " ")).unwrap_err().code, "INVALID");
    t.db.write(|tx| year::month_reopen(tx, t.sep, "환불 반영 필요")).unwrap();
    assert!(t.preview(&req2).can_commit);
    t.commit(&req2).unwrap();
    assert_eq!(t.month_total().0, 479_400);
    let months = t.db.read(|c| year::months(c, t.year_id)).unwrap();
    let m = months.iter().find(|m| m.id == t.sep).unwrap();
    assert_eq!(m.status, "open");
    assert!(m.last_closed_at.is_some());
}

// ── 프로그램 · 재원 매핑 ───────────────────────────────────────────────────

#[test]
fn roster_upload_requires_program_mapping_and_reuses_rules() {
    let t = setup();
    let req = t.req("roster_manager.xlsx", "roster_manager", "initial_collect");
    let p = t.preview(&req);
    assert!(!p.can_commit);
    let texts: Vec<&str> = p.unresolved_programs.iter().map(|u| u.text.as_str()).collect();
    assert_eq!(texts, ["로봇과학A", "코딩A", "코딩B", "항공드론A"], "0원 행뿐인 금요방송댄스A 는 거래가 없어 나오지 않는다");
    assert!(p.unresolved_programs.iter().all(|u| u.suggestion_program_id.is_none()));
    // '코딩' 을 만들면 코딩A·코딩B 는 제안만 받는다 (자동 합치지 않음).
    t.map_new(&["코딩"]);
    let p = t.preview(&req);
    let a = p.unresolved_programs.iter().find(|u| u.text == "코딩A").unwrap();
    assert_eq!(a.suggestion_name, "코딩");
    assert!(!p.can_commit);
    // 사용자가 확정: 코딩A·코딩B → 코딩, 나머지는 새 프로그램
    let coding = t.db.read(|c| program::resolve(c, t.year_id, "코딩")).unwrap().unwrap();
    for alias in ["코딩A", "코딩B"] {
        let input = ProgramMapInput { school_year_id: t.year_id, alias: alias.into(), target: "program".into(), program_id: Some(coding), group_id: None, new_name: None, member_ids: None, parent_id: None };
        t.db.write(|tx| program::map_save(tx, &input)).unwrap();
    }
    t.map_new(&["로봇과학A", "항공드론A"]);
    let p = t.preview(&req);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    assert!(p.unresolved_programs.is_empty());
    assert_eq!(p.unresolved_funds.iter().map(|u| u.text.as_str()).collect::<Vec<_>>(), ["방과후 이용권 + 자유수강권"]);
    assert!(p.warnings.iter().any(|w| w.contains("미확정")));
    assert_eq!(p.summary.collection_total, 480_300);
    assert!(p.summary.by_program.iter().any(|l| l.name == "코딩" && l.total == 151_500), "{:?}", p.summary.by_program);
    assert!(p.summary.by_fund.iter().any(|l| l.name == "미확정"));
    let r = t.commit(&req).unwrap();
    assert_eq!(r.collection_total, 480_300);
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    assert!(s.rows.iter().any(|r| r.fund_type_name == "미확정"));
    assert!(s.rows.iter().any(|r| r.fund_type_name == "방과후이용권"));
    // 규칙은 다음 업로드에 그대로 재사용된다.
    let again = t.req("roster_manager.xlsx", "roster_manager", "snapshot");
    let p = t.preview(&again);
    assert!(p.unresolved_programs.is_empty());
}

#[test]
fn vendor_blank_program_rows_need_a_row_override() {
    let t = setup();
    t.map_new(&["코딩", "로봇과학", "체스", "주산암산"]);
    let mut req = t.req("vendor_pay.xlsx", "vendor_pay", "vendor_pay");
    let p = t.preview(&req);
    assert!(!p.can_commit);
    assert!(p.block_reasons.iter().any(|b| b.contains("프로그램이 비어")), "{:?}", p.block_reasons);
    let blank: Vec<&PreviewRow> = p.rows.iter().filter(|r| r.row_no == 8).collect();
    assert_eq!(blank.len(), 2);
    assert_eq!(blank[0].program_suggestion, "체스");
    assert_eq!(blank[0].status, "check");
    // 병합 셀 행은 자동으로 이어받아 정상이다.
    assert!(p.rows.iter().filter(|r| r.row_no == 6).all(|r| r.program_name == "로봇과학" && r.status == "ok"));
    let chess = t.db.read(|c| program::resolve(c, t.year_id, "체스")).unwrap().unwrap();
    req.overrides = vec![RowOverride { sheet: "2026.9.".into(), row_no: 8, program_id: Some(chess), program_group_id: None }];
    let p = t.preview(&req);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    assert_eq!(p.summary.expenditure_total, 2_036_000);
    assert!(p.summary.by_program.iter().any(|l| l.name == "체스" && l.total == 330_000));
    let r = t.commit(&req).unwrap();
    assert_eq!(r.expenditure_total, 2_036_000);
    assert_eq!(t.month_total(), (0, 2_036_000, 16));
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    assert!(s.rows.iter().all(|r| r.amount_item_name == "교재·재료비(합산)"));
    assert!(s.rows.iter().any(|r| r.fund_type_name == "방과후이용권"), "3학년 지원금 열은 방과후이용권으로");
}

#[test]
fn instructor_group_row_lands_on_a_program_group() {
    let t = setup();
    t.map_new(&["코딩", "로봇과학", "항공드론", "우쿨렐레", "통기타"]);
    let uk = t.db.read(|c| program::resolve(c, t.year_id, "우쿨렐레")).unwrap().unwrap();
    let gt = t.db.read(|c| program::resolve(c, t.year_id, "통기타")).unwrap().unwrap();
    let g = ProgramMapInput { school_year_id: t.year_id, alias: "우쿨렐레&통기타".into(), target: "new_group".into(), program_id: None, group_id: None, new_name: None, member_ids: Some(vec![uk, gt]), parent_id: None };
    t.db.write(|tx| program::map_save(tx, &g)).unwrap();
    let req = t.req("instructor_pay.xlsx", "instructor_pay", "instructor_pay");
    let p = t.preview(&req);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    assert!(p.rows.iter().any(|r| r.program_name == "우쿨렐레&통기타 (그룹)"));
    let r = t.commit(&req).unwrap();
    assert_eq!(r.expenditure_total, 4_290_100);
    let grouped: i64 = t
        .db
        .read(|c| Ok(c.query_row("SELECT coalesce(sum(amount), 0) FROM ledger_entry WHERE program_group_id IS NOT NULL AND state = 'active'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(grouped, 864_000, "그룹 금액은 개별 프로그램에 배분하지 않는다");
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    assert!(s.rows.iter().any(|r| r.program_name == "우쿨렐레&통기타 (그룹)"));
    let by_fund: Vec<(String, i64)> = {
        let imp = t.db.read(|c| impact(c, r.file.id, "cancel")).unwrap();
        imp.before.by_fund.iter().map(|l| (l.name.clone(), l.total)).collect()
    };
    assert!(by_fund.contains(&("방과후이용권".into(), 1_141_000)), "{by_fund:?}");
    assert!(by_fund.contains(&("맞춤형지원".into(), 1_760_000)));
}

#[test]
fn formula_file_cannot_be_committed() {
    let t = setup();
    t.map_new(&["코딩", "로봇과학", "항공드론", "우쿨렐레&통기타"]);
    let req = t.req("instructor_pay_formula.xlsx", "instructor_pay", "instructor_pay");
    let p = t.preview(&req);
    assert!(!p.can_commit);
    assert!(p.block_reasons.iter().any(|b| b.contains("오류 행")), "{:?}", p.block_reasons);
    assert_eq!(p.summary.error_count, 14);
    assert!(t.commit(&req).is_err());
}

#[test]
fn settle_s1_as_ledger_keeps_three_kinds_and_roster_link_makes_no_entries() {
    let t = setup();
    t.map_new(&["코딩A", "로봇과학A", "컴퓨터C"]);
    let link = t.commit(&t.req("settle_s1.xlsm", "settle_s1", "roster_link")).unwrap();
    assert_eq!(link.entry_count, 0);
    assert_eq!(t.file(link.file.id).row_count, 18, "정규화 행은 참고용으로 남는다");
    let r = t.commit(&t.req("roster_s1.xlsx", "roster_s1", "roster_link")).unwrap();
    assert_eq!(r.entry_count, 0);
    // 같은 xlsm 을 원장 자료로 올리면 징수·환불·추가징수가 각각 남는다.
    let copy = t.dir.path().join("settle_copy.xlsm");
    std::fs::copy(fx("settle_s1.xlsm"), &copy).unwrap();
    let mut req = t.req("settle_s1.xlsm", "settle_s1", "initial_collect");
    req.path = copy.display().to_string();
    let p = t.preview(&req);
    assert!(!p.can_commit, "같은 내용이 이미 연결 자료로 살아 있다: {:?}", p.block_reasons);
    t.db.write(|tx| cancel(tx, link.file.id, "원장 자료로 다시 올림")).unwrap();
    let p = t.preview(&req);
    assert!(p.can_commit, "{:?}", p.block_reasons);
    t.commit(&req).unwrap();
    assert_eq!(t.month_total().0, 216_500);
    let s = t.db.read(|c| ledger::month_summary(c, t.sep)).unwrap();
    let kinds: std::collections::BTreeSet<String> = {
        let mut st_ok = Vec::new();
        t.db.read(|c| {
            let mut st = c.prepare("SELECT DISTINCT entry_kind FROM ledger_entry WHERE state = 'active'")?;
            st_ok = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
            Ok(())
        })
        .unwrap();
        st_ok.into_iter().collect()
    };
    assert_eq!(kinds.into_iter().collect::<Vec<_>>(), ["extra_collect", "initial_collect", "refund"]);
    assert!(s.rows.iter().all(|r| r.fund_type_name == "미확정"));
}
