//! P5 시험 — 백업(1~7) · 복원(8~15) · 무결성(16~23) · Excel(39~43) · 장애 시나리오 1~5. 가명·가상 금액, 임시 폴더만 쓴다.

use std::path::{Path, PathBuf};

use rusqlite::params;

use crate::db::Db;
use crate::excel::parse::StudentKey;
use crate::model::*;
use crate::ops::{backup, export, history, restore};
use crate::repo::{adjustment, catalog, integrity, ledger, program, student, upload, voucher, year};

struct T {
    _dir: tempfile::TempDir,
    data_dir: PathBuf,
    db: Db,
    year_id: i64,
    months: Vec<i64>,
    ben: i64,
    g3x: i64,
    voucher: i64,
    free: i64,
    instr: i64,
    textbook: i64,
    material: i64,
    tm: i64,
    coding: i64,
    stu: i64,
}

fn setup() -> T {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().to_path_buf();
    let db = Db::open(&data_dir.join("asaccounting.db")).unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months: Vec<i64> = db.read(|c| year::months(c, y.id)).unwrap().iter().map(|m| m.id).collect();
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    let coding = db.write(|tx| program::create(tx, y.id, "코딩")).unwrap();
    let stu = db.write(|tx| student::upsert(tx, y.id, &StudentKey { grade: Some(3), class_name: "가람".into(), student_no: "3".into(), name: "가명A".into() })).unwrap().unwrap();
    T { _dir: dir, data_dir, db, year_id: y.id, months, ben: fund("beneficiary"), g3x: fund("g3_excess"), voucher: fund("after_voucher"), free: fund("free_voucher"), instr: item("instructor"), textbook: item("textbook"), material: item("material"), tm: item("textbook_material"), coding, stu }
}

impl T {
    fn m(&self, seq: usize) -> i64 {
        self.months[seq - 1]
    }
    fn collect(&self, seq: usize, fund: i64, item: i64, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: self.m(seq), program_id: Some(self.coding), program_group_id: None, student_id: Some(self.stu),
                    fund_type_id: Some(fund), amount_item_id: item, flow: "collection".into(), entry_kind: "initial_collect".into(), amount,
                    stage: None, processed_on: Some("2026-04-10".into()), counterparty: None, upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn policy(&self, annual: i64) {
        self.db.write(|tx| voucher::policy_save(tx, &VoucherPolicyInput { school_year_id: self.year_id, basis: "annual".into(), annual_limit: Some(annual), ..Default::default() })).unwrap();
    }
    fn confirm(&self) {
        self.db.write(|tx| voucher::target_save(tx, &VoucherTargetInput { school_year_id: self.year_id, student_id: self.stu, status: "confirmed".into(), confirmed_on: Some("2026-05-10".into()), apply_from: Some("2026-03-01".into()), apply_to: None, transfer_in: false, transfer_in_on: None, note: String::new(), reason: "시험".into() })).unwrap();
    }
    /// 가명 엑셀(강사료 지급)을 업로드해 보관 원본을 만든다. 프로그램 표기는 P1 시험과 같은 방식으로 매핑한다.
    fn upload_fixture(&self) -> UploadFile {
        let path = crate::excel::fixtures_dir().join("instructor_pay.xlsx");
        let sep = self.m(7);
        self.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: self.year_id, alias: "코딩".into(), target: "program".into(), program_id: Some(self.coding), group_id: None, new_name: None, member_ids: None }).map(|_| ())).unwrap();
        for n in ["로봇과학", "항공드론", "우쿨렐레", "통기타"] {
            self.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: self.year_id, alias: n.into(), target: "new_program".into(), program_id: None, group_id: None, new_name: Some(n.into()), member_ids: None }).map(|_| ())).unwrap();
        }
        let uk = self.db.read(|c| program::resolve(c, self.year_id, "우쿨렐레")).unwrap().unwrap();
        let gt = self.db.read(|c| program::resolve(c, self.year_id, "통기타")).unwrap().unwrap();
        self.db.write(|tx| program::map_save(tx, &ProgramMapInput { school_year_id: self.year_id, alias: "우쿨렐레&통기타".into(), target: "new_group".into(), program_id: None, group_id: None, new_name: None, member_ids: Some(vec![uk, gt]) }).map(|_| ())).unwrap();
        let req = PreviewRequest { path: path.display().to_string(), school_year_id: self.year_id, fiscal_month_id: sep, format_code: "instructor_pay".into(), data_kind: "instructor_pay".into(), overrides: vec![], replace_file_id: None, note: None };
        let pv = self.db.read(|c| upload::preview(c, &req)).unwrap();
        assert!(pv.can_commit, "{:?}", pv.block_reasons);
        let dd = self.data_dir.clone();
        self.db.write(|tx| upload::commit(tx, &dd, &req)).unwrap().file
    }
    fn apply_retro(&self) -> VoucherBatch {
        let p = self.db.read(|c| voucher::retro_preview(c, self.year_id, &[self.stu], &[])).unwrap();
        let rows: Vec<VoucherApplyRow> = p.proposals.iter().filter(|x| x.state == "normal").map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: false }).collect();
        self.db.write(|tx| voucher::apply(tx, &VoucherApplyInput { school_year_id: self.year_id, kind: "retro".into(), reason: "시험".into(), decided_on: None, processed_on: Some("2026-05-15".into()), status: "done".into(), rows, decisions: vec![] })).unwrap()
    }
    fn counts(&self) -> DbCounts {
        self.db.read(backup::counts).unwrap()
    }
    fn backup(&self, kind: &str) -> BackupInfo {
        let dd = self.data_dir.clone();
        self.db.read(|c| backup::create(c, &dd, kind, "시험")).unwrap()
    }
    fn hist(&self, kind: &str) -> Vec<OpsRecord> {
        history::list(&self.data_dir, 100).unwrap().into_iter().filter(|r| r.kind == kind).collect()
    }
}

/// 시나리오 자료: 원장·재원경정·환불·자유수강권 대상자.
fn seed_scenario(t: &T) -> (VoucherBatch, Adjustment) {
    t.policy(600_000);
    t.collect(1, t.voucher, t.instr, 60_000);
    t.collect(1, t.g3x, t.instr, 40_000);
    t.collect(2, t.ben, t.instr, 30_000);
    t.confirm();
    let b = t.apply_retro();
    let r = t.db.read(|c| voucher::refund_list(c, t.year_id)).unwrap();
    let a = t.db.write(|tx| voucher::refund_register(tx, &VoucherRefundInput { transfer_id: r[0].transfer_id, amount: r[0].amount, status: "done".into(), processed_on: Some("2026-05-20".into()), collected_confirmed: true, evidence: "수납 대장".into(), reason: "소급 환불".into() })).unwrap();
    (b, a)
}

fn corrupt_zip(path: &Path) {
    // DB 항목의 압축 바이트 일부를 뒤집는다 → 체크섬 불일치 (zip 구조는 그대로)
    let mut bytes = std::fs::read(path).unwrap();
    let mid = bytes.len() / 2;
    for b in bytes.iter_mut().skip(mid).take(64) {
        *b ^= 0xFF;
    }
    std::fs::write(path, bytes).unwrap();
}

// ── A. 백업 (1·2·3·4·5·6·7) · 시나리오 4 WAL ─────────────────────────────

#[test]
fn a_backup_package_is_consistent_verified_and_includes_originals() {
    let t = setup();
    seed_scenario(&t);
    let f = t.upload_fixture();
    assert!(!f.stored_name.is_empty());
    // WAL 모드에서 거래가 있는 상태 (2 · 시나리오 4)
    let mode: String = t.db.read(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?)).unwrap();
    assert_eq!(mode, "wal");
    let before = t.counts();
    let info = t.backup("manual");
    // 1·3·4 — 생성·원본 포함·체크섬 검증
    assert!(info.verified, "{:?}", info.verify_errors);
    assert!(info.complete);
    assert_eq!(info.file_count, 2, "DB + 원본 1개");
    assert!(Path::new(&info.path).exists());
    assert!(info.file_name.starts_with("방과후회계백업_") && info.file_name.ends_with(".zip"));
    let m = backup::read_manifest(Path::new(&info.path)).unwrap();
    assert_eq!(m.counts, before, "메타데이터 건수 = 백업 시점 건수");
    assert!(m.files.iter().any(|x| x.path == format!("uploads/{}", f.stored_name)));
    assert_eq!(m.schema_version, crate::db::migrate::latest_version());
    let v = backup::verify(Path::new(&info.path)).unwrap();
    assert!(v.ok && v.errors.is_empty());
    // 시나리오 4: 백업 뒤 새 거래가 들어와도 백업은 백업 시점 그대로 (스냅샷)
    t.collect(3, t.ben, t.instr, 10_000);
    let v2 = backup::verify(Path::new(&info.path)).unwrap();
    assert!(v2.ok);
    assert_eq!(backup::read_manifest(Path::new(&info.path)).unwrap().counts.ledger_entries, before.ledger_entries);
    // 목록·이력
    let dd = t.data_dir.clone();
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    assert_eq!(l.backups.len(), 1);
    assert!(l.failed.is_empty());
    assert_eq!(l.last_backup_at.as_deref(), Some(info.created_at.as_str()));
    assert_eq!(t.hist("backup").len(), 1);
    assert_eq!(t.hist("backup")[0].status, "ok");
}

#[test]
fn a_corrupted_backup_is_detected_and_listed_as_failed() {
    let t = setup();
    seed_scenario(&t);
    let info = t.backup("manual");
    corrupt_zip(Path::new(&info.path));
    let v = backup::verify(Path::new(&info.path)).unwrap();
    assert!(!v.ok, "5. 손상 감지");
    assert!(v.errors.iter().any(|e| e.contains("체크섬") || e.contains("구조") || e.contains("읽는 중")), "{:?}", v.errors);
    // 곁 파일은 생성 당시 '검증됨' 이지만 다시 검증하면 실패 → 복원 미리보기가 막는다
    let pv = restore::inspect(&t.db, Path::new(&info.path)).unwrap();
    assert!(!pv.can_restore);
    assert!(pv.blocked.iter().any(|b| b.contains("검증 실패")));
    // 7. 마이그레이션 직전 백업: v7 로 만든 파일 DB 를 열면 패키지가 생긴다
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("asaccounting.db");
    {
        let mut c = rusqlite::Connection::open(&p).unwrap();
        crate::db::migrate::run_up_to(&mut c, 7).unwrap();
        year::create(&c, 2026).unwrap();
    }
    let db = Db::open(&p).unwrap();
    let l = db.read(|c| backup::list(c, dir.path())).unwrap();
    assert_eq!(l.backups.len(), 1);
    assert_eq!(l.backups[0].kind, "migration");
    assert!(l.backups[0].protected && l.backups[0].verified);
    assert_eq!(l.backups[0].schema_version, 7);
}

#[test]
fn a_retention_keeps_recent_autos_and_never_deletes_protected() {
    let t = setup();
    seed_scenario(&t);
    t.backup("manual");
    t.backup("migration");
    for _ in 0..4 {
        t.backup("auto");
    }
    let removed = backup::cleanup(&t.data_dir, 2).unwrap();
    assert_eq!(removed.len(), 2);
    let dd = t.data_dir.clone();
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    assert_eq!(l.backups.iter().filter(|b| b.kind == "auto").count(), 2);
    assert_eq!(l.backups.iter().filter(|b| b.protected).count(), 2, "수동·마이그레이션은 남는다");
    let cl = t.hist("cleanup");
    assert_eq!(cl.len(), 2);
    assert!(cl[0].summary.contains("보관 2개 초과"));
    // 정기 자동 백업은 하루 한 번
    let dd = t.data_dir.clone();
    assert!(t.db.read(|c| backup::auto_daily(c, &dd)).unwrap().is_none(), "오늘 자동 백업이 있으면 건너뛴다");
    t.db.write(|tx| crate::repo::setting::set(tx, "backup.auto", "0")).unwrap();
    assert!(t.db.read(|c| backup::auto_daily(c, &dd)).unwrap().is_none());
}

// ── B. 복원 (8~15) · 시나리오 1·2·3 ─────────────────────────────────────

#[test]
fn b_full_restore_round_trip_with_safety_backup_and_history() {
    let t = setup();
    let (batch, refund) = seed_scenario(&t);
    t.upload_fixture();
    let snapshot = t.counts();
    let info = t.backup("manual");
    // 백업 뒤 추가 등록
    t.collect(3, t.ben, t.instr, 50_000);
    t.db.write(|tx| voucher::target_save(tx, &VoucherTargetInput { school_year_id: t.year_id, student_id: t.stu, status: "ended".into(), confirmed_on: Some("2026-05-10".into()), apply_from: Some("2026-03-01".into()), apply_to: Some("2026-06-30".into()), transfer_in: false, transfer_in_on: None, note: String::new(), reason: "시험".into() })).unwrap();
    let later = t.counts();
    assert!(later.ledger_entries > snapshot.ledger_entries);
    // 10. 미리보기
    let pv = restore::inspect(&t.db, Path::new(&info.path)).unwrap();
    assert!(pv.can_restore, "{:?}", pv.blocked);
    assert_eq!(pv.current, later);
    assert_eq!(pv.backup.as_ref().unwrap(), &snapshot);
    assert!(pv.warnings.iter().any(|w| w.contains("사라집니다")));
    // 확인 없이는 실행되지 않는다
    assert_eq!(restore::apply(&t.db, Path::new(&info.path), false).unwrap_err().code, "INVALID");
    assert_eq!(t.counts(), later);
    // 8·9. 복원 + 안전 백업
    let r = restore::apply(&t.db, Path::new(&info.path), true).unwrap();
    assert!(r.ok, "{}", r.message);
    // 복원 뒤에는 '복원했다'는 감사 로그 한 줄이 더해지므로 감사 건수는 빼고 비교한다
    let strip = |c: &DbCounts| DbCounts { audit_logs: 0, ..c.clone() };
    assert_eq!(r.after, snapshot, "백업 당시 자료로 돌아간다 (감사 로그는 그 뒤에 한 줄 더해진다)");
    assert_eq!(t.counts().audit_logs, snapshot.audit_logs + 1, "복원 감사 로그");
    assert_eq!((r.before.ledger_entries, r.before.voucher_targets), (later.ledger_entries, later.voucher_targets));
    assert_eq!(strip(&t.counts()), strip(&snapshot));
    let safety = r.safety_backup.clone().unwrap();
    let dd = t.data_dir.clone();
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    let sb = l.backups.iter().find(|b| b.file_name == safety).expect("안전 백업");
    assert!(sb.verified && sb.kind == "pre_restore" && sb.protected);
    assert_eq!(sb.counts.as_ref().unwrap().ledger_entries, later.ledger_entries, "안전 백업에는 복원 직전 자료가 들어 있다");
    // 연결 유지: 묶음·경정·환불·감사 이력
    let b2 = t.db.read(|c| voucher::batch_get(c, batch.id)).unwrap();
    assert_eq!(b2.items.len(), batch.items.len());
    let a2 = t.db.read(|c| adjustment::get(c, refund.id)).unwrap();
    assert_eq!((a2.kind.as_str(), a2.status.as_str()), ("refund_return", "done"));
    let rl = t.db.read(|c| voucher::refund_list(c, t.year_id)).unwrap();
    assert_eq!(rl[0].status, "complete");
    assert_eq!(t.db.read(|c| voucher::target_list(c, t.year_id)).unwrap()[0].status, "confirmed", "백업 뒤의 상태 변경은 사라진다");
    // 14. 복원 후 무결성 · 15. 이력 (DB 밖)
    let rep = t.db.read(|c| integrity::run(c, &dd, t.year_id)).unwrap();
    assert_eq!(rep.error_count, 0, "{:?}", rep.items);
    let h = t.hist("restore");
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].status, "ok");
    assert_eq!(h[0].detail["safetyBackup"].as_str(), Some(safety.as_str()));
    assert!(!r.before_db_sha256.is_empty() && !r.after_db_sha256.is_empty() && r.before_db_sha256 != r.after_db_sha256);
    assert!(!Path::new(&format!("{}.before-restore", t.db.path().display())).exists());
    // 복원 뒤에도 쓰기가 된다 (연결 정상)
    t.collect(4, t.ben, t.instr, 1_000);
    assert_eq!(t.counts().ledger_entries, snapshot.ledger_entries + 1);
    // 복원 사실이 감사 로그에도 남는다
    let audits = t.db.read(|c| crate::repo::audit::list(c, 5)).unwrap();
    assert!(audits.iter().any(|a| a.kind == "ops.restore"));
}

#[test]
fn b_corrupted_and_incompatible_backups_are_blocked_and_db_untouched() {
    let t = setup();
    seed_scenario(&t);
    let before = t.counts();
    let info = t.backup("manual");
    corrupt_zip(Path::new(&info.path));
    // 11. 손상 → 차단
    let err = restore::apply(&t.db, Path::new(&info.path), true).unwrap_err();
    assert!(err.message.contains("복원할 수 없습니다"));
    assert_eq!(t.counts(), before);
    assert_eq!(t.hist("restore")[0].status, "blocked");
    // 12. 호환되지 않는 백업(미래 형식·미래 스키마) → 차단
    let info2 = t.backup("manual");
    let m = backup::read_manifest(Path::new(&info2.path)).unwrap();
    let fake = t.data_dir.join("backups").join("future.zip");
    {
        let f = std::fs::File::create(&fake).unwrap();
        let mut z = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default();
        let mut m2 = m.clone();
        m2.format_version = backup::FORMAT_VERSION + 1;
        m2.schema_version = crate::db::migrate::latest_version() + 5;
        use std::io::Write;
        z.start_file("manifest.json", opt).unwrap();
        z.write_all(serde_json::to_string(&m2).unwrap().as_bytes()).unwrap();
        z.finish().unwrap();
    }
    let pv = restore::inspect(&t.db, &fake).unwrap();
    assert!(!pv.can_restore);
    assert!(pv.blocked.iter().any(|b| b.contains("지원하지 않는 백업 형식")) || pv.blocked.iter().any(|b| b.contains("검증 실패")));
    assert!(restore::apply(&t.db, &fake, true).is_err());
    assert_eq!(t.counts(), before);
    // 이전 형식(.db) 은 목록에 '실패/이전 형식' 으로만
    std::fs::write(t.data_dir.join("backups").join("before-migrate-v3-x.db"), b"x").unwrap();
    let dd = t.data_dir.clone();
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    assert!(l.failed.iter().any(|b| b.legacy));
}

#[test]
fn b_failure_during_restore_keeps_existing_db_and_records_it() {
    let t = setup();
    seed_scenario(&t);
    let info = t.backup("manual");
    t.collect(3, t.ben, t.instr, 50_000);
    // 시나리오 3: 파일 이동 뒤 · 복사 뒤(깨진 파일) · 검증 뒤 — 세 지점 모두 기존 DB 유지
    for point in ["after_move", "after_copy", "after_verify"] {
        let before = t.counts();
        let r = restore::apply_inner(&t.db, Path::new(&info.path), true, Some(point)).unwrap();
        assert!(!r.ok, "{point}");
        assert!(r.message.contains("기존 자료를 그대로"), "{point}: {}", r.message);
        assert_eq!(t.counts(), before, "{point}: 기존 DB 유지");
        assert!(r.safety_backup.is_some());
        let h = t.hist("restore");
        assert_eq!(h[0].status, "failed", "{point}");
        assert!(h[0].summary.contains("기존 자료 유지"), "{point}: {}", h[0].summary);
        // 복원 뒤에도 쓰기 가능
        let n = t.counts().ledger_entries;
        let id = t.collect(4, t.ben, t.instr, 1_000);
        assert_eq!(t.counts().ledger_entries, n + 1);
        t.db.write(|tx| ledger::void(tx, id, "input_error", "시험").map(|_| ())).unwrap();
    }
    // 실패 뒤 정상 복원은 여전히 된다
    let r = restore::apply(&t.db, Path::new(&info.path), true).unwrap();
    assert!(r.ok);
}

// ── C. 무결성 (16~23) · 시나리오 5·6 ──────────────────────────────────────

#[test]
fn c_integrity_is_clean_on_consistent_data_and_flags_known_problems() {
    let t = setup();
    seed_scenario(&t);
    let dd = t.data_dir.clone();
    let rep = t.db.read(|c| integrity::run(c, &dd, t.year_id)).unwrap();
    assert_eq!(rep.error_count, 0, "{:?}", rep.items);
    assert!(rep.checks.iter().any(|c| c.code == "sqlite" && c.level == "ok"));
    assert!(rep.checks.iter().any(|c| c.code == "foreign_keys" && c.level == "ok"));
    assert!(rep.checks.iter().any(|c| c.code == "transfer_pairs" && c.level == "ok"), "18");
    assert!(rep.checks.iter().any(|c| c.code == "refund_cap" && c.level == "ok"), "19");
    assert!(rep.checks.iter().any(|c| c.code == "voucher_limit" && c.level == "ok"), "20");
    assert!(rep.checks.iter().any(|c| c.code == "closed_month" && c.level == "ok"), "21");
    assert!(rep.checks.iter().any(|c| c.code == "upload_links" && c.level == "ok"), "22");
    assert!(rep.checks.iter().any(|c| c.code == "audit" && c.level == "ok"), "23");
    // 시나리오 5: 전체 금액 보존 · 한도 준수 · 환불 중복 없음 · 원본 연결 정상
    let u = t.db.read(|c| voucher::usage(c, t.year_id, t.stu, &[])).unwrap();
    assert_eq!(u.rows.iter().map(|r| r.tuition).sum::<i64>(), 130_000);
    assert_eq!(u.over_total, 0);
    assert!(u.dual, "이용권 실적 → 중복 대상자");
    assert!(rep.items.iter().any(|i| i.code == "av_unconfirmed"), "실적은 있으나 대상 확인 없음 → 확인 필요");
    // 확정월 뒤 변경 → 확인 필요 (21)
    t.db.write(|tx| year::month_close(tx, t.m(4), "확정")).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100)); // created_at 초 단위 비교
    t.collect(4, t.ben, t.instr, 1_000);
    // 반환 누계 초과 (19) — 트리거 없이 직접 넣어 오류를 흉내낸다
    t.db.write(|tx| {
        tx.execute("UPDATE voucher_refund SET collected_confirmed = 0 WHERE id = 1", [])?;
        Ok(())
    }).unwrap();
    // 재원경정 쌍 손상 (18): 쌍 중 하나만 void
    let eid: i64 = t.db.read(|c| Ok(c.query_row("SELECT min(e.id) FROM ledger_entry e WHERE e.flow = 'transfer' AND e.state = 'active'", [], |r| r.get(0))?)).unwrap();
    t.db.write(|tx| {
        tx.execute("UPDATE ledger_entry SET state = 'voided' WHERE id = ?1", [eid])?;
        Ok(())
    }).unwrap();
    let rep = t.db.read(|c| integrity::run(c, &dd, t.year_id)).unwrap();
    assert!(rep.error_count >= 2, "{:?}", rep.items.iter().map(|i| (&i.code, &i.level)).collect::<Vec<_>>());
    assert!(rep.items.iter().any(|i| i.code == "transfer_pairs" && i.level == "error"));
    assert!(rep.items.iter().any(|i| i.code == "adjustment_links" && i.level == "error"));
    assert!(rep.items.iter().any(|i| i.code == "refund_evidence" && i.level == "check"));
    assert!(rep.items.iter().any(|i| i.code == "closed_month" && i.level == "check"));
    assert!(rep.items.iter().all(|i| !i.cause.is_empty() && !i.how_to_check.is_empty() && !i.fix_hint.is_empty()));
    // 검사는 아무것도 고치지 않는다
    let state: String = t.db.read(|c| Ok(c.query_row("SELECT state FROM ledger_entry WHERE id = ?1", [eid], |r| r.get(0))?)).unwrap();
    assert_eq!(state, "voided");
    assert_eq!(t.hist("integrity").len(), 0, "repo 검사 자체는 이력을 남기지 않고 명령 층에서 남긴다");
}

#[test]
fn c_sum_item_duplicate_is_flagged_and_excluded_from_retro() {
    // 시나리오 6: 강사료 30,000 · 재료비 10,000 · 합산 40,000
    let t = setup();
    t.policy(600_000);
    t.collect(2, t.ben, t.instr, 30_000);
    t.collect(2, t.ben, t.material, 10_000);
    t.collect(2, t.ben, t.tm, 40_000);
    t.confirm();
    let u = t.db.read(|c| voucher::usage(c, t.year_id, t.stu, &[])).unwrap();
    assert_eq!(u.used_expected, 30_000, "80,000 으로 계산하지 않는다 — 합산·재료비는 확인 전 제외");
    assert_eq!(u.rows.iter().filter(|r| r.state == "dup_check").count(), 2);
    assert!(u.warnings.iter().any(|w| w.contains("합산")));
    let p = t.db.read(|c| voucher::retro_preview(c, t.year_id, &[t.stu], &[])).unwrap();
    assert_eq!((p.normal_count, p.normal_total), (1, 30_000), "확인 전 일괄 경정 차단");
    let dd = t.data_dir.clone();
    let rep = t.db.read(|c| integrity::run(c, &dd, t.year_id)).unwrap();
    assert!(rep.items.iter().any(|i| i.code == "sum_item_dup" && i.level == "check"));
    // 기존 원장 보존
    assert_eq!(t.counts().active_entries, 3);
    // 독립 합산(개별 항목 없음)은 그대로 계산 (30)
    let t2 = setup();
    t2.policy(600_000);
    t2.collect(2, t2.ben, t2.tm, 40_000);
    t2.confirm();
    let u2 = t2.db.read(|c| voucher::usage(c, t2.year_id, t2.stu, &[])).unwrap();
    assert_eq!(u2.used_expected, 40_000);
    assert!(u2.rows.iter().all(|r| r.state != "dup_check"));
}

// ── G. Excel 출력 (39~43) ────────────────────────────────────────────────

fn read_sheet(path: &Path, sheet: &str) -> Vec<Vec<String>> {
    use calamine::{open_workbook, Data, Reader, Xlsx};
    let mut wb: Xlsx<_> = open_workbook(path).unwrap();
    let r = wb.worksheet_range(sheet).unwrap();
    r.rows()
        .map(|row| {
            row.iter()
                .map(|c| match c {
                    Data::Float(f) => format!("{}", *f as i64),
                    Data::Int(i) => i.to_string(),
                    Data::String(s) => s.clone(),
                    Data::Empty => String::new(),
                    other => format!("{other}"),
                })
                .collect()
        })
        .collect()
}

#[test]
fn g_excel_exports_have_headers_numbers_filters_and_totals() {
    let t = setup();
    seed_scenario(&t);
    let dd = t.data_dir.clone();
    // 39. 월별 검증 (필터 반영)
    let out = t.data_dir.join("월별.xlsx");
    let r = t.db.read(|c| export::recon_month(c, &dd, t.m(1), &ReconExportFilter { status: Some("all".into()), item_set: Some("instructor".into()), program_text: Some("코딩".into()) }, &out)).unwrap();
    assert!(r.rows >= 2 && Path::new(&r.path).exists());
    let rows = read_sheet(&out, "월별 검증");
    assert_eq!(rows[0][0], "학년도");
    assert_eq!(rows[0][5], "징수액");
    assert!(rows.iter().any(|x| x[2] == "코딩" && x[4] == "방과후이용권" && x[5] == "60000"), "{rows:?}");
    let total = rows.last().unwrap();
    assert_eq!(total[0], "합계");
    assert_eq!(total[5], "100000", "코딩 3월 징수 합계 (이용권 60,000 + 자유 40,000)");
    // 필터: 프로그램 이름이 다르면 0행 + 합계만
    let out0 = t.data_dir.join("월별0.xlsx");
    let r0 = t.db.read(|c| export::recon_month(c, &dd, t.m(1), &ReconExportFilter { status: Some("all".into()), item_set: None, program_text: Some("없는프로그램".into()) }, &out0)).unwrap();
    assert_eq!(r0.rows, 0);
    // 40. 사용액
    let out = t.data_dir.join("사용액.xlsx");
    let r = t.db.read(|c| export::voucher_usage(c, &dd, t.year_id, &out)).unwrap();
    assert_eq!(r.rows, 1);
    let rows = read_sheet(&out, "자유수강권 사용액");
    assert_eq!(&rows[0][..5], &["학년도", "학년", "반", "번호", "학생명"]);
    assert_eq!(rows[1][4], "가명A");
    assert_eq!(rows[1][7], "600000");
    let hdr = &rows[0];
    let i_mar = hdr.iter().position(|h| h == "3월 사용액").unwrap();
    let i_cum = hdr.iter().position(|h| h == "누적 사용액").unwrap();
    let i_av = hdr.iter().position(|h| h == "방과후이용권 사용액").unwrap();
    assert_eq!((rows[1][i_mar].as_str(), rows[1][i_cum].as_str(), rows[1][i_av].as_str()), ("40000", "40000", "60000"), "일반 수익자 30,000 은 중복 대상자라 확인 전이므로 전환되지 않았다");
    assert_eq!(rows[2][0], "합계");
    assert_eq!(rows[2][i_cum], "40000");
    // 41. 환불 (필터)
    let out = t.data_dir.join("환불.xlsx");
    let r = t.db.read(|c| export::voucher_refunds(c, &dd, t.year_id, &RefundExportFilter { status: Some("all".into()), fund_name: Some("초3 초과금".into()) }, &out)).unwrap();
    assert_eq!(r.rows, 1);
    let rows = read_sheet(&out, "소급 환불");
    assert_eq!(rows[1][5], "초3 초과금");
    assert_eq!((rows[1][6].as_str(), rows[1][8].as_str(), rows[1][9].as_str(), rows[1][10].as_str()), ("40000", "40000", "0", "처리 완료"));
    assert_eq!(rows[2][0], "합계");
    let r_open = t.db.read(|c| export::voucher_refunds(c, &dd, t.year_id, &RefundExportFilter { status: Some("open".into()), fund_name: None }, &t.data_dir.join("환불2.xlsx"))).unwrap();
    assert_eq!(r_open.rows, 0, "미처리 환불 대상이 없다 (유일한 경정은 처리 완료)");
    // 42. 무결성
    let out = t.data_dir.join("무결성.xlsx");
    let r = t.db.read(|c| export::integrity_report(c, &dd, t.year_id, &out)).unwrap();
    assert!(Path::new(&r.path).exists());
    let rows = read_sheet(&out, "무결성 검사");
    assert_eq!(rows[0][2], "결과 등급");
    assert!(rows.iter().any(|x| x[0] == "요약"));
    let sum = read_sheet(&out, "검사 항목 요약");
    assert!(sum.len() > 10);
    // 내보내기 이력
    assert_eq!(t.hist("export").len(), 6);
}

#[test]
fn ops_history_survives_restore() {
    let t = setup();
    seed_scenario(&t);
    let info = t.backup("manual");
    let n = history::list(&t.data_dir, 100).unwrap().len();
    restore::apply(&t.db, Path::new(&info.path), true).unwrap();
    let h = history::list(&t.data_dir, 100).unwrap();
    assert!(h.len() >= n + 2, "백업(안전)·복원 기록이 더해진다");
    assert_eq!(h[0].kind, "restore");
    // params 사용 (경고 방지)
    let _ = params![1];
}
