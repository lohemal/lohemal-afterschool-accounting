//! 양식 7종 인식·해석 시험 — `test/fixtures/` 의 가명 파일 (scripts/make-fixtures.py).

use super::detect::{detect, FormatCode};
use super::parse::parse;
use super::{fixtures_dir, Book};

fn book(name: &str) -> Book {
    Book::open(&fixtures_dir().join(name)).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn sum_where(rows: &[super::parse::ParsedRow], f: impl Fn(&super::parse::ParsedRow) -> bool) -> i64 {
    rows.iter().filter(|r| f(r)).map(|r| r.amount).sum()
}

#[test]
fn detects_each_fixture_as_its_own_format() {
    let cases = [
        ("roster_manager.xlsx", FormatCode::RosterManager),
        ("instructor_pay.xlsx", FormatCode::InstructorPay),
        ("vendor_pay.xlsx", FormatCode::VendorPay),
        ("collect_initial.xlsx", FormatCode::CollectInitial),
        ("refund_extra_s1.xlsx", FormatCode::RefundExtraS1),
        ("settle_s1.xlsm", FormatCode::SettleS1),
        ("roster_s1.xlsx", FormatCode::RosterS1),
    ];
    for (name, expect) in cases {
        let d = detect(&book(name));
        assert!(!d.is_empty(), "{name}: 인식 못 함");
        assert_eq!(d[0].format, expect, "{name}: {:?}", d.iter().map(|x| x.code.clone()).collect::<Vec<_>>());
        assert_eq!(d[0].confidence, if expect == FormatCode::RosterS1 { 90 } else { 100 }, "{name}");
    }
}

#[test]
fn xlsm_is_read_as_data_only() {
    let b = book("settle_s1.xlsm");
    assert!(b.sheet("부서별").is_some());
    assert!(super::detect::suggest_month(&book("instructor_pay.xlsx")) == Some(9));
}

#[test]
fn instructor_pay_splits_by_fund_column_and_checks_totals() {
    let p = parse(&book("instructor_pay.xlsx"), FormatCode::InstructorPay).unwrap();
    assert_eq!(p.data_row_count, 4);
    assert_eq!(p.rows.len(), 14, "0 이 아닌 재원 칸마다 한 행");
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 4_290_100);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text.contains("3학년 지원금")), 1_141_000);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "3학년 수익자"), 11_100);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "자유수강권"), 166_000);
    assert!(p.rows.iter().all(|r| r.entry_kind == "instructor_pay" && r.flow == "expenditure" && r.item_code == "instructor"));
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"), "{:?}", p.rows.iter().flat_map(|r| r.issues.clone()).collect::<Vec<_>>());
    assert_eq!(p.rows[0].counterparty, "강사가명1");
    assert!(p.rows.iter().any(|r| r.program_text == "우쿨렐레&통기타"));
    // 합계 행은 자료 행이 아니고, 열 합계 대조는 통과한다.
    assert!(p.file_issues.iter().any(|i| i.level == "info" && i.message.contains("일치")), "{:?}", p.file_issues);
}

#[test]
fn formula_cells_without_cached_values_are_errors_not_zero() {
    let p = parse(&book("instructor_pay_formula.xlsx"), FormatCode::InstructorPay).unwrap();
    assert_eq!(p.rows.len(), 14);
    assert!(p.rows.iter().all(|r| r.worst_level() == "error"), "모든 행의 합계 칸이 계산되지 않은 수식");
    assert!(p.rows[0].issues.iter().any(|i| i.message.contains("수식")));
    // 금액 자체는 그대로 읽힌다 — 0 으로 바뀌지 않는다.
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 4_290_100);
}

#[test]
fn vendor_pay_inherits_merged_program_cells_and_flags_unmerged_blanks() {
    let p = parse(&book("vendor_pay.xlsx"), FormatCode::VendorPay).unwrap();
    assert_eq!(p.data_row_count, 8);
    assert_eq!(p.excluded_row_count, 2, "0원 업체 행 둘은 원장에 넣을 것이 없다");
    assert_eq!(p.rows.len(), 16);
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 2_036_000);
    // 6행: B5:B6 병합 → 로봇과학 이어받음
    let r6: Vec<_> = p.rows.iter().filter(|r| r.row_no == 6).collect();
    assert_eq!(r6.len(), 3);
    assert!(r6.iter().all(|r| r.program_text == "로봇과학" && r.program_inherited && r.worst_level() == "ok"));
    assert_eq!(r6.iter().map(|r| r.amount).sum::<i64>(), 360_000);
    // 8행: 병합 없는 공란 → 확인 필요, 바로 위 '체스' 를 제안
    let r8: Vec<_> = p.rows.iter().filter(|r| r.row_no == 8).collect();
    assert_eq!(r8.len(), 2);
    assert!(r8.iter().all(|r| r.program_text.is_empty() && r.program_suggestion == "체스" && r.worst_level() == "check"));
    assert!(r8[0].issues.iter().any(|i| i.message.contains("둘째 업체")));
    // 합계 행 아래의 교재비/재료비 총액은 자료로 읽히지 않는다.
    assert!(p.rows.iter().all(|r| r.row_no <= 11));
    assert!(p.rows.iter().all(|r| r.item_code == "textbook_material"));
    assert!(p.file_issues.iter().any(|i| i.message.contains("일치")));
}

#[test]
fn roster_manager_maps_items_and_skips_cancelled_rows() {
    let p = parse(&book("roster_manager.xlsx"), FormatCode::RosterManager).unwrap();
    assert_eq!(p.data_row_count, 8);
    assert_eq!(p.excluded_row_count, 1, "취소 수강 1건 제외");
    assert_eq!(p.rows.len(), 20);
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 480_300);
    assert!(p.rows.iter().all(|r| r.entry_kind == "initial_collect"));
    assert_eq!(sum_where(&p.rows, |r| r.item_code == "material"), 51_000 * 2 + 80_000);
    assert!(p.rows.iter().any(|r| r.fund_text == "방과후 이용권 + 자유수강권"));
    let s = p.rows[0].student.as_ref().unwrap();
    assert_eq!((s.grade, s.class_name.as_str(), s.student_no.as_str(), s.name.as_str()), (Some(1), "가람", "3", "가명A"));
    assert!(p.source_rows.len() == 8, "취소 행도 원본에는 남는다");
}

#[test]
fn collect_initial_uses_fund_sheets_and_only_checks_the_total_sheet() {
    let p = parse(&book("collect_initial.xlsx"), FormatCode::CollectInitial).unwrap();
    let total: i64 = p.rows.iter().map(|r| r.amount).sum();
    assert_eq!(total, 484_400);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "beneficiary"), 204_800);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "free_voucher"), 120_300);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "after_voucher"), 154_300);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "g3_excess"), 5_000);
    assert!(p.rows.iter().all(|r| r.program_text.is_empty()), "프로그램 열이 없는 양식");
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"));
    assert!(p.file_issues.iter().any(|i| i.level == "info" && i.message.contains("전체 시트 합계")), "{:?}", p.file_issues);
    // 자유수강권 시트의 합계 행은 읽지 않는다.
    assert!(!p.rows.iter().any(|r| r.student.as_ref().map(|s| s.name == "합계").unwrap_or(false)));
}

#[test]
fn refund_and_extra_keep_their_own_kinds_and_signs() {
    let p = parse(&book("refund_extra_s1.xlsx"), FormatCode::RefundExtraS1).unwrap();
    assert_eq!(p.rows.len(), 7);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "refund"), -114_900);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "extra_collect"), 50_850);
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"), "{:?}", p.rows.iter().flat_map(|r| r.issues.clone()).collect::<Vec<_>>());
    assert!(p.rows.iter().any(|r| r.program_text == "로봇과학A"));
}

#[test]
fn settle_s1_splits_collect_refund_extra_from_grouped_headers() {
    let p = parse(&book("settle_s1.xlsm"), FormatCode::SettleS1).unwrap();
    assert_eq!(p.data_row_count, 4);
    assert_eq!(p.rows.len(), 18);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "initial_collect"), 255_650);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "refund"), -90_000);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "extra_collect"), 50_850);
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"), "{:?}", p.rows.iter().flat_map(|r| r.issues.clone()).collect::<Vec<_>>());
}

#[test]
fn roster_s1_has_students_but_no_amounts() {
    let p = parse(&book("roster_s1.xlsx"), FormatCode::RosterS1).unwrap();
    assert_eq!(p.rows.len(), 4);
    assert!(p.rows.iter().all(|r| r.entry_kind.is_empty() && r.amount == 0));
    assert_eq!(p.rows[3].program_text, "바둑B");
}

#[test]
fn amount_cell_rules() {
    use super::{amount_of, Cell, CellValue};
    let c = |v| Cell { value: v, formula_without_value: false };
    assert_eq!(amount_of(&c(CellValue::Empty)).unwrap(), None);
    assert_eq!(amount_of(&c(CellValue::Int(0))).unwrap(), Some(0), "빈 칸과 0 은 다르다");
    assert_eq!(amount_of(&c(CellValue::Float(35000.0))).unwrap(), Some(35000));
    assert_eq!(amount_of(&c(CellValue::Text("1,234원".into()))).unwrap(), Some(1234));
    assert_eq!(amount_of(&c(CellValue::Text("-".into()))).unwrap(), None);
    assert!(amount_of(&c(CellValue::Float(3.5))).is_err());
    assert!(amount_of(&Cell { value: CellValue::Empty, formula_without_value: true }).is_err());
}

/// 실제 9월 파일 대조 — 개인정보가 있는 실제 파일은 저장소 밖에 있으므로 환경변수로만 돈다.
///   ASACCOUNTING_REAL_DIR="C:\...\방과후 회계 자동화" cargo test --lib real_ -- --ignored
#[test]
#[ignore]
fn real_september_files_match_expected_totals() {
    let dir = std::path::PathBuf::from(std::env::var("ASACCOUNTING_REAL_DIR").expect("ASACCOUNTING_REAL_DIR"));
    let open = |n: &str| Book::open(&dir.join(n)).unwrap();

    let b = open("2026학년도 9월 방과후프로그램 강사료 내역 양식.xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::InstructorPay);
    let p = parse(&b, FormatCode::InstructorPay).unwrap();
    assert_eq!(p.data_row_count, 26);
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 27_329_000);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text.starts_with("맞춤형지원")), 12_400_000);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "3학년 지원금"), 5_756_800);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "3학년 수익자"), 69_700);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "수익자징수"), 8_656_500);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "자유수강권"), 446_000);
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"));

    let b = open("2026학년도 방과후프로그램 9월 교재,재료비 지급업체.xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::VendorPay);
    let p = parse(&b, FormatCode::VendorPay).unwrap();
    assert_eq!(p.data_row_count, 21);
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 9_987_700);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "수익자징수금액"), 7_844_900);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "3학년 지원금"), 1_923_500);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "3학년 수익자"), 50_300);
    assert_eq!(sum_where(&p.rows, |r| r.fund_text == "자유수강권"), 169_000);
    let inherited: std::collections::BTreeSet<u32> = p.rows.iter().filter(|r| r.program_inherited).map(|r| r.row_no).collect();
    assert_eq!(inherited.len(), 2, "공란 부서명 3행 중 금액이 있는 2행은 병합 셀에서 이어받음: {inherited:?}");
    assert_eq!(p.excluded_row_count, 2, "0원 업체 행(공란 1 + 통기타) 은 원장에 넣을 것이 없다");
    assert!(p.rows.iter().all(|r| r.worst_level() == "ok"), "{:?}", p.rows.iter().filter(|r| r.worst_level() != "ok").map(|r| (r.row_no, r.issues.clone())).collect::<Vec<_>>());

    let b = open("수강생명단(방과후 통합 매니저 9월부터).xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::RosterManager);
    let p = parse(&b, FormatCode::RosterManager).unwrap();
    assert_eq!(p.data_row_count, 832);
    assert_eq!(p.excluded_row_count, 29);
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 24_784_150);
    assert_eq!(sum_where(&p.rows, |r| r.item_code == "instructor"), 14_572_000);
    assert_eq!(sum_where(&p.rows, |r| r.item_code == "material"), 7_143_750);

    let b = open("방과후 징수명단(기안용).xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::CollectInitial);
    let p = parse(&b, FormatCode::CollectInitial).unwrap();
    assert_eq!(p.rows.iter().map(|r| r.amount).sum::<i64>(), 25_734_600);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "beneficiary"), 16_957_250);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "free_voucher"), 633_600);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "after_voucher"), 8_019_100);
    assert_eq!(sum_where(&p.rows, |r| r.fund_code == "g3_excess"), 124_650);
    assert!(p.file_issues.iter().any(|i| i.message.contains("일치")), "{:?}", p.file_issues);

    let b = open("방과후 환불 및 추가징수 명단.xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::RefundExtraS1);
    let p = parse(&b, FormatCode::RefundExtraS1).unwrap();
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "refund"), -425_750);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "extra_collect"), 80_050);

    let b = open("방과후 수강생 명단2(환불, 추가징수 포함 1학기).xlsm");
    assert_eq!(detect(&b)[0].format, FormatCode::SettleS1);
    let p = parse(&b, FormatCode::SettleS1).unwrap();
    assert_eq!(p.data_row_count, 833);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "initial_collect"), 25_734_600);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "refund"), -425_750);
    assert_eq!(sum_where(&p.rows, |r| r.entry_kind == "extra_collect"), 80_050);

    let b = open("방과후 수강생 명단(1학기).xlsx");
    assert_eq!(detect(&b)[0].format, FormatCode::RosterS1);
    let p = parse(&b, FormatCode::RosterS1).unwrap();
    assert!(p.rows.len() > 800);
}
