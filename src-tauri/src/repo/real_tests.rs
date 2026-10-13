//! 실제 9월 파일로 하는 대조 검증 — 저장소 밖의 실제 파일을 쓰므로 환경변수로만 돈다.
//!
//!   ASACCOUNTING_REAL_DIR="C:/Users/monmi/Documents/방과후 회계 자동화" cargo test --lib real_september_recon -- --ignored --nocapture
//!
//! 출력에 학생 이름·업체 연락처는 넣지 않는다(프로그램명·금액만). 실사용 DB 가 아니라 메모리 DB 다.

use std::path::PathBuf;

use crate::db::Db;
use crate::domain::money::format_won as w;
use crate::model::{PreviewRequest, ProgramMapInput};
use crate::repo::{adjustment, allocation, program, recon, upload, voucher, year};

fn real_dir() -> PathBuf {
    PathBuf::from(std::env::var("ASACCOUNTING_REAL_DIR").expect("ASACCOUNTING_REAL_DIR"))
}

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    dir: tempfile::TempDir,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let sep = db.read(|c| year::months(c, y.id)).unwrap().iter().find(|m| m.ym == "2026-09").unwrap().id;
    T { db, year_id: y.id, sep, dir: tempfile::tempdir().unwrap() }
}

impl T {
    fn req(&self, file: &str, format: &str, kind: &str) -> PreviewRequest {
        PreviewRequest {
            path: real_dir().join(file).display().to_string(),
            school_year_id: self.year_id,
            fiscal_month_id: self.sep,
            format_code: format.into(),
            data_kind: kind.into(),
            overrides: vec![],
            replace_file_id: None,
            note: None,
        }
    }
    /// 미리보기에서 모르는 프로그램 표기를 전부 등록한다. 반 접미(A·B·C)가 붙은 표기는
    /// 사용자가 '제안'을 확정하는 것을 흉내 내어 접미를 뗀 프로그램에 잇는다(시험 전용 자동화).
    fn map_all(&self, req: &PreviewRequest) {
        loop {
            let p = self.db.read(|c| upload::preview(c, req)).unwrap();
            if p.unresolved_programs.is_empty() {
                break;
            }
            for u in &p.unresolved_programs {
                let text = u.text.clone();
                let base: String = {
                    let chars: Vec<char> = text.chars().collect();
                    if chars.len() > 1 && chars[chars.len() - 1].is_ascii_uppercase() { chars[..chars.len() - 1].iter().collect() } else { text.clone() }
                };
                // 숫자 접미 ('토탈공예미니어처1A' → '토탈공예미니어처1') 는 그대로 둔다 — 실제 구분이 다를 수 있다.
                let input = ProgramMapInput {
                    school_year_id: self.year_id,
                    alias: text.clone(),
                    target: "new_program".into(),
                    program_id: None,
                    group_id: None,
                    new_name: Some(base),
                    member_ids: None,
                    parent_id: None,
                };
                self.db.write(|tx| program::map_save(tx, &input)).unwrap();
            }
        }
    }
    fn commit(&self, req: &PreviewRequest) -> crate::model::CommitResult {
        let d = self.dir.path().to_path_buf();
        let p = self.db.read(|c| upload::preview(c, req)).unwrap();
        assert!(p.can_commit, "{}: {:?}", req.path, p.block_reasons);
        self.db.write(|tx| upload::commit(tx, &d, req)).unwrap()
    }
    fn print_recon(&self, title: &str) -> crate::model::MonthRecon {
        let r = self.db.read(|c| recon::month(c, self.sep)).unwrap();
        println!("\n## {title}");
        println!("전체 징수 {} · 전체 지출 {} · 대조 가능 기준액 {} · 대조 가능 지급액 {} · 미배분 {} · 불일치 {}건 · 확인 필요 {}건",
            w(r.collection_total), w(r.expenditure_total), w(r.comparable_basis), w(r.comparable_expense), w(r.unallocated_total), r.mismatch_count, r.todo_count);
        println!("| 프로그램 | 항목 | 징수 기준액 | 지급액 | 차액 | 검증 상태 | 확인 단서 |");
        println!("|---|---|---:|---:|---:|---|---|");
        for row in &r.rows {
            if row.status == "unverifiable" && row.expense == 0 && row.item_set == "other" {
                continue;
            }
            let diff = if row.status == "ok" || row.status == "mismatch" { w(row.diff) } else { "—".into() };
            let hints: Vec<&str> = row.hints.iter().map(|h| h.as_str()).filter(|h| !h.contains("학생을 식별")).collect();
            println!("| {} | {} | {} | {} | {} | {} | {} |", row.unit_name, row.item_label, w(row.basis), w(row.expense), diff, row.status_label, hints.join(" / "));
        }
        r
    }
}

#[test]
#[ignore]
fn real_september_recon() {
    // ── 시나리오 1: 지시서의 기본 흐름 — 징수명단(기안용) 을 최초 징수 기준으로 ─────────
    let t = setup();
    t.commit(&t.req("방과후 징수명단(기안용).xlsx", "collect_initial", "initial_collect"));
    let roster = t.req("수강생명단(방과후 통합 매니저 9월부터).xlsx", "roster_manager", "roster_link");
    t.map_all(&roster);
    t.commit(&roster);
    let board = t.db.read(|c| allocation::pending(c, t.sep)).unwrap();
    println!("\n## 시나리오 1 — 징수명단(기안용) 기준 · 배분 현황");
    println!("미배분 거래 {}건 · 미배분 금액 {} · 학생 식별 불가 {}건 · 자동 배분 가능 {}건 · 사용자 확인 필요 {}건",
        board.entries.len(), w(board.unallocated_total), board.no_student, board.auto_possible, board.need_user);
    let a = t.db.write(|tx| allocation::auto_allocate(tx, t.sep)).unwrap();
    println!("자동 배분 실행: 배분 {}건, 보류 {}건", a.entries_allocated, a.skipped);
    assert_eq!(board.no_student as usize, board.entries.len(), "실제 파일은 이름이 비어 있어 전부 학생 식별 불가여야 한다");
    assert_eq!(a.entries_allocated, 0, "근거 없이 배분하면 안 된다");
    let instr = t.req("2026학년도 9월 방과후프로그램 강사료 내역 양식.xlsx", "instructor_pay", "instructor_pay");
    // 우쿨렐레&통기타 는 그룹으로
    t.map_all_except_group(&instr);
    t.commit(&instr);
    let vend = t.req("2026학년도 방과후프로그램 9월 교재,재료비 지급업체.xlsx", "vendor_pay", "vendor_pay");
    t.map_all(&vend);
    t.commit(&vend);
    let r1 = t.print_recon("시나리오 1 — 징수명단(기안용) 기준 (학생 식별 불가 → 전부 배분 미확정)");
    assert_eq!(r1.unallocated_total, 25_734_600);
    assert!(r1.rows.iter().filter(|r| r.item_set == "instructor" || r.item_set == "textbook_material").all(|r| matches!(r.status.as_str(), "unallocated" | "group_only" | "link_pending")), "미배분이 남아 있는 동안 정상 판정이 나오면 안 된다 (P7-1.1: '토탈공예미니어처1' 처럼 세부반 추천이 걸린 행은 매핑 확인 필요가 먼저)");
    assert_eq!(r1.expenditure_total, 27_329_000 + 9_987_700);

    // ── 시나리오 2: 2학기 운영 — 통합 매니저 명단(최초 징수 시점)을 최초 징수 기준으로 ─────
    let t = setup();
    let roster = t.req("수강생명단(방과후 통합 매니저 9월부터).xlsx", "roster_manager", "initial_collect");
    t.map_all(&roster);
    let p = t.db.read(|c| upload::preview(c, &roster)).unwrap();
    println!("\n## 시나리오 2 — 통합 매니저 명단 기준 · 미리보기");
    println!("재원 미확정 표기: {:?}", p.unresolved_funds.iter().map(|u| (u.text.clone(), u.count)).collect::<Vec<_>>());
    t.commit(&roster);
    let instr = t.req("2026학년도 9월 방과후프로그램 강사료 내역 양식.xlsx", "instructor_pay", "instructor_pay");
    t.map_all_except_group(&instr);
    t.commit(&instr);
    let vend = t.req("2026학년도 방과후프로그램 9월 교재,재료비 지급업체.xlsx", "vendor_pay", "vendor_pay");
    t.map_all(&vend);
    t.commit(&vend);
    let r2 = t.print_recon("시나리오 2 — 통합 매니저 명단 기준 (프로그램 직접 귀속)");
    assert_eq!(r2.collection_total, 24_784_150);
    assert_eq!(r2.expenditure_total, 37_316_700);
    assert_eq!(r2.unallocated_total, 0);
    let groups: Vec<&crate::model::ReconRow> = r2.rows.iter().filter(|r| r.unit_kind == "group").collect();
    assert_eq!(groups.len(), 1, "우쿨렐레&통기타 그룹 강사료");
    assert_eq!(groups[0].expense, 864_000);
    // 재원별 지급액 합이 파일 추출값과 같다 (중복 없음)
    let instr_exp: i64 = r2.rows.iter().filter(|r| r.item_set == "instructor").map(|r| r.expense).sum::<i64>()
        - r2.rows.iter().filter(|r| r.item_set == "instructor" && r.status == "group_only").map(|r| r.expense).sum::<i64>();
    assert_eq!(instr_exp, 27_329_000);
    let vend_exp: i64 = r2.rows.iter().filter(|r| r.item_set == "textbook_material").map(|r| r.expense).sum();
    assert_eq!(vend_exp, 9_987_700);
    assert!(r2.rows.iter().any(|r| r.status == "mismatch"), "9월 전환기라 불일치가 있어야 자연스럽다");
    // 원본 추적: 불일치 하나를 골라 상세를 연다
    let m = r2.rows.iter().find(|r| r.status == "mismatch").unwrap();
    let d = t.db.read(|c| recon::detail(c, t.sep, &m.unit_kind, m.unit_id, &m.item_set)).unwrap();
    assert!(!d.expenditure.is_empty() && d.expenditure.iter().all(|e| e.source_row_id.is_some() && !e.file_name.is_empty()));
    let src = t.db.read(|c| recon::source_row(c, d.expenditure[0].source_row_id.unwrap())).unwrap();
    assert!(src.row_no > 0 && !src.cells.is_empty());
    println!("\n원본 추적 확인: '{}' {} 지급 거래 {}건 → {} 시트 {}행 등 원본 행 연결", m.unit_name, m.item_label, d.expenditure.len(), src.sheet, src.row_no);

    // ── P3 연동: 바둑 교재·재료비 '총액 일치 · 재원 불일치' 에 담당자가 재원경정을 등록하는 흐름 ─────
    let baduk = r2.rows.iter().find(|r| r.unit_name == "바둑" && r.item_set == "textbook_material").expect("바둑 교재·재료비");
    assert!(baduk.total_match_fund_mismatch, "{:?}", baduk.status_label);
    let voucher = baduk.funds.iter().find(|f| f.diff == Some(12_000)).unwrap().fund_id.unwrap();
    let ben = baduk.funds.iter().find(|f| f.diff == Some(-12_000)).unwrap().fund_id.unwrap();
    let item = t.db.read(|c| crate::repo::catalog::amount_item_by_code(c, "textbook_material")).unwrap().id;
    let input = crate::model::AdjustmentInput {
        school_year_id: t.year_id, fiscal_month_id: t.sep, kind: "fund_transfer".into(), program_id: baduk.unit_id, student_id: None,
        from_fund_id: Some(voucher), to_fund_id: Some(ben), fund_type_id: None, amount_item_id: item, amount: 12_000,
        decided_on: Some("2026-10-13".into()), processed_on: Some("2026-10-13".into()), status: "done".into(),
        reason: "실제 자료 점검 — 이용권 12,000 을 일반 수익자로 경정(시험)".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
    };
    let p = t.db.read(|c| adjustment::preview(c, &input)).unwrap();
    println!("\n## P3 — 바둑 교재·재료비 재원경정 미리보기 (프로그램 단위)");
    for f in &p.funds { println!("{}: {} → {} ({})", f.fund_name, w(f.before), w(f.after), w(f.change)); }
    println!("전체 기준액 {} → {}", w(p.total_before), w(p.total_after));
    assert!(p.blocked.is_empty(), "{:?}", p.blocked);
    assert_eq!(p.total_before, p.total_after);
    t.db.write(|tx| adjustment::create(tx, &input)).unwrap();
    let r3 = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    let after = r3.rows.iter().find(|r| r.unit_name == "바둑" && r.item_set == "textbook_material").unwrap();
    println!("경정 후: 상태 {} · 단순 차액 {} · 대조 차액 {} · 재원경정 {}건", after.status_label, w(after.simple_diff), w(after.diff), after.transfer_count);
    assert_eq!(after.status, "ok");
    assert_eq!(after.basis, baduk.basis, "전체 기준액은 그대로");
    assert_eq!(after.transfer_count, 2);
    assert_eq!(after.status_label, "비교 가능 재원 정상 · 초3 초과금 미검증");

    // ── P4 연동: 실제 자료에는 학생 식별 정보가 비어 있어 학생 단위 대상자를 만들 수 없다.
    //    자유수강권 모듈이 실제 DB 상태 위에서 깨지지 않는지, 프로그램 단위 경정이 환불 대상으로 섞이지 않는지만 확인한다.
    println!("\n## P4 — 자유수강권 모듈 (실제 9월 자료 위, 학생 정보 없음)");
    let pol = t.db.write(|tx| voucher::policy_save(tx, &crate::model::VoucherPolicyInput { school_year_id: t.year_id, basis: "annual".into(), annual_limit: Some(600_000), semester1_limit: None, semester2_limit: None, carry_over: false, note: "시험".into(), ..Default::default() })).unwrap();
    println!("정책 등록: {} (연간 {})", pol.basis_label, w(pol.annual_limit.unwrap()));
    let targets = t.db.read(|c| voucher::target_list(c, t.year_id)).unwrap();
    let students = t.db.read(|c| adjustment::students(c, t.year_id)).unwrap();
    println!("학생 표 {}명 · 대상자 {}명 (학생 정보가 비어 있어 등록 불가)", students.len(), targets.len());
    assert!(targets.is_empty());
    let pv = t.db.read(|c| voucher::retro_preview(c, t.year_id, &[], &[])).unwrap();
    println!("소급 미리보기: 학생 {}명 · 제안 {}건", pv.student_count, pv.proposals.len());
    assert_eq!((pv.student_count, pv.proposals.len()), (0, 0));
    let refunds = t.db.read(|c| voucher::refund_list(c, t.year_id)).unwrap();
    println!("환불 대상: {}건 (프로그램 단위 바둑 경정은 포함되지 않음)", refunds.len());
    assert!(refunds.is_empty());
    let h = t.db.read(|c| voucher::history(c, t.year_id)).unwrap();
    assert_eq!(h.policy_history.len(), 1);
    let r4 = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    assert_eq!(r4.rows.len(), r3.rows.len(), "정책 등록은 대조 결과를 바꾸지 않는다");

    // ── P5 연동: 실제 자료 위에서 무결성 검사·Excel 내보내기 (메모리 DB, 임시 폴더) ─────
    println!("
## P5 — 무결성 검사 · 내보내기 (실제 9월 자료)");
    let tmp = tempfile::tempdir().unwrap();
    let dd = tmp.path().to_path_buf();
    let rep = t.db.read(|c| crate::repo::integrity::run(c, &dd, t.year_id)).unwrap();
    println!("무결성: 정상 {} · 확인 필요 {} · 오류 {}", rep.ok_count, rep.check_count, rep.error_count);
    for it in rep.items.iter().take(12) {
        println!("  [{}] {} — {}", crate::repo::integrity::level_label(&it.level), it.title, it.detail);
    }
    assert_eq!(rep.error_count, 0, "{:?}", rep.items.iter().filter(|i| i.level == "error").map(|i| &i.detail).collect::<Vec<_>>());
    let out = dd.join("월별검증.xlsx");
    let x = t.db.read(|c| crate::ops::export::recon_month(c, &dd, t.sep, &crate::model::ReconExportFilter::default(), &out)).unwrap();
    println!("Excel 월별 검증: {}행 → {}", x.rows, out.display());
    assert!(x.rows > 0 && out.exists());
    let x2 = t.db.read(|c| crate::ops::export::integrity_report(c, &dd, t.year_id, &dd.join("무결성.xlsx"))).unwrap();
    println!("Excel 무결성: {}행", x2.rows);
}

impl T {
    /// 강사료 파일: '우쿨렐레&통기타' 는 그룹으로 만들고 나머지는 새 프로그램으로.
    fn map_all_except_group(&self, req: &PreviewRequest) {
        let p = self.db.read(|c| upload::preview(c, req)).unwrap();
        for u in &p.unresolved_programs {
            if u.text.contains('&') {
                let mut ids = Vec::new();
                for part in u.text.split('&') {
                    let name = part.trim();
                    let id = match self.db.read(|c| program::resolve(c, self.year_id, name)).unwrap() {
                        Some(id) => id,
                        None => self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap(),
                    };
                    ids.push(id);
                }
                let input = ProgramMapInput { school_year_id: self.year_id, alias: u.text.clone(), target: "new_group".into(), program_id: None, group_id: None, new_name: None, member_ids: Some(ids), parent_id: None };
                self.db.write(|tx| program::map_save(tx, &input)).unwrap();
            }
        }
        self.map_all(req);
    }
}

// ── P7-1.1: 세부반 → 통합 프로그램 연결로 실제 9월 자료를 다시 검증 ─────────────────
//
//   ASACCOUNTING_REAL_DIR="C:/Users/monmi/Documents/방과후 회계 자동화" cargo test --lib real_september_links -- --ignored --nocapture
//
//   시나리오 2 와 같은 파일이지만, 반 접미가 붙은 명단 표기를 통합 프로그램의 별칭으로 합치지 않고
//   **세부반 프로그램으로 등록**한 뒤 (1) 연결 전 (2) 자동 추천 (3) 추천 확정 후 의 검증 결과를 비교한다.
//   실제 자료에 자동으로 매핑을 확정하지 않는다 — 메모리 DB 에서만 돈다. 학생 이름은 파일에 없고 출력에도 없다.

impl T {
    /// 미리보기의 모르는 표기를 전부 **그 이름 그대로** 새 프로그램으로 등록한다 (세부반 유지, 그룹 표기는 그룹으로).
    fn map_all_verbatim(&self, req: &PreviewRequest) {
        loop {
            let p = self.db.read(|c| upload::preview(c, req)).unwrap();
            if p.unresolved_programs.is_empty() {
                break;
            }
            for u in &p.unresolved_programs {
                if u.text.contains('&') {
                    let mut ids = Vec::new();
                    for part in u.text.split('&') {
                        let name = part.trim();
                        let id = match self.db.read(|c| program::resolve(c, self.year_id, name)).unwrap() {
                            Some(id) => id,
                            None => self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap(),
                        };
                        ids.push(id);
                    }
                    let input = ProgramMapInput { school_year_id: self.year_id, alias: u.text.clone(), target: "new_group".into(), program_id: None, group_id: None, new_name: None, member_ids: Some(ids), parent_id: None };
                    self.db.write(|tx| program::map_save(tx, &input)).unwrap();
                    continue;
                }
                let input = ProgramMapInput { school_year_id: self.year_id, alias: u.text.clone(), target: "new_program".into(), program_id: None, group_id: None, new_name: None, member_ids: None, parent_id: None };
                self.db.write(|tx| program::map_save(tx, &input)).unwrap();
            }
        }
    }
}

#[test]
#[ignore]
fn real_september_links() {
    use crate::repo::program_link;
    let t = setup();
    // 지급 파일 먼저 (통합 프로그램명이 생긴다), 그다음 명단 (세부반 표기 그대로)
    let instr = t.req("2026학년도 9월 방과후프로그램 강사료 내역 양식.xlsx", "instructor_pay", "instructor_pay");
    t.map_all_verbatim(&instr);
    t.commit(&instr);
    let vend = t.req("2026학년도 방과후프로그램 9월 교재,재료비 지급업체.xlsx", "vendor_pay", "vendor_pay");
    t.map_all_verbatim(&vend);
    t.commit(&vend);
    let roster = t.req("수강생명단(방과후 통합 매니저 9월부터).xlsx", "roster_manager", "initial_collect");
    let p0 = t.db.read(|c| upload::preview(c, &roster)).unwrap();
    println!("\n## 1. 원본 프로그램명 (명단 표기 중 지급 파일에 없는 것 {}개)", p0.unresolved_programs.len());
    let mut names: Vec<String> = p0.unresolved_programs.iter().map(|u| format!("{}({}행{})", u.text, u.count, if u.suggestion_kind == "child" { format!(" → 세부반 제안: {}", u.suggestion_name) } else { String::new() })).collect();
    names.sort();
    println!("{}", names.join(" · "));
    let child_suggested = p0.unresolved_programs.iter().filter(|u| u.suggestion_kind == "child").count();
    println!("업로드 단계 세부반 제안 {}개 (확정은 사용자)", child_suggested);
    t.map_all_verbatim(&roster);
    t.commit(&roster);

    // (1) 연결 전
    let r1 = t.print_recon("2. 연결 전 — 세부반을 그대로 등록 (지급은 통합명, 징수는 세부반명)");
    let b1 = t.db.read(|c| program_link::board(c, t.year_id)).unwrap();
    println!("\n## 3. 자동 추천: 추천 {}건 · 후보(조건 미충족) {}건 · 프로그램 {}개", b1.pending_count, b1.candidate_count, b1.rows.len());
    for r in b1.rows.iter().filter(|r| r.suggestion.is_some()) {
        let s = r.suggestion.as_ref().unwrap();
        println!("  {} → {} [{}] {}", r.name, s.parent_name, r.status_label, s.reason);
    }
    let unmapped_before = r1.link_pending_count;
    println!("미매핑(매핑 확인 필요) 행 {}건 · 자료 확인 필요 {}건 · 차액 확인 {}건", unmapped_before, r1.missing_count, r1.mismatch_count);
    assert!(b1.pending_count > 0, "실제 자료에서 추천이 나와야 한다");
    assert!(r1.rows.iter().filter(|r| r.status == "link_pending").all(|r| r.basis >= 0 || r.expense >= 0), "원본 금액 표시");

    // (3) 추천 확정 (시험 전용 — 실제 운영에서는 사용자가 화면에서 확정한다)
    let items: Vec<crate::model::LinkItem> = b1.rows.iter().filter(|r| r.status == "suggested").map(|r| crate::model::LinkItem { program_id: r.program_id, parent_id: Some(r.suggestion.as_ref().unwrap().parent_id) }).collect();
    let pv = t.db.read(|c| program_link::preview(c, t.year_id, &items)).unwrap();
    assert!(!pv.blocked, "{:?}", pv.block_reasons);
    println!("\n## 4. 추천 {}건 일괄 확정 미리보기: 영향 달 {} · 경고 {}건", items.len(), pv.months.iter().map(|m| m.label.clone()).collect::<Vec<_>>().join(","), pv.items.iter().map(|i| i.warnings.len()).sum::<usize>());
    for i in pv.items.iter().filter(|i| !i.warnings.is_empty()) {
        println!("  {}: {}", i.program_name, i.warnings.join(" / "));
    }
    t.db.write(|tx| program_link::apply(tx, t.year_id, &items, "실제 자료 시험 (메모리 DB)", "suggested")).unwrap();
    let r2 = t.print_recon("5. 추천 확정 후 — 통합 프로그램 기준 대조");
    assert_eq!(r2.link_pending_count, 0);
    assert_eq!(r2.collection_total, r1.collection_total, "연결은 징수 총액을 바꾸지 않는다");
    assert_eq!(r2.expenditure_total, r1.expenditure_total, "연결은 지급 총액을 바꾸지 않는다");
    assert_eq!(r2.expenditure_total, 27_329_000 + 9_987_700);
    assert!(r2.rows.len() < r1.rows.len(), "세부반 행이 통합 행 아래로 들어간다");

    println!("\n## 6. 지시서 지정 프로그램 — 연결 전후 비교");
    println!("| 프로그램 | 항목 | 전: 징수 | 전: 지급 | 전: 상태 | 후: 징수 기준액 | 후: 지급액 | 후: 차액 | 후: 상태 | 세부반 |");
    println!("|---|---|---:|---:|---|---:|---:|---:|---|---|");
    for name in ["금요방송댄스", "로봇과학", "만화애니메이션", "난타와사물놀이", "코딩"] {
        for set in ["instructor", "textbook_material"] {
            let after = r2.rows.iter().find(|r| r.unit_name == name && r.item_set == set);
            let Some(after) = after else { continue };
            let before = r1.rows.iter().find(|r| r.unit_name == name && r.item_set == set);
            let (bb, be, bs) = before.map(|b| (w(b.basis), w(b.expense), b.status_label.clone())).unwrap_or_else(|| ("—".into(), "—".into(), "(행 없음)".into()));
            let diff = if after.status == "ok" || after.status == "mismatch" { w(after.diff) } else { "—".into() };
            let kids: Vec<String> = after.children.iter().map(|c| format!("{} {}", c.program_name, w(c.basis))).collect();
            println!("| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", name, after.item_label, bb, be, bs, w(after.basis), w(after.expense), diff, after.status_label, kids.join(" · "));
        }
    }
    let ok_after = r2.rows.iter().filter(|r| r.status == "ok").count();
    let ok_before = r1.rows.iter().filter(|r| r.status == "ok").count();
    println!("\n정상 판정: 연결 전 {}건 → 후 {}건 · 매핑 확인 필요 {} → {} · 자료 확인 필요 {} → {} · 차액 확인 {} → {}", ok_before, ok_after, r1.link_pending_count, r2.link_pending_count, r1.missing_count, r2.missing_count, r1.mismatch_count, r2.mismatch_count);
    // 무결성: 연결 뒤 오류 0
    let tmp = tempfile::tempdir().unwrap();
    let rep = t.db.read(|c| crate::repo::integrity::run(c, tmp.path(), t.year_id)).unwrap();
    println!("무결성: 정상 {} · 확인 필요 {} · 오류 {}", rep.ok_count, rep.check_count, rep.error_count);
    assert_eq!(rep.error_count, 0, "{:?}", rep.items.iter().filter(|i| i.level == "error").map(|i| &i.detail).collect::<Vec<_>>());
    let h = t.db.read(|c| program_link::history(c, t.year_id)).unwrap();
    assert_eq!(h.len(), items.len());
}
