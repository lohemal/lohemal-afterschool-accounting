//! 양식 인식 — 파일 이름이 아니라 **시트 구조와 머리글**로 판단한다.

use serde::{Deserialize, Serialize};

use super::{norm, Book};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatCode {
    /// 1학기 수강생 명단 — 부서·학년·반·이름 (금액 없음)
    RosterS1,
    /// 1학기 정산 명단2 — 요약/부서별/학생별, 징수·환불·추가징수
    SettleS1,
    /// 징수명단(기안용) — 전체/수익자/방과후이용권/자유수강권
    CollectInitial,
    /// 환불 및 추가징수 명단 — 환불/추가징수
    RefundExtraS1,
    /// 방과후 통합 매니저 수강생명단
    RosterManager,
    /// 강사료 지급 내역
    InstructorPay,
    /// 교재·재료비 지급업체
    VendorPay,
}

impl FormatCode {
    pub const ALL: [FormatCode; 7] = [
        FormatCode::CollectInitial,
        FormatCode::RosterManager,
        FormatCode::SettleS1,
        FormatCode::RefundExtraS1,
        FormatCode::InstructorPay,
        FormatCode::VendorPay,
        FormatCode::RosterS1,
    ];

    pub fn code(self) -> &'static str {
        match self {
            FormatCode::RosterS1 => "roster_s1",
            FormatCode::SettleS1 => "settle_s1",
            FormatCode::CollectInitial => "collect_initial",
            FormatCode::RefundExtraS1 => "refund_extra_s1",
            FormatCode::RosterManager => "roster_manager",
            FormatCode::InstructorPay => "instructor_pay",
            FormatCode::VendorPay => "vendor_pay",
        }
    }

    pub fn from_code(s: &str) -> Option<FormatCode> {
        FormatCode::ALL.into_iter().find(|f| f.code() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            FormatCode::RosterS1 => "1학기 수강생 명단",
            FormatCode::SettleS1 => "1학기 정산 명단(징수·환불·추가징수)",
            FormatCode::CollectInitial => "징수명단(기안용)",
            FormatCode::RefundExtraS1 => "환불 및 추가징수 명단",
            FormatCode::RosterManager => "통합 매니저 수강생명단",
            FormatCode::InstructorPay => "강사료 지급 내역",
            FormatCode::VendorPay => "교재·재료비 지급업체",
        }
    }

    /// 이 양식으로 올릴 수 있는 자료 유형 (첫 번째가 기본값).
    pub fn allowed_kinds(self) -> &'static [&'static str] {
        match self {
            FormatCode::CollectInitial => &["initial_collect", "snapshot"],
            FormatCode::RosterManager => &["initial_collect", "snapshot", "roster_link"],
            FormatCode::SettleS1 => &["roster_link", "initial_collect", "snapshot"],
            FormatCode::RefundExtraS1 => &["extra_refund", "snapshot"],
            FormatCode::InstructorPay => &["instructor_pay"],
            FormatCode::VendorPay => &["vendor_pay"],
            FormatCode::RosterS1 => &["roster_link", "snapshot"],
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    pub format: FormatCode,
    pub code: String,
    pub label: String,
    /// 인식에 쓴 시트와 머리글 행
    pub sheet: String,
    pub header_row: u32,
    /// 100 = 확실, 그 아래는 사용자 확인 권장
    pub confidence: u8,
}

fn has_sheet_with(book: &Book, sheet: &str, needles: &[&str]) -> Option<(String, u32)> {
    let s = book.sheet(sheet)?;
    let r = s.find_header_row(needles, 6)?;
    Some((s.name.clone(), r))
}

fn any_sheet_with(book: &Book, needles: &[&str]) -> Option<(String, u32)> {
    book.sheets
        .iter()
        .find_map(|s| s.find_header_row(needles, 6).map(|r| (s.name.clone(), r)))
}

/// 가능한 양식을 확신 순으로 돌려준다. 비어 있으면 알 수 없는 양식.
pub fn detect(book: &Book) -> Vec<Detected> {
    let mut out: Vec<Detected> = Vec::new();
    let push = |out: &mut Vec<Detected>, format: FormatCode, sheet: String, header_row: u32, confidence: u8| {
        out.push(Detected {
            format,
            code: format.code().into(),
            label: format.label().into(),
            sheet,
            header_row,
            confidence,
        });
    };

    // 통합 매니저 명단: 부서·학년·반·번호·이름·지원유형·강사료·…·상태
    if let Some((s, r)) = any_sheet_with(book, &["부서", "학년", "반", "번호", "이름", "지원유형", "강사료", "상태"]) {
        push(&mut out, FormatCode::RosterManager, s, r, 100);
    }
    // 강사료: 부서·강사명·최종강사료합계
    if let Some((s, r)) = any_sheet_with(book, &["부서", "강사명", "최종강사료합계"]) {
        push(&mut out, FormatCode::InstructorPay, s, r, 100);
    }
    // 교재·재료비: 부서명·최종지급금액·업체명
    if let Some((s, r)) = any_sheet_with(book, &["부서명", "최종지급금액", "업체명"]) {
        push(&mut out, FormatCode::VendorPay, s, r, 100);
    }
    // 징수명단(기안용): 시트 '전체' + 학년·반·번호·이름·강사료·합계, 재원 시트가 더 있으면 확실
    if let Some((s, r)) = has_sheet_with(book, "전체", &["학년", "반", "번호", "이름", "강사료", "수용비", "교재비", "재료비", "합계"]) {
        let fund_sheets = ["수익자", "방과후이용권", "자유수강권"]
            .iter()
            .filter(|n| book.sheet(n).is_some())
            .count();
        push(&mut out, FormatCode::CollectInitial, s, r, if fund_sheets >= 2 { 100 } else { 70 });
    }
    // 환불·추가징수: 시트 '환불' 또는 '추가징수' + 순번·부서·학년·반·번호·이름·강사료
    if let Some((s, r)) = ["환불", "추가징수"]
        .iter()
        .find_map(|n| has_sheet_with(book, n, &["순번", "부서", "학년", "반", "번호", "이름", "강사료", "합계"]))
    {
        push(&mut out, FormatCode::RefundExtraS1, s, r, 100);
    }
    // 1학기 정산 명단2: 시트 '부서별' 2행 머리글 (순번·부서·학년·반·번호·이름·강사료…)
    if let Some((s, r)) = has_sheet_with(book, "부서별", &["순번", "부서", "학년", "반", "번호", "이름", "강사료", "수용비", "교재비", "재료비"]) {
        let sh = book.sheet("부서별").unwrap();
        let has_groups = r >= 2
            && ["징수", "환불", "추가징수"]
                .iter()
                .all(|g| (1..=sh.max_col).any(|c| norm(&sh.text(r - 1, c)) == *g));
        push(&mut out, FormatCode::SettleS1, s, r, if has_groups { 100 } else { 70 });
    }
    // 1학기 수강생 명단: 부서·학년·반·이름 (금액 머리글 없음) — 가장 느슨해서 맨 뒤
    if out.is_empty() {
        if let Some((s, r)) = any_sheet_with(book, &["부서", "학년", "반", "이름"]) {
            let sh = book.sheet(&s).unwrap();
            let has_money = ["강사료", "수용비", "교재비"].iter().any(|n| sh.col_of(r, n).is_some());
            if !has_money {
                push(&mut out, FormatCode::RosterS1, s, r, 90);
            }
        }
    }
    out.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    out
}

/// 제목 글자('2026학년도 9월 …')에서 귀속월 후보를 찾는다.
pub fn suggest_month(book: &Book) -> Option<u32> {
    for s in &book.sheets {
        for r in 1..=s.max_row.min(3) {
            for c in 1..=s.max_col.min(12) {
                let t = s.text(r, c);
                if let Some(m) = month_in(&t) {
                    return Some(m);
                }
            }
        }
    }
    None
}

fn month_in(t: &str) -> Option<u32> {
    let chars: Vec<char> = t.chars().collect();
    for (i, ch) in chars.iter().enumerate() {
        if *ch == '월' && i > 0 {
            let mut j = i;
            let mut digits = String::new();
            while j > 0 && chars[j - 1].is_ascii_digit() && digits.len() < 2 {
                digits.insert(0, chars[j - 1]);
                j -= 1;
            }
            if let Ok(m) = digits.parse::<u32>() {
                if (1..=12).contains(&m) {
                    return Some(m);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_from_title() {
        assert_eq!(month_in("2026학년도 9월  강사료 내역"), Some(9));
        assert_eq!(month_in("2026학년도 12월 -교재,재료비"), Some(12));
        assert_eq!(month_in("2026학년도 강사료"), None);
        assert_eq!(month_in("13월"), None);
    }

    #[test]
    fn format_codes_round_trip() {
        for f in FormatCode::ALL {
            assert_eq!(FormatCode::from_code(f.code()), Some(f));
            assert!(!f.allowed_kinds().is_empty());
        }
    }
}
