//! 양식별 파서. 열 위치는 `docs/01-P0-설계.md` 2장의 실측에 따른다.
//! 머리글 이름으로 열을 찾으므로 열 순서가 바뀌어도 읽힌다.

use super::detect::FormatCode;
use super::parse::*;
use super::{norm, Book, Sheet};
use crate::domain::money;
use crate::error::AppResult;

fn is_total_marker(t: &str) -> bool {
    let n = norm(t);
    n == "합계" || n == "소계" || n == "총계"
}

/// 한 학생(행)의 항목별 금액을 행으로 펼친다. 돌려주는 값은 항목 합.
#[allow(clippy::too_many_arguments)]
fn emit_items(
    ctx: &mut Ctx,
    sheet: &Sheet,
    row: u32,
    base: &ParsedRow,
    item_cols: &[(u32, &str)],
    entry_kind: &str,
    flow: &str,
    negate: bool,
    row_issues: &mut Vec<Issue>,
) -> i64 {
    let mut sum = 0i64;
    let mut emitted: Vec<ParsedRow> = Vec::new();
    for (col, code) in item_cols {
        let Some(v) = read_amount(sheet, row, *col, row_issues) else { continue };
        sum = sum.saturating_add(v);
        if v == 0 {
            continue;
        }
        let mut r = base.clone();
        r.item_code = code.to_string();
        r.entry_kind = entry_kind.to_string();
        r.flow = flow.to_string();
        r.amount = if negate { -v } else { v };
        emitted.push(r);
    }
    for mut r in emitted {
        r.issues.extend(row_issues.iter().cloned());
        ctx.push(r);
    }
    sum
}

// ── E. 통합 매니저 수강생명단 ───────────────────────────────────────────────

pub fn roster_manager(book: &Book) -> AppResult<Parsed> {
    let sheet = book
        .sheets
        .iter()
        .find(|s| s.find_header_row(&["부서", "학년", "반", "번호", "이름", "지원유형", "강사료"], 6).is_some())
        .ok_or_else(|| crate::error::AppError::invalid("통합 매니저 수강생명단 머리글을 찾지 못했습니다."))?;
    let h = header_or_err(sheet, &["부서", "학년", "반", "번호", "이름", "지원유형", "강사료"])?;
    let c_dept = col_or_err(sheet, h, "부서")?;
    let c_grade = sheet.col_of(h, "학년");
    let c_class = sheet.col_of(h, "반");
    let c_no = sheet.col_of(h, "번호");
    let c_name = sheet.col_of(h, "이름");
    let c_type = col_or_err(sheet, h, "지원유형")?;
    let c_total = sheet.col_of(h, "합계");
    let c_status = sheet.col_of(h, "상태");
    let items: Vec<(u32, &str)> = ITEM_HEADERS
        .iter()
        .filter_map(|(hname, code)| sheet.col_of(h, hname).map(|c| (c, *code)))
        .collect();
    let mut ctx = Ctx::new();
    for row in (h + 1)..=sheet.max_row {
        let dept = sheet.text(row, c_dept);
        if dept.is_empty() {
            continue;
        }
        if is_total_marker(&dept) {
            continue;
        }
        ctx.keep_source(sheet, row);
        ctx.data_row_count += 1;
        let student = student_at(sheet, row, c_grade, c_class, c_no, c_name);
        let mut issues = student_issues(&student);
        let status = c_status.map(|c| sheet.text(row, c)).unwrap_or_default();
        let cancelled = status == "취소";
        let mut base = blank_row(&sheet.name, row);
        base.program_text = dept;
        base.fund_text = sheet.text(row, c_type);
        base.student = Some(student);
        if cancelled {
            ctx.excluded_row_count += 1;
            // 취소된 수강은 원장에 넣지 않는다. 원본 행에는 남긴다.
            continue;
        }
        if base.fund_text.is_empty() {
            issues.push(Issue::check("지원유형이 비어 있습니다."));
        }
        let sum = emit_items(&mut ctx, sheet, row, &base, &items, "initial_collect", "collection", false, &mut issues);
        if issues.iter().any(|i| i.level == "error") {
            // 금액을 못 읽은 행은 행 자체를 오류로 남긴다 (금액 없는 행으로 사라지지 않게).
            let mut r = base.clone();
            r.issues = issues.clone();
            ctx.push(r);
        }
        let mut tail = Vec::new();
        check_total(sheet, row, c_total, sum, &mut tail);
        if !tail.is_empty() {
            for r in ctx.rows.iter_mut().rev().take_while(|r| r.row_no == row) {
                r.issues.extend(tail.iter().cloned());
            }
        }
    }
    Ok(ctx.finish(FormatCode::RosterManager))
}

// ── F. 강사료 지급 내역 ─────────────────────────────────────────────────────

pub fn instructor_pay(book: &Book) -> AppResult<Parsed> {
    let sheet = book
        .sheets
        .iter()
        .find(|s| s.find_header_row(&["부서", "강사명", "최종강사료합계"], 6).is_some())
        .ok_or_else(|| crate::error::AppError::invalid("강사료 내역 머리글을 찾지 못했습니다."))?;
    let h = header_or_err(sheet, &["부서", "강사명", "최종강사료합계"])?;
    let c_dept = col_or_err(sheet, h, "부서")?;
    let c_name = col_or_err(sheet, h, "강사명")?;
    let c_total = col_or_err(sheet, h, "최종강사료합계")?;
    // 강사명과 합계 사이의 열이 재원 열이다.
    let fund_cols: Vec<(u32, String)> = ((c_name + 1)..c_total)
        .filter_map(|c| {
            let t = sheet.text(h, c);
            if t.is_empty() { None } else { Some((c, t)) }
        })
        .collect();
    let mut ctx = Ctx::new();
    let mut col_sums: Vec<i64> = vec![0; fund_cols.len()];
    let mut total_row: Option<u32> = None;
    for row in (h + 1)..=sheet.max_row {
        let dept = sheet.text(row, c_dept);
        let name = sheet.text(row, c_name);
        if is_total_marker(&name) || is_total_marker(&dept) {
            total_row = Some(row);
            ctx.keep_source(sheet, row);
            continue;
        }
        if dept.is_empty() && name.is_empty() {
            continue;
        }
        ctx.keep_source(sheet, row);
        ctx.data_row_count += 1;
        let mut issues = Vec::new();
        if dept.is_empty() {
            issues.push(Issue::check("부서(프로그램)명이 비어 있습니다."));
        }
        let mut base = blank_row(&sheet.name, row);
        base.program_text = dept;
        base.counterparty = name.trim().to_string();
        base.item_code = "instructor".into();
        let mut sum = 0i64;
        let mut emitted = Vec::new();
        for (i, (col, ftext)) in fund_cols.iter().enumerate() {
            let Some(v) = read_amount(sheet, row, *col, &mut issues) else { continue };
            sum += v;
            col_sums[i] += v;
            if v == 0 {
                continue;
            }
            let mut r = base.clone();
            r.fund_text = ftext.clone();
            r.entry_kind = "instructor_pay".into();
            r.flow = "expenditure".into();
            r.amount = v;
            emitted.push(r);
        }
        check_total(sheet, row, Some(c_total), sum, &mut issues);
        if emitted.is_empty() && issues.iter().any(|i| i.level == "error") {
            emitted.push(base.clone());
        }
        for mut r in emitted {
            r.issues.extend(issues.iter().cloned());
            ctx.push(r);
        }
    }
    if let Some(tr) = total_row {
        for (i, (col, ftext)) in fund_cols.iter().enumerate() {
            if let Ok(Some(t)) = super::amount_of(sheet.cell(tr, *col)) {
                if t != col_sums[i] {
                    ctx.file_issues.push(Issue::check(format!(
                        "'{}' 열의 합계 행({})과 읽은 행들의 합({})이 다릅니다.",
                        norm(ftext),
                        money::format_won(t),
                        money::format_won(col_sums[i])
                    )));
                }
            }
        }
        if ctx.file_issues.is_empty() {
            ctx.file_issues.push(Issue::info("합계 행과 읽은 행들의 합이 재원별로 모두 일치합니다."));
        }
    }
    Ok(ctx.finish(FormatCode::InstructorPay))
}

// ── G. 교재·재료비 지급업체 ────────────────────────────────────────────────

pub fn vendor_pay(book: &Book) -> AppResult<Parsed> {
    let sheet = book
        .sheets
        .iter()
        .find(|s| s.find_header_row(&["부서명", "최종지급금액", "업체명"], 6).is_some())
        .ok_or_else(|| crate::error::AppError::invalid("교재·재료비 지급업체 머리글을 찾지 못했습니다."))?;
    let h = header_or_err(sheet, &["부서명", "최종지급금액", "업체명"])?;
    let c_dept = col_or_err(sheet, h, "부서명")?;
    let c_total = col_or_err(sheet, h, "최종지급금액")?;
    let c_vendor = col_or_err(sheet, h, "업체명")?;
    let fund_cols: Vec<(u32, String)> = ((c_dept + 1)..c_total)
        .filter_map(|c| {
            let t = sheet.text(h, c);
            if t.is_empty() { None } else { Some((c, t)) }
        })
        .collect();
    let mut ctx = Ctx::new();
    let mut col_sums: Vec<i64> = vec![0; fund_cols.len()];
    let mut prev_program = String::new();
    let mut total_row: Option<u32> = None;
    for row in (h + 1)..=sheet.max_row {
        let dept_raw = sheet.text(row, c_dept);
        let vendor = sheet.text(row, c_vendor);
        if is_total_marker(&dept_raw) || is_total_marker(&vendor) {
            total_row = Some(row);
            ctx.keep_source(sheet, row);
            break; // 합계 아래의 교재비/재료비 총액 칸은 자료 행이 아니다.
        }
        let has_amount = fund_cols.iter().any(|(c, _)| !sheet.cell(row, *c).is_empty())
            || !sheet.cell(row, c_total).is_empty();
        if dept_raw.is_empty() && vendor.is_empty() && !has_amount {
            continue;
        }
        ctx.keep_source(sheet, row);
        ctx.data_row_count += 1;
        let mut issues = Vec::new();
        let mut base = blank_row(&sheet.name, row);
        if dept_raw.is_empty() {
            // 병합 셀의 아래 칸이면 안전하게 이어받는다. 아니면 사용자 확인.
            if let Some((r1, c1)) = sheet.merged_origin(row, c_dept) {
                let origin = sheet.text(r1, c1);
                if !origin.is_empty() {
                    base.program_text = origin;
                    base.program_inherited = true;
                    issues.push(Issue::info("부서명 병합 셀에서 이어받았습니다."));
                }
            }
            if base.program_text.is_empty() {
                base.program_suggestion = prev_program.clone();
                if !vendor.is_empty() && !prev_program.is_empty() {
                    issues.push(Issue::check(format!(
                        "부서명이 비어 있습니다. 바로 위 '{prev_program}' 의 둘째 업체인지 확인해 주세요."
                    )));
                } else {
                    issues.push(Issue::check("부서명이 비어 있습니다."));
                }
            }
        } else {
            base.program_text = dept_raw.clone();
            prev_program = dept_raw.clone();
        }
        if vendor.is_empty() {
            issues.push(Issue::check("업체명이 비어 있습니다."));
        }
        base.counterparty = vendor;
        base.item_code = "textbook_material".into();
        let mut sum = 0i64;
        let mut emitted = Vec::new();
        for (i, (col, ftext)) in fund_cols.iter().enumerate() {
            let Some(v) = read_amount(sheet, row, *col, &mut issues) else { continue };
            sum += v;
            col_sums[i] += v;
            if v == 0 {
                continue;
            }
            let mut r = base.clone();
            r.fund_text = ftext.clone();
            r.entry_kind = "vendor_pay".into();
            r.flow = "expenditure".into();
            r.amount = v;
            emitted.push(r);
        }
        check_total(sheet, row, Some(c_total), sum, &mut issues);
        if emitted.is_empty() {
            // 0원 업체 행(예: 주언출판사)은 원장에 넣을 것이 없다. 참고로만 남긴다.
            if issues.iter().any(|i| i.level == "error") {
                emitted.push(base.clone());
            } else {
                ctx.excluded_row_count += 1;
            }
        }
        for mut r in emitted {
            r.issues.extend(issues.iter().cloned());
            ctx.push(r);
        }
    }
    if let Some(tr) = total_row {
        for (i, (col, ftext)) in fund_cols.iter().enumerate() {
            if let Ok(Some(t)) = super::amount_of(sheet.cell(tr, *col)) {
                if t != col_sums[i] {
                    ctx.file_issues.push(Issue::check(format!(
                        "'{}' 열의 합계 행({})과 읽은 행들의 합({})이 다릅니다.",
                        norm(ftext),
                        money::format_won(t),
                        money::format_won(col_sums[i])
                    )));
                }
            }
        }
        if ctx.file_issues.is_empty() {
            ctx.file_issues.push(Issue::info("합계 행과 읽은 행들의 합이 재원별로 모두 일치합니다."));
        }
    }
    Ok(ctx.finish(FormatCode::VendorPay))
}

// ── C. 징수명단(기안용) — 재원별 시트 ─────────────────────────────────────

pub fn collect_initial(book: &Book) -> AppResult<Parsed> {
    let mut ctx = Ctx::new();
    // (시트, 재원 코드, 항목 머리글 접두)
    let plans: [(&str, &[(&str, &str)]); 3] = [
        ("수익자", &[("", "beneficiary")]),
        ("자유수강권", &[("", "free_voucher")]),
        ("방과후이용권", &[("이용권 ", "after_voucher"), ("초과 ", "g3_excess")]),
    ];
    let mut fund_totals: [i64; 4] = [0; 4];
    let mut any_fund_sheet = false;
    for (sheet_name, groups) in plans {
        let Some(sheet) = book.sheet(sheet_name) else { continue };
        any_fund_sheet = true;
        let h = header_or_err(sheet, &["학년", "반", "번호", "이름"])?;
        let c_grade = sheet.col_of(h, "학년");
        let c_class = sheet.col_of(h, "반");
        let c_no = sheet.col_of(h, "번호");
        let c_name = sheet.col_of(h, "이름");
        for (prefix, fund_code) in groups.iter() {
            let items: Vec<(u32, &str)> = ITEM_HEADERS
                .iter()
                .filter_map(|(hname, code)| sheet.col_of(h, &format!("{prefix}{hname}")).map(|c| (c, *code)))
                .collect();
            if items.is_empty() {
                ctx.file_issues.push(Issue::check(format!("시트 '{sheet_name}' 에 '{prefix}강사료' 같은 항목 열이 없습니다.")));
                continue;
            }
            let total_col = sheet.col_of(h, &format!("{prefix}합계"));
            for row in (h + 1)..=sheet.max_row {
                let student = student_at(sheet, row, c_grade, c_class, c_no, c_name);
                if is_total_marker(&student.name) {
                    continue;
                }
                if student.is_blank() && !items.iter().any(|(c, _)| !sheet.cell(row, *c).is_empty()) {
                    continue;
                }
                if prefix.is_empty() || *prefix == "이용권 " {
                    ctx.keep_source(sheet, row);
                    ctx.data_row_count += 1;
                }
                let mut issues = student_issues(&student);
                let mut base = blank_row(&sheet.name, row);
                base.student = Some(student);
                base.fund_code = fund_code.to_string();
                let sum = emit_items(&mut ctx, sheet, row, &base, &items, "initial_collect", "collection", false, &mut issues);
                check_total(sheet, row, total_col, sum, &mut Vec::new());
                for (i, (c, _)) in items.iter().enumerate() {
                    if let Ok(Some(v)) = super::amount_of(sheet.cell(row, *c)) {
                        fund_totals[i.min(3)] += v;
                    }
                }
            }
        }
    }
    if !any_fund_sheet {
        return Err(crate::error::AppError::invalid(
            "재원별 시트(수익자·방과후이용권·자유수강권)가 없습니다. 전체 시트만으로는 재원을 알 수 없습니다.",
        ));
    }
    // 전체 시트는 합산하지 않고 대조만 한다.
    if let Some(all) = book.sheet("전체") {
        if let Some(h) = all.find_header_row(&["학년", "반", "번호", "이름"], 8) {
            let c_name = all.col_of(h, "이름");
            let items: Vec<(u32, usize)> = ITEM_HEADERS
                .iter()
                .enumerate()
                .filter_map(|(i, (hname, _))| all.col_of(h, hname).map(|c| (c, i)))
                .collect();
            let mut all_totals = [0i64; 4];
            for row in (h + 1)..=all.max_row {
                if c_name.map(|c| is_total_marker(&all.text(row, c))).unwrap_or(false) {
                    continue;
                }
                for (c, i) in &items {
                    if let Ok(Some(v)) = super::amount_of(all.cell(row, *c)) {
                        all_totals[*i] += v;
                    }
                }
            }
            let a: i64 = all_totals.iter().sum();
            let f: i64 = fund_totals.iter().sum();
            if a == f {
                ctx.file_issues.push(Issue::info(format!(
                    "전체 시트 합계({})와 재원별 시트 합계가 일치합니다. 전체 시트는 합산하지 않았습니다.",
                    money::format_won(a)
                )));
            } else {
                ctx.file_issues.push(Issue::check(format!(
                    "전체 시트 합계({})와 재원별 시트 합계({})가 다릅니다. 재원별 시트만 반영됩니다.",
                    money::format_won(a),
                    money::format_won(f)
                )));
            }
        }
    }
    ctx.file_issues.push(Issue::info("이 양식에는 프로그램 열이 없어 거래가 '미배분'으로 등록됩니다. 프로그램 연결은 프로그램별 수강생명단으로 합니다."));
    Ok(ctx.finish(FormatCode::CollectInitial))
}

// ── D. 환불 및 추가징수 명단 ──────────────────────────────────────────────

pub fn refund_extra_s1(book: &Book) -> AppResult<Parsed> {
    let mut ctx = Ctx::new();
    let plans = [("환불", "refund", true), ("추가징수", "extra_collect", false)];
    let mut found = false;
    for (sheet_name, kind, negate) in plans {
        let Some(sheet) = book.sheet(sheet_name) else { continue };
        found = true;
        let h = header_or_err(sheet, &["부서", "학년", "반", "번호", "이름", "강사료"])?;
        let c_dept = col_or_err(sheet, h, "부서")?;
        let c_grade = sheet.col_of(h, "학년");
        let c_class = sheet.col_of(h, "반");
        let c_no = sheet.col_of(h, "번호");
        let c_name = sheet.col_of(h, "이름");
        let c_total = sheet.col_of(h, "합계");
        let items: Vec<(u32, &str)> = ITEM_HEADERS
            .iter()
            .filter_map(|(hname, code)| sheet.col_of(h, hname).map(|c| (c, *code)))
            .collect();
        for row in (h + 1)..=sheet.max_row {
            let dept = sheet.text(row, c_dept);
            let student = student_at(sheet, row, c_grade, c_class, c_no, c_name);
            if is_total_marker(&student.name) || is_total_marker(&dept) {
                ctx.keep_source(sheet, row);
                continue;
            }
            if dept.is_empty() && student.is_blank() {
                continue;
            }
            ctx.keep_source(sheet, row);
            ctx.data_row_count += 1;
            let mut issues = student_issues(&student);
            if dept.is_empty() {
                issues.push(Issue::check("부서(프로그램)명이 비어 있습니다."));
            }
            let mut base = blank_row(&sheet.name, row);
            base.program_text = dept;
            base.student = Some(student);
            let sum = emit_items(&mut ctx, sheet, row, &base, &items, kind, "collection", negate, &mut issues);
            let mut tail = Vec::new();
            check_total(sheet, row, c_total, sum, &mut tail);
            for r in ctx.rows.iter_mut().rev().take_while(|r| r.row_no == row && r.sheet == sheet.name) {
                r.issues.extend(tail.iter().cloned());
            }
        }
    }
    if !found {
        return Err(crate::error::AppError::invalid("'환불' 또는 '추가징수' 시트가 없습니다."));
    }
    ctx.file_issues.push(Issue::info("이 양식에는 재원 열이 없어 재원이 '미확정'으로 등록됩니다."));
    Ok(ctx.finish(FormatCode::RefundExtraS1))
}

// ── B. 1학기 정산 명단2 — '부서별' 시트 ────────────────────────────────────

pub fn settle_s1(book: &Book) -> AppResult<Parsed> {
    let sheet = sheet_or_err(book, "부서별")?;
    let h = header_or_err(sheet, &["순번", "부서", "학년", "반", "번호", "이름", "강사료"])?;
    if h < 2 {
        return Err(crate::error::AppError::invalid("'부서별' 시트에 징수·환불·추가징수 묶음 머리글이 없습니다."));
    }
    let c_dept = col_or_err(sheet, h, "부서")?;
    let c_grade = sheet.col_of(h, "학년");
    let c_class = sheet.col_of(h, "반");
    let c_no = sheet.col_of(h, "번호");
    let c_name = sheet.col_of(h, "이름");
    // 묶음 머리글(윗줄)의 시작 열을 찾고, 그 열부터 항목 4개를 읽는다.
    let group_start = |label: &str| -> Option<u32> { (1..=sheet.max_col).find(|&c| norm(&sheet.text(h - 1, c)) == label) };
    let items_from = |start: u32| -> Vec<(u32, &str)> {
        ITEM_HEADERS
            .iter()
            .filter_map(|(hname, code)| {
                (start..=sheet.max_col.min(start + 3)).find(|&c| norm(&sheet.text(h, c)) == *hname).map(|c| (c, *code))
            })
            .collect()
    };
    let plans: Vec<(&str, &str, bool, Vec<(u32, &str)>)> = [("징수", "initial_collect", false), ("환불", "refund", true), ("추가징수", "extra_collect", false)]
        .into_iter()
        .filter_map(|(label, kind, negate)| group_start(label).map(|s| (label, kind, negate, items_from(s))))
        .collect();
    if plans.is_empty() {
        return Err(crate::error::AppError::invalid("'부서별' 시트에서 징수·환불·추가징수 열을 찾지 못했습니다."));
    }
    let total_items = group_start("합계").map(items_from).unwrap_or_default();
    let mut ctx = Ctx::new();
    for row in (h + 1)..=sheet.max_row {
        let dept = sheet.text(row, c_dept);
        if dept.is_empty() || is_total_marker(&dept) {
            continue;
        }
        ctx.keep_source(sheet, row);
        ctx.data_row_count += 1;
        let student = student_at(sheet, row, c_grade, c_class, c_no, c_name);
        let mut issues = student_issues(&student);
        let mut base = blank_row(&sheet.name, row);
        base.program_text = dept;
        base.student = Some(student);
        let mut net = [0i64; 4];
        for (_, kind, negate, items) in &plans {
            let mut item_issues = Vec::new();
            for (i, (col, code)) in items.iter().enumerate() {
                let Some(v) = read_amount(sheet, row, *col, &mut item_issues) else { continue };
                net[i] += if *negate { -v } else { v };
                if v == 0 {
                    continue;
                }
                let mut r = base.clone();
                r.item_code = code.to_string();
                r.entry_kind = kind.to_string();
                r.flow = "collection".into();
                r.amount = if *negate { -v } else { v };
                r.issues = issues.clone();
                ctx.push(r);
            }
            issues.extend(item_issues);
        }
        // 합계 묶음 = 징수 − 환불 + 추가징수 인지 본다.
        for (i, (col, _)) in total_items.iter().enumerate() {
            if let Ok(Some(t)) = super::amount_of(sheet.cell(row, *col)) {
                if t != net[i] {
                    issues.push(Issue::check(format!(
                        "합계 묶음 {}열({})이 징수−환불+추가징수({})와 다릅니다.",
                        col_letter(*col),
                        money::format_won(t),
                        money::format_won(net[i])
                    )));
                }
            }
        }
        if issues.iter().any(|i| i.level != "info") {
            for r in ctx.rows.iter_mut().rev().take_while(|r| r.row_no == row) {
                for is in &issues {
                    if !r.issues.contains(is) {
                        r.issues.push(is.clone());
                    }
                }
            }
        }
    }
    ctx.file_issues.push(Issue::info("이 양식에는 재원 열이 없어 재원이 '미확정'으로 등록됩니다. 징수·환불·추가징수는 각각 다른 거래 유형으로 남깁니다."));
    Ok(ctx.finish(FormatCode::SettleS1))
}

// ── A. 1학기 수강생 명단 (금액 없음) ───────────────────────────────────────

pub fn roster_s1(book: &Book) -> AppResult<Parsed> {
    let sheet = book
        .sheets
        .iter()
        .find(|s| s.find_header_row(&["부서", "학년", "반", "이름"], 6).is_some())
        .ok_or_else(|| crate::error::AppError::invalid("수강생 명단 머리글(부서·학년·반·이름)을 찾지 못했습니다."))?;
    let h = header_or_err(sheet, &["부서", "학년", "반", "이름"])?;
    let c_dept = col_or_err(sheet, h, "부서")?;
    let c_grade = sheet.col_of(h, "학년");
    let c_class = sheet.col_of(h, "반");
    let c_no = sheet.col_of(h, "번호");
    let c_name = sheet.col_of(h, "이름");
    let mut ctx = Ctx::new();
    for row in (h + 1)..=sheet.max_row {
        let dept = sheet.text(row, c_dept);
        if dept.is_empty() || is_total_marker(&dept) {
            continue;
        }
        ctx.keep_source(sheet, row);
        ctx.data_row_count += 1;
        let student = student_at(sheet, row, c_grade, c_class, c_no, c_name);
        let issues = student_issues(&student);
        let mut r = blank_row(&sheet.name, row);
        r.program_text = dept;
        r.student = Some(student);
        r.issues = issues;
        ctx.push(r);
    }
    ctx.file_issues.push(Issue::info("이 양식에는 금액이 없습니다. 프로그램·학생 연결 자료로만 저장됩니다."));
    Ok(ctx.finish(FormatCode::RosterS1))
}
