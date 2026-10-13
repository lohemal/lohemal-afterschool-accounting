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
    // P5 시험은 비암호화 zip 기준. 암호화는 P6 시험(p6_*)에서 따로 본다.
    db.write(|tx| crate::repo::setting::set(tx, "backup.encrypt", "none")).unwrap();
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
    let m = backup::read_manifest(Path::new(&info.path), &t.data_dir, None).unwrap();
    assert_eq!(m.counts, before, "메타데이터 건수 = 백업 시점 건수");
    assert!(m.files.iter().any(|x| x.path == format!("uploads/{}", f.stored_name)));
    assert_eq!(m.schema_version, crate::db::migrate::latest_version());
    let v = backup::verify(Path::new(&info.path), &t.data_dir).unwrap();
    assert!(v.ok && v.errors.is_empty());
    // 시나리오 4: 백업 뒤 새 거래가 들어와도 백업은 백업 시점 그대로 (스냅샷)
    t.collect(3, t.ben, t.instr, 10_000);
    let v2 = backup::verify(Path::new(&info.path), &t.data_dir).unwrap();
    assert!(v2.ok);
    assert_eq!(backup::read_manifest(Path::new(&info.path), &t.data_dir, None).unwrap().counts.ledger_entries, before.ledger_entries);
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
    let v = backup::verify(Path::new(&info.path), &t.data_dir).unwrap();
    assert!(!v.ok, "5. 손상 감지");
    assert!(v.errors.iter().any(|e| e.contains("체크섬") || e.contains("구조") || e.contains("읽는 중")), "{:?}", v.errors);
    // 곁 파일은 생성 당시 '검증됨' 이지만 다시 검증하면 실패 → 복원 미리보기가 막는다
    let pv = restore::inspect(&t.db, Path::new(&info.path), None).unwrap();
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
    let removed = backup::cleanup(&t.data_dir, &backup::default_dir(&t.data_dir), 2).unwrap();
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
    let pv = restore::inspect(&t.db, Path::new(&info.path), None).unwrap();
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
    let m = backup::read_manifest(Path::new(&info2.path), &t.data_dir, None).unwrap();
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
    let pv = restore::inspect(&t.db, &fake, None).unwrap();
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
        let r = restore::apply_inner(&t.db, Path::new(&info.path), true, Some(point), None).unwrap();
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

// ══════════════════════════════════════════════════════════════════════════
// P6 — 암호화(1~8) · 백업 경로(9~15) · 월 확정 지문(16~20) · PC 이전(31~35) · 마이그레이션(25)
// ══════════════════════════════════════════════════════════════════════════

fn set(t: &T, key: &str, value: &str) {
    t.db.write(|tx| crate::repo::setting::set(tx, key, value)).unwrap();
}

fn file_contains(path: &Path, needle: &str) -> bool {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).contains(needle)).unwrap_or(false)
}

#[test]
fn p6_a_password_backup_round_trip_wrong_password_and_no_leaks() {
    let t = setup();
    seed_scenario(&t);
    let f = t.upload_fixture();
    let pw = "교직원-이전용-2026!";
    // 3. 비밀번호 암호화 백업
    let dd = t.data_dir.clone();
    let info = t.db.read(|c| backup::create_with_password(c, &dd, "manual", "PC 이전용", Some(pw))).unwrap();
    assert!(info.file_name.ends_with(".asbk") && info.encrypted && info.encryption == "password" && info.verified, "{:?}", info);
    assert!(info.complete && info.file_count == 2);
    let bytes = std::fs::read(&info.path).unwrap();
    assert!(crate::ops::crypto::is_encrypted_file(&bytes));
    assert!(!String::from_utf8_lossy(&bytes).contains("manifest.json"), "zip 구조가 밖에서 보이지 않는다");
    assert!(!String::from_utf8_lossy(&bytes).contains("가명A"), "학생 이름이 평문으로 없다");
    // 8. 비밀번호·키가 어디에도 기록되지 않는다
    for p in [PathBuf::from(format!("{}.meta.json", info.path)), PathBuf::from(format!("{}.verify.json", info.path)), history::file_path(&t.data_dir)] {
        assert!(!file_contains(&p, pw), "{}", p.display());
    }
    assert!(!file_contains(&t.data_dir.join("asaccounting.db"), pw));
    // 목록: meta 로 건수가 보인다 (복호화 없이)
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    let b = l.backups.iter().find(|b| b.file_name == info.file_name).unwrap();
    assert_eq!((b.encryption.as_str(), b.counts.as_ref().map(|c| c.uploads)), ("password", Some(1)));
    // 5. 잘못된 비밀번호 → 검증 실패·복원 차단 (메시지에 비밀번호 없음)
    let v = backup::verify_with(Path::new(&info.path), &dd, Some("틀린비밀번호!!")).unwrap();
    assert!(!v.ok && v.errors.iter().any(|e| e.contains("비밀번호가 틀리거나")));
    let pv = restore::inspect(&t.db, Path::new(&info.path), Some("틀린비밀번호!!")).unwrap();
    assert!(!pv.can_restore);
    let pv0 = restore::inspect(&t.db, Path::new(&info.path), None).unwrap();
    assert!(!pv0.can_restore && pv0.blocked.iter().any(|b| b.contains("비밀번호를 입력")));
    let before = t.counts();
    assert!(restore::apply_with_password(&t.db, Path::new(&info.path), true, Some("틀린비밀번호!!")).is_err());
    assert_eq!(t.counts(), before);
    // 4. 올바른 비밀번호로 복원
    t.collect(3, t.ben, t.instr, 50_000);
    let r = restore::apply_with_password(&t.db, Path::new(&info.path), true, Some(pw)).unwrap();
    assert!(r.ok, "{}", r.message);
    assert_eq!(t.counts().ledger_entries, before.ledger_entries);
    assert!(crate::repo::upload::uploads_dir(&t.data_dir).join(&f.stored_name).is_file());
    // 6. 손상된 암호화 파일
    corrupt_zip(Path::new(&info.path));
    let v = backup::verify_with(Path::new(&info.path), &dd, Some(pw)).unwrap();
    assert!(!v.ok);
    assert!(restore::apply_with_password(&t.db, Path::new(&info.path), true, Some(pw)).is_err());
    // 7. 옛 zip 백업 호환 — 읽히지만 '암호화되지 않았습니다'
    set(&t, "backup.encrypt", "none");
    let z = t.backup("manual");
    assert!(z.file_name.ends_with(".zip") && !z.encrypted);
    let pv = restore::inspect(&t.db, Path::new(&z.path), None).unwrap();
    assert!(pv.can_restore);
    assert!(pv.warnings.iter().any(|w| w.contains("암호화되지 않았습니다")));
    // 임시 파일이 남지 않는다
    let leftovers: Vec<String> = std::fs::read_dir(&t.data_dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with(".backup-") || n.starts_with(".restore-")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[cfg(windows)]
#[test]
fn p6_a_dpapi_backup_is_default_for_auto_and_restores_without_password() {
    let t = setup();
    seed_scenario(&t);
    set(&t, "backup.encrypt", "dpapi");
    let dd = t.data_dir.clone();
    // 1. DPAPI 백업 (자동 백업 기본)
    let info = t.db.read(|c| backup::auto_daily(c, &dd)).unwrap().expect("오늘 첫 자동 백업");
    assert!(info.file_name.ends_with(".asbk") && info.encryption == "dpapi" && info.verified);
    let m = backup::read_manifest(Path::new(&info.path), &dd, None).unwrap();
    assert_eq!(m.kind, "auto");
    // 2. DPAPI 복원 (비밀번호 없이)
    t.collect(3, t.ben, t.instr, 10_000);
    let before = t.counts();
    let r = restore::apply_with_password(&t.db, Path::new(&info.path), true, None).unwrap();
    assert!(r.ok);
    assert_eq!(t.counts().ledger_entries, before.ledger_entries - 1);
    // 마이그레이션 직전 백업도 설정(dpapi)을 따른다
    assert!(t.db.read(|c| backup::auto_daily(c, &dd)).unwrap().is_none(), "하루 한 번");
}

#[test]
fn p6_b_custom_backup_dir_fallback_and_failure_handling() {
    let t = setup();
    seed_scenario(&t);
    let dd = t.data_dir.clone();
    // 9. 기본 경로
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    assert!(l.custom_dir.is_none() && l.dir == backup::default_dir(&t.data_dir).display().to_string());
    // 10·11. 사용자 지정 경로(다른 폴더 = 다른 드라이브·USB 흉내)
    let other = tempfile::tempdir().unwrap();
    set(&t, "backup.dir", &other.path().display().to_string());
    let info = t.backup("manual");
    assert_eq!(info.location, other.path().display().to_string());
    assert!(!info.fallback && Path::new(&info.path).starts_with(other.path()));
    let l = t.db.read(|c| backup::list(c, &dd)).unwrap();
    assert_eq!(l.custom_dir.as_deref(), Some(other.path().display().to_string().as_str()));
    assert!(l.dir_check.ok);
    assert!(l.backups.iter().any(|b| b.file_name == info.file_name));
    // 12. 경로 접근 실패 (USB 분리·네트워크 끊김 흉내) → 실패 기록 + 기본 경로 대체, 저장 위치 명시
    let gone = other.path().join("없는폴더").join("backup");
    set(&t, "backup.dir", &gone.display().to_string());
    let chk = backup::check_dir(&gone, 0);
    assert!(!chk.ok && !chk.exists);
    let fb = t.backup("manual");
    assert!(fb.fallback, "대체 백업 표시");
    assert_eq!(fb.location, backup::default_dir(&t.data_dir).display().to_string());
    let h = t.hist("backup");
    assert!(h.iter().any(|r| r.status == "failed"), "지정 경로 실패가 기록된다");
    assert!(h.iter().any(|r| r.summary.contains("대체 백업")));
    // 15. 대체 백업 끔 → 오류
    set(&t, "backup.fallback", "0");
    let e = t.db.read(|c| backup::create(c, &dd, "manual", "x")).unwrap_err();
    assert_eq!(e.code, "BACKUP_DIR");
    set(&t, "backup.fallback", "1");
    set(&t, "backup.dir", "");
    // 13. 저장 공간 부족
    let chk = backup::check_dir(other.path(), u64::MAX / 2);
    assert!(!chk.ok && chk.message.contains("저장 공간이 부족"));
    assert!(chk.free_bytes.is_some());
    // 쓰기 권한 없음(폴더가 아니라 파일)
    let file_as_dir = other.path().join("file.txt");
    std::fs::write(&file_as_dir, b"x").unwrap();
    assert!(!backup::check_dir(&file_as_dir, 0).ok);
    // 14. 쓰기 중단 → 최종 파일·임시 파일 없음, 실패 기록
    let dir = backup::default_dir(&t.data_dir);
    let before: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    let dd2 = dd.clone();
    let err = t.db.read(|c| backup::create_opts(c, &dd2, "manual", "중단", &backup::CreateOpts { dir: dir.clone(), encryption: "none", password: None, fail_at: Some("after_zip") })).unwrap_err();
    assert_eq!(err.code, "TEST_FAIL");
    let after: Vec<String> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
    assert_eq!(before, after, "임시 파일도 최종 파일도 남지 않는다");
    assert!(t.hist("backup")[0].status == "failed");
    // 시작 때 임시 파일 정리
    std::fs::create_dir_all(dir.join(".backup-stale")).unwrap();
    std::fs::write(dir.join(".tmp-stale.zip"), b"x").unwrap();
    let removed = backup::cleanup_temp(&[dir.clone()]);
    assert_eq!(removed.len(), 2);
}

#[test]
fn p6_c_month_close_fingerprint_detects_changes_regardless_of_time() {
    let t = setup();
    t.policy(600_000);
    t.collect(2, t.ben, t.instr, 30_000);
    t.collect(2, t.g3x, t.instr, 10_000);
    t.confirm();
    let m = t.m(2);
    // 16. 같은 초에 확정과 변경
    t.db.write(|tx| year::month_close(tx, m, "확정")).unwrap();
    let id = t.collect(2, t.ben, t.instr, 5_000);
    let ch = t.db.read(|c| year::month_changes(c, m)).unwrap();
    assert!(ch.has_fingerprint && ch.changed);
    assert_eq!(ch.new_entry_ids, vec![id]);
    assert!(ch.summary.contains("새 거래 1건"));
    let months = t.db.read(|c| year::months(c, t.year_id)).unwrap();
    assert!(months.iter().find(|x| x.id == m).unwrap().changed_after_close, "시각이 같아도 지문으로 잡힌다");
    // 17. 확정 직후 상태 변경(취소)도 잡힌다
    t.db.write(|tx| ledger::void(tx, id, "input_error", "시험").map(|_| ())).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, m)).unwrap();
    assert!(ch.changed && ch.new_entry_ids == vec![id]);
    // 18. 재개 → 정상 정정 → 재확정하면 새 지문 기준으로 '변경 없음'
    t.db.write(|tx| year::month_reopen(tx, m, "소급 처리")).unwrap();
    let p = t.db.read(|c| voucher::retro_preview(c, t.year_id, &[t.stu], &[])).unwrap();
    let rows: Vec<VoucherApplyRow> = p.proposals.iter().filter(|x| x.state == "normal").map(|x| VoucherApplyRow { student_id: x.student_id, fiscal_month_id: x.fiscal_month_id, program_id: x.program_id, amount_item_id: x.amount_item_id, from_fund_id: x.from_fund_id.unwrap(), amount: x.amount, confirmed: false }).collect();
    t.db.write(|tx| voucher::apply(tx, &VoucherApplyInput { school_year_id: t.year_id, kind: "retro".into(), reason: "시험".into(), decided_on: None, processed_on: Some("2026-05-15".into()), status: "done".into(), rows, decisions: vec![] })).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, m)).unwrap();
    assert_eq!(ch.status, "open");
    assert!(ch.changed && !ch.adjustment_changes.is_empty(), "재개 중 변경은 마지막 확정 대비 변경으로 보인다");
    t.db.write(|tx| year::month_close(tx, m, "재확정")).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, m)).unwrap();
    assert!(!ch.changed);
    assert!(!t.db.read(|c| year::months(c, t.year_id)).unwrap().iter().find(|x| x.id == m).unwrap().changed_after_close);
    // 확정 상태에서 정정 상태 변경(취소)은 P3 가 막는다 → 지문도 그대로
    let adj: i64 = t.db.read(|c| Ok(c.query_row("SELECT max(id) FROM adjustment", [], |r| r.get(0))?)).unwrap();
    assert!(t.db.write(|tx| adjustment::cancel(tx, adj, "시험")).is_err());
    assert!(!t.db.read(|c| year::month_changes(c, m)).unwrap().changed);
    // 20. 기존 확정 이력 보존 (3건: 확정·재개·재확정, 첫 확정의 지문 그대로)
    let rows: Vec<(String, String)> = t.db.read(|c| {
        let mut st = c.prepare("SELECT action, fingerprint FROM month_close WHERE fiscal_month_id = ?1 ORDER BY id")?;
        let v: Vec<(String, String)> = st.query_map([m], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }).unwrap();
    assert_eq!(rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["close", "reopen", "close"]);
    assert!(!rows[0].1.is_empty() && rows[0].1 != rows[2].1);
    // 19. 원본 파일 교체(여기서는 거래 취소로 흉내) → 변경 감지
    let e2: i64 = t.db.read(|c| Ok(c.query_row("SELECT min(id) FROM ledger_entry WHERE fiscal_month_id = ?1 AND state = 'active' AND flow = 'collection'", [m], |r| r.get(0))?)).unwrap();
    t.db.write(|tx| year::month_reopen(tx, m, "교체")).unwrap();
    t.db.write(|tx| ledger::void(tx, e2, "input_error", "교체 흉내").map(|_| ())).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, m)).unwrap();
    assert!(ch.changed_entry_ids.contains(&e2));
    // 무결성 검사도 지문 기준으로 '확정 뒤 변경' 을 낸다 (시간 대기 없이)
    t.db.write(|tx| year::month_close(tx, m, "재확정2")).unwrap();
    t.collect(2, t.ben, t.instr, 1_000);
    let dd = t.data_dir.clone();
    let rep = t.db.read(|c| integrity::run(c, &dd, t.year_id)).unwrap();
    assert!(rep.items.iter().any(|i| i.code == "closed_month"));
}

#[test]
fn p6_d_v8_close_history_survives_v9_and_reads_as_legacy() {
    use crate::db::migrate;
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::run_up_to(&mut c, 8).unwrap();
    let y = year::create(&c, 2026).unwrap();
    let m: i64 = c.query_row("SELECT id FROM fiscal_month WHERE school_year_id = ?1 AND seq = 1", [y.id], |r| r.get(0)).unwrap();
    year::month_close(&c, m, "v8 확정").unwrap_or_else(|_| ());
    // v8 에는 fingerprint 칸이 없으므로 직접 행을 넣는다
    c.execute("DELETE FROM month_close", []).ok();
    c.execute("INSERT INTO month_close (fiscal_month_id, action, totals_json, note) VALUES (?1, 'close', '{}', 'v8 확정')", [m]).unwrap();
    c.execute("UPDATE fiscal_month SET status = 'closed' WHERE id = ?1", [m]).unwrap();
    migrate::run(&mut c).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), 9);
    let ch = year::month_changes(&c, m).unwrap();
    assert!(!ch.has_fingerprint && !ch.changed && ch.summary.contains("옛 확정"));
    let n: i64 = c.query_row("SELECT count(*) FROM month_close", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "기존 확정 이력 보존");
}

#[test]
fn p6_f_pc_migration_restore_password_backup_into_a_fresh_data_dir() {
    // 31. 기존 PC: 암호화 백업
    let a = setup();
    let (batch, refund) = seed_scenario(&a);
    let f = a.upload_fixture();
    let pw = "다른PC로-옮기기-2026";
    let dda = a.data_dir.clone();
    let info = a.db.read(|c| backup::create_with_password(c, &dda, "manual", "PC 이전", Some(pw))).unwrap();
    let src_counts = a.counts();
    // 32. 별도 환경(빈 자료 폴더 + 새 DB) 에서 복원
    let dir_b = tempfile::tempdir().unwrap();
    let moved = dir_b.path().join(&info.file_name);
    std::fs::copy(&info.path, &moved).unwrap();
    let db_b = Db::open(&dir_b.path().join("asaccounting.db")).unwrap();
    assert_eq!(db_b.read(backup::counts).unwrap().ledger_entries, 0);
    let pv = restore::inspect(&db_b, &moved, Some(pw)).unwrap();
    assert!(pv.can_restore, "{:?}", pv.blocked);
    let r = restore::apply_with_password(&db_b, &moved, true, Some(pw)).unwrap();
    assert!(r.ok, "{}", r.message);
    let b_counts = db_b.read(backup::counts).unwrap();
    assert_eq!((b_counts.ledger_entries, b_counts.adjustments, b_counts.voucher_targets, b_counts.uploads, b_counts.students), (src_counts.ledger_entries, src_counts.adjustments, src_counts.voucher_targets, src_counts.uploads, src_counts.students));
    // 33. 원본 파일 · 34. 감사 이력 · 35. 자유수강권·환불
    assert!(crate::repo::upload::uploads_dir(dir_b.path()).join(&f.stored_name).is_file());
    assert_eq!(b_counts.audit_logs, src_counts.audit_logs + 1, "복원 감사 로그 한 줄");
    let b2 = db_b.read(|c| voucher::batch_get(c, batch.id)).unwrap();
    assert_eq!(b2.total_amount, batch.total_amount);
    let rl = db_b.read(|c| voucher::refund_list(c, a.year_id)).unwrap();
    assert_eq!(rl[0].status, "complete");
    assert_eq!(db_b.read(|c| adjustment::get(c, refund.id)).unwrap().status, "done");
    let dd_b = dir_b.path().to_path_buf();
    let rep = db_b.read(|c| integrity::run(c, &dd_b, a.year_id)).unwrap();
    assert_eq!(rep.error_count, 0);
    // 복원 이력은 B 환경의 자료 폴더에 남는다
    let h = history::list(dir_b.path(), 10).unwrap();
    assert!(h.iter().any(|x| x.kind == "restore" && x.status == "ok"));
}


/// 만들어진 설치본의 .sig 를 tauri.conf.json 의 공개키로 직접 검증한다 (updater 와 같은 minisign 형식).
/// 설치본이 있을 때만 돈다: `cargo test --lib p6_e_installer_signature -- --ignored`
#[test]
#[ignore]
fn p6_e_installer_signature_verifies_with_public_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("tauri.conf.json")).unwrap()).unwrap();
    let pub_b64 = conf["plugins"]["updater"]["pubkey"].as_str().unwrap();
    let endpoints = conf["plugins"]["updater"]["endpoints"].as_array().unwrap();
    assert!(endpoints.iter().all(|e| e.as_str().unwrap().starts_with("https://")), "HTTPS 만");
    assert!(conf["plugins"]["updater"]["dangerousInsecureTransportProtocol"].is_null());
    use base64::Engine as _;
    let pub_txt = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(pub_b64).unwrap()).unwrap();
    let pk = minisign_verify::PublicKey::decode(&pub_txt).unwrap();
    let dir = root.join("target/release/bundle/nsis");
    let exe = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).find(|p| p.to_string_lossy().ends_with("-setup.exe")).expect("설치본");
    let sig_path = PathBuf::from(format!("{}.sig", exe.display()));
    let sig_txt = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(std::fs::read_to_string(&sig_path).unwrap().trim()).unwrap()).unwrap();
    let sig = minisign_verify::Signature::decode(&sig_txt).unwrap();
    let data = std::fs::read(&exe).unwrap();
    pk.verify(&data, &sig, false).expect("서명 검증");
    // 변조된 파일은 거부
    let mut tampered = data.clone();
    tampered[1000] ^= 0xFF;
    assert!(pk.verify(&tampered, &sig, false).is_err());
    // 설치본(NSIS 압축)과 실행 파일 안에 자료·키가 없다 (개인정보·개인키 문자열 검색). 업데이트 주소·공개키는 실행 파일에 들어 있다.
    let app = std::fs::read(root.join("target/release/asaccounting.exe")).unwrap();
    let priv_key = std::fs::read_to_string(std::path::Path::new(&std::env::var("USERPROFILE").unwrap()).join(".tauri/asaccounting.key")).unwrap();
    let priv_line = priv_key.lines().last().unwrap().trim().to_string();
    for bytes in [&data, &app] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(&priv_line), "개인키가 설치본·실행 파일에 없다");
        assert!(!text.contains("가명A") && !text.contains(".sandbox") && !text.contains("asaccounting.db-wal"));
    }
    let app_text = String::from_utf8_lossy(&app);
    assert!(app_text.contains("releases/latest/download/latest.json"), "업데이트 주소가 실행 파일에 들어 있다");
    assert!(app_text.contains(pub_b64), "공개키가 실행 파일에 들어 있다");
}

// ══════════════════════════════════════════════════════════════════════════
// P6.1 — 옛 스키마 DB 의 연속 마이그레이션 검증 (앱이 쓰는 경로 `Db::open` 그대로) · 실패 시 보존
//   `ASACCOUNTING_MIGRATE_DIR=<자료 폴더>` 에 있는 asaccounting.db 를 올린다. 건수·합계만 출력하고 이름은 출력하지 않는다.
//   cargo test --lib p61_migrate -- --ignored --nocapture
// ══════════════════════════════════════════════════════════════════════════

fn table_exists(c: &rusqlite::Connection, t: &str) -> bool {
    c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0
}

/// 버전에 따라 표가 없을 수 있다 → "미지원" 으로 적는다.
fn baseline(c: &rusqlite::Connection) -> std::collections::BTreeMap<String, String> {
    let mut m = std::collections::BTreeMap::new();
    let cnt = |t: &str, sql: &str| -> String { if table_exists(c, t) { c.query_row(sql, [], |r| r.get::<_, i64>(0)).map(|v| v.to_string()).unwrap_or_else(|e| format!("오류 {e}")) } else { "미지원".into() } };
    m.insert("schema".into(), c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap().to_string());
    m.insert("ledger_count".into(), cnt("ledger_entry", "SELECT count(*) FROM ledger_entry"));
    m.insert("ledger_active".into(), cnt("ledger_entry", "SELECT count(*) FROM ledger_entry WHERE state = 'active'"));
    m.insert("ledger_sum".into(), cnt("ledger_entry", "SELECT coalesce(sum(amount), 0) FROM ledger_entry WHERE state = 'active'"));
    m.insert("ledger_sum_collection".into(), cnt("ledger_entry", "SELECT coalesce(sum(amount), 0) FROM ledger_entry WHERE state = 'active' AND flow = 'collection'"));
    if table_exists(c, "ledger_entry") {
        let mut st = c.prepare("SELECT coalesce(fund_type_id, 0), coalesce(sum(amount), 0) FROM ledger_entry WHERE state = 'active' GROUP BY fund_type_id ORDER BY 1").unwrap();
        let v: Vec<String> = st.query_map([], |r| Ok(format!("{}:{}", r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap().map(|x| x.unwrap()).collect();
        m.insert("by_fund".into(), v.join(","));
    }
    m.insert("students".into(), cnt("student", "SELECT count(*) FROM student"));
    m.insert("programs".into(), cnt("program", "SELECT count(*) FROM program"));
    m.insert("uploads".into(), cnt("upload_file", "SELECT count(*) FROM upload_file"));
    m.insert("allocations".into(), cnt("allocation", "SELECT count(*) FROM allocation"));
    m.insert("transfers".into(), cnt("adjustment", "SELECT count(*) FROM adjustment WHERE kind = 'fund_transfer'"));
    m.insert("refunds".into(), cnt("adjustment", "SELECT count(*) FROM adjustment WHERE kind IN ('refund', 'refund_return')"));
    m.insert("month_closes".into(), cnt("month_close", "SELECT count(*) FROM month_close"));
    m.insert("audit".into(), cnt("audit_log", "SELECT count(*) FROM audit_log"));
    m.insert("voucher_targets".into(), cnt("voucher_target", "SELECT count(*) FROM voucher_target"));
    m.insert("after_voucher_targets".into(), cnt("after_voucher_target", "SELECT count(*) FROM after_voucher_target"));
    m
}

#[test]
#[ignore]
fn p61_migrate_old_db_through_app_path() {
    let Ok(dir) = std::env::var("ASACCOUNTING_MIGRATE_DIR") else {
        eprintln!("ASACCOUNTING_MIGRATE_DIR 없음 — 건너뜀");
        return;
    };
    let dir = PathBuf::from(dir);
    let db_path = dir.join("asaccounting.db");
    let before = {
        let c = rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        baseline(&c)
    };
    println!("## 마이그레이션 전 기준값 (v{})", before["schema"]);
    for (k, v) in &before {
        println!("  {k} = {v}");
    }
    // 앱이 쓰는 경로 그대로: Db::open → 마이그레이션 직전 백업(패키지) → 연속 마이그레이션
    let db = Db::open(&db_path).expect("Db::open");
    let after = db.read(|c| Ok(baseline(c))).unwrap();
    println!("## 마이그레이션 후 (v{})", after["schema"]);
    for (k, v) in &after {
        println!("  {k} = {v}");
    }
    assert_eq!(after["schema"], crate::db::migrate::latest_version().to_string());
    for k in ["ledger_count", "ledger_active", "ledger_sum", "ledger_sum_collection", "by_fund", "students", "programs", "uploads", "allocations", "transfers", "refunds", "month_closes", "audit"] {
        if before.get(k).map(|v| v != "미지원").unwrap_or(false) {
            assert_eq!(before[k], after[k], "{k} 보존");
        }
    }
    assert_ne!(after["voucher_targets"], "미지원", "자유수강권 표 생성");
    assert_ne!(after["after_voucher_targets"], "미지원", "이용권 대상 표 생성");
    // 무결성·외래키
    db.read(|c| {
        let ic: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        assert_eq!(ic, "ok");
        let fk = c.prepare("PRAGMA foreign_key_check")?.query_map([], |_| Ok(()))?.count();
        assert_eq!(fk, 0);
        Ok(())
    })
    .unwrap();
    // 마이그레이션 직전 백업이 검증된 채 있다
    let l = db.read(|c| backup::list(c, &dir)).unwrap();
    let mig: Vec<&BackupInfo> = l.backups.iter().filter(|b| b.kind == "migration").collect();
    println!("마이그레이션 직전 백업: {}개 (검증됨 {}개, 스키마 {:?})", mig.len(), mig.iter().filter(|b| b.verified).count(), mig.iter().map(|b| b.schema_version).collect::<Vec<_>>());
    assert!(mig.iter().any(|b| b.verified && b.schema_version.to_string() == before["schema"]));
    // 무결성 검사 (학년도가 있을 때)
    let years = db.read(crate::repo::year::list).unwrap();
    for y in &years {
        let rep = db.read(|c| integrity::run(c, &dir, y.id)).unwrap();
        println!("무결성 {}: 정상 {} · 확인 필요 {} · 오류 {}", y.name, rep.ok_count, rep.check_count, rep.error_count);
        for it in rep.items.iter().filter(|i| i.level == "error") {
            println!("  [오류] {} — {}", it.title, it.detail);
        }
        assert_eq!(rep.error_count, 0);
    }
    // 백업·복원 왕복 (격리 폴더 안)
    let b = db.read(|c| backup::create(c, &dir, "manual", "검증")).unwrap();
    assert!(b.verified);
    let r = restore::apply(&db, Path::new(&b.path), true).unwrap();
    assert!(r.ok);
    let again = db.read(|c| Ok(baseline(c))).unwrap();
    assert_eq!(again["ledger_count"], after["ledger_count"]);
    println!("백업·복원 왕복 정상 ({})", b.encryption_label);
}

#[test]
#[ignore]
fn p61_migrate_failure_keeps_old_db_and_backup() {
    let Ok(dir) = std::env::var("ASACCOUNTING_MIGRATE_FAIL_DIR") else {
        eprintln!("ASACCOUNTING_MIGRATE_FAIL_DIR 없음 — 건너뜀");
        return;
    };
    let dir = PathBuf::from(dir);
    let db_path = dir.join("asaccounting.db");
    // 다음 마이그레이션(006)이 만들 표를 미리 다른 구조로 만들어 SQL 실패를 일으킨다
    let before = {
        let c = rusqlite::Connection::open(&db_path).unwrap();
        assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 5, "v5 복사본이어야 한다");
        c.execute("CREATE TABLE voucher_policy (x INTEGER)", []).unwrap();
        baseline(&c)
    };
    let err = match Db::open(&db_path) { Ok(_) => panic!("열리면 안 된다"), Err(e) => e };
    println!("실패 오류: {} / {}", err.code, err.message);
    assert_eq!(err.code, "MIGRATE");
    let after = {
        let c = rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        baseline(&c)
    };
    assert_eq!(after["schema"], "5", "버전이 올라가지 않았다 (잘못된 완료 표시 없음)");
    assert_eq!(before, after, "자료 그대로");
    let bk: Vec<PathBuf> = std::fs::read_dir(dir.join("backups")).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().map(|e| e == "asbk" || e == "zip").unwrap_or(false)).collect();
    assert!(!bk.is_empty(), "마이그레이션 직전 백업이 남아 있다");
    let v = backup::verify(&bk[0], &dir).unwrap();
    assert!(v.ok, "{:?}", v.errors);
    println!("마이그레이션 직전 백업 {}개 검증 통과, DB v5 보존", bk.len());
}

#[test]
fn p61_migration_aborts_when_pre_migration_backup_cannot_be_made() {
    // 파일 DB 를 v8 로 만들고, backups 자리를 파일로 막아 백업이 실패하게 한다 → 마이그레이션을 시작하지 않는다
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("asaccounting.db");
    {
        let mut c = rusqlite::Connection::open(&p).unwrap();
        crate::db::migrate::run_up_to(&mut c, 8).unwrap();
        year::create(&c, 2026).unwrap();
    }
    std::fs::write(dir.path().join("backups"), b"not a dir").unwrap();
    let err = match Db::open(&p) { Ok(_) => panic!("열리면 안 된다"), Err(e) => e };
    assert_eq!(err.code, "MIGRATE_BACKUP", "{err:?}");
    let c = rusqlite::Connection::open(&p).unwrap();
    assert_eq!(c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 8, "백업 없이는 올리지 않는다");
    let n: i64 = c.query_row("SELECT count(*) FROM school_year", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    // 막힌 것을 치우면 정상 진행
    drop(c);
    std::fs::remove_file(dir.path().join("backups")).unwrap();
    let db = Db::open(&p).unwrap();
    assert_eq!(db.read(|c| Ok(crate::db::migrate::current_version(c)?)).unwrap(), crate::db::migrate::latest_version());
}
