//! 실제 9월 파일로 하는 대조 검증 — 저장소 밖의 실제 파일을 쓰므로 환경변수로만 돈다.
//!
//!   ASACCOUNTING_REAL_DIR="C:/Users/monmi/Documents/방과후 회계 자동화" cargo test --lib real_september_recon -- --ignored --nocapture
//!
//! 출력에 학생 이름·업체 연락처는 넣지 않는다(프로그램명·금액만). 실사용 DB 가 아니라 메모리 DB 다.

use std::path::PathBuf;

use crate::db::Db;
use crate::domain::money::format_won as w;
use crate::model::{PreviewRequest, ProgramMapInput};
use crate::repo::{adjustment, allocation, program, recon, upload, year};

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
    assert!(r1.rows.iter().filter(|r| r.item_set == "instructor" || r.item_set == "textbook_material").all(|r| r.status == "unallocated" || r.status == "group_only"), "미배분이 남아 있는 동안 정상 판정이 나오면 안 된다");
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
                let input = ProgramMapInput { school_year_id: self.year_id, alias: u.text.clone(), target: "new_group".into(), program_id: None, group_id: None, new_name: None, member_ids: Some(ids) };
                self.db.write(|tx| program::map_save(tx, &input)).unwrap();
            }
        }
        self.map_all(req);
    }
}
