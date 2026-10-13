//! 회계 자료 무결성 검사 — 원장·정정·환불·자유수강권·원본 연결·감사 이력을 서로 연결된 상태로 본다.
//! **아무것도 고치지 않는다.** 등급: ok(정상) · check(확인 필요) · error(오류). 항목마다 원인·확인 방법·수정 안내를 붙인다.

use std::collections::BTreeMap;
use std::path::Path;

use rusqlite::Connection;

use crate::domain::money::format_won as w;
use crate::error::AppResult;
use crate::model::*;
use crate::repo::{voucher, year};

pub fn level_label(l: &str) -> &'static str {
    match l {
        "ok" => "정상",
        "check" => "확인 필요",
        "error" => "오류",
        _ => "알 수 없음",
    }
}

struct Acc {
    items: Vec<IntegrityItem>,
    checks: Vec<IntegrityCheck>,
}

impl Acc {
    fn item(&mut self, code: &str, level: &str, title: &str, detail: String, student: String, program: String, month: String, entry_ids: Vec<i64>, cause: &str, how: &str, fix: &str) {
        self.items.push(IntegrityItem { code: code.into(), level: level.into(), title: title.into(), detail, student, program, month, entry_ids, cause: cause.into(), how_to_check: how.into(), fix_hint: fix.into() });
    }
    fn close(&mut self, code: &str, title: &str, level_if_any: &str) {
        let n = self.items.iter().filter(|i| i.code == code).count() as i64;
        self.checks.push(IntegrityCheck { code: code.into(), title: title.into(), level: if n == 0 { "ok".into() } else { level_if_any.into() }, count: n });
    }
}

const FIX_NO_AUTO: &str = "프로그램은 원장·감사 이력을 자동으로 고치지 않습니다. 원본 회계자료로 확인한 뒤 회계 정정(취소·재등록)으로 바로잡아 주세요.";

fn student_name(conn: &Connection, id: Option<i64>) -> String {
    id.and_then(|i| conn.query_row("SELECT grade || '학년 ' || class_name || '반 ' || student_no || '번 ' || name FROM student WHERE id = ?1", [i], |r| r.get::<_, String>(0)).ok()).unwrap_or_default()
}

fn program_name(conn: &Connection, id: Option<i64>) -> String {
    id.and_then(|i| conn.query_row("SELECT name FROM program WHERE id = ?1", [i], |r| r.get::<_, String>(0)).ok()).unwrap_or_default()
}

fn month_label(conn: &Connection, id: i64) -> String {
    year::month_label_of(conn, id).unwrap_or_default()
}

pub fn run(conn: &Connection, data_dir: &Path, school_year_id: i64) -> AppResult<IntegrityReport> {
    let mut a = Acc { items: vec![], checks: vec![] };
    let yid = school_year_id;

    // 1. SQLite 무결성
    let ic: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if ic != "ok" {
        a.item("sqlite", "error", "SQLite 무결성", ic, String::new(), String::new(), String::new(), vec![], "자료 파일 손상", "백업 목록에서 최근 검증된 백업을 확인", "복원 화면에서 검증된 백업으로 전체 복원");
    }
    a.close("sqlite", "SQLite 무결성 검사", "error");
    // 2. 외래 키
    {
        let mut st = conn.prepare("PRAGMA foreign_key_check")?;
        let rows: Vec<(String, i64, String)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        for (t, rowid, parent) in rows.into_iter().take(50) {
            a.item("foreign_keys", "error", "외래 키 위반", format!("표 {t} 행 {rowid} 이(가) {parent} 를 가리키지만 대상이 없습니다."), String::new(), String::new(), String::new(), vec![], "자료 연결 손상", "해당 표의 행을 확인", FIX_NO_AUTO);
        }
    }
    a.close("foreign_keys", "외래 키 검사", "error");
    // 3·6. 재원경정 쌍: 2건 · 합 0
    {
        let mut st = conn.prepare("SELECT transfer_group_id, count(*), sum(amount), min(id), max(id), fiscal_month_id, program_id, student_id FROM ledger_entry WHERE school_year_id = ?1 AND flow = 'transfer' AND state = 'active' GROUP BY transfer_group_id")?;
        let rows: Vec<(i64, i64, i64, i64, i64, i64, Option<i64>, Option<i64>)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?.collect::<Result<_, _>>()?;
        for (g, n, sum, lo, hi, m, p, s) in rows {
            if n != 2 || sum != 0 {
                a.item("transfer_pairs", "error", "재원경정 쌍 불일치", format!("재원 이동 묶음 #{g}: 거래 {n}건, 합 {}원 (2건·합 0 이어야 합니다).", w(sum)), student_name(conn, s), program_name(conn, p), month_label(conn, m), vec![lo, hi], "쌍 중 한쪽만 취소되었거나 원장이 손상됨", "회계 정정 → 정정 이력에서 해당 경정의 원장 거래 확인", FIX_NO_AUTO);
            }
        }
    }
    a.close("transfer_pairs", "재원경정 음수·양수 합계 0", "error");
    // 7. 재원경정 원본 연결 (살아 있는 정정의 원장 거래가 취소됨)
    {
        let mut st = conn.prepare(
            "SELECT a.id, a.kind, a.fiscal_month_id, a.program_id, a.student_id, e.id, e.state FROM adjustment a JOIN adjustment_entry ae ON ae.adjustment_id = a.id JOIN ledger_entry e ON e.id = ae.entry_id
              WHERE a.school_year_id = ?1 AND a.status <> 'cancelled' AND e.state <> 'active'",
        )?;
        let rows: Vec<(i64, String, i64, i64, Option<i64>, i64, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?.collect::<Result<_, _>>()?;
        for (aid, kind, m, p, s, eid, state) in rows {
            a.item("adjustment_links", "error", "정정 원장 연결 손상", format!("{} #{aid} 은 살아 있는데 원장 거래 #{eid} 는 '{state}' 입니다.", crate::repo::adjustment::kind_label(&kind)), student_name(conn, s), program_name(conn, Some(p)), month_label(conn, m), vec![eid], "정정 묶음을 거치지 않고 원장 거래가 취소됨", "정정 이력과 변경 이력에서 취소 경위 확인", FIX_NO_AUTO);
        }
    }
    a.close("adjustment_links", "재원경정·정정 원본 연결", "error");
    // 9. 반환 이행은 원장 거래가 없어야 한다
    {
        let mut st = conn.prepare("SELECT a.id, a.fiscal_month_id, a.program_id, a.student_id, ae.entry_id FROM adjustment a JOIN adjustment_entry ae ON ae.adjustment_id = a.id WHERE a.school_year_id = ?1 AND a.kind = 'refund_return'")?;
        let rows: Vec<(i64, i64, i64, Option<i64>, i64)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        for (aid, m, p, s, eid) in rows {
            a.item("refund_return_ledger", "error", "반환 이행 중복 차감", format!("반환 이행 #{aid} 에 원장 거래 #{eid} 가 연결되어 있습니다 (경정이 이미 차감했으므로 중복).") , student_name(conn, s), program_name(conn, Some(p)), month_label(conn, m), vec![eid], "반환 이행이 원장에 잘못 기록됨", "정정 이력에서 반환 이행 확인", FIX_NO_AUTO);
        }
    }
    a.close("refund_return_ledger", "반환 이행의 원장 중복 차감 없음", "error");
    // 8. 환불 허용 범위: 재원별 환불 합 ≤ 징수 합 · 반환 이행 누계 ≤ 경정 금액
    {
        let mut st = conn.prepare(
            "SELECT fiscal_month_id, program_id, student_id, fund_type_id, amount_item_id,
                    sum(CASE WHEN entry_kind IN ('initial_collect', 'extra_collect') THEN amount ELSE 0 END),
                    sum(CASE WHEN entry_kind = 'refund' THEN -amount ELSE 0 END), group_concat(id)
               FROM ledger_entry WHERE school_year_id = ?1 AND state = 'active' AND flow = 'collection' AND program_id IS NOT NULL
              GROUP BY fiscal_month_id, program_id, student_id, fund_type_id, amount_item_id HAVING sum(CASE WHEN entry_kind = 'refund' THEN -amount ELSE 0 END) > sum(CASE WHEN entry_kind IN ('initial_collect', 'extra_collect') THEN amount ELSE 0 END)",
        )?;
        let rows: Vec<(i64, i64, Option<i64>, Option<i64>, i64, i64, i64, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?)))?.collect::<Result<_, _>>()?;
        for (m, p, s, _f, _i, col, ref_, ids) in rows {
            let entry_ids: Vec<i64> = ids.split(',').filter_map(|x| x.parse().ok()).collect();
            a.item("refund_cap", "error", "환불 허용 금액 초과", format!("징수 {}원보다 환불 {}원이 큽니다.", w(col), w(ref_)), student_name(conn, s), program_name(conn, Some(p)), month_label(conn, m), entry_ids, "재원경정 뒤 환불이 등록되었거나 원본 징수가 취소됨", "차액 상세에서 징수·환불 거래 확인", FIX_NO_AUTO);
        }
        let mut st2 = conn.prepare(
            "SELECT t.id, t.amount, coalesce(sum(r.amount), 0), t.fiscal_month_id, t.program_id, t.student_id FROM adjustment t LEFT JOIN adjustment r ON r.related_adjustment_id = t.id AND r.kind = 'refund_return' AND r.status <> 'cancelled'
              WHERE t.school_year_id = ?1 AND t.kind = 'fund_transfer' GROUP BY t.id HAVING coalesce(sum(r.amount), 0) > t.amount",
        )?;
        let rows: Vec<(i64, i64, i64, i64, i64, Option<i64>)> = st2.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        for (tid, amt, ret, m, p, s) in rows {
            a.item("refund_cap", "error", "반환 이행 누계 초과", format!("재원경정 #{tid} 금액 {}원보다 반환 누계 {}원이 큽니다.", w(amt), w(ret)), student_name(conn, s), program_name(conn, Some(p)), month_label(conn, m), vec![], "환불이 중복 등록됨", "자유수강권 관리 → 환불 처리에서 내역 확인", "중복 환불을 취소(사유 기록)해 주세요.");
        }
    }
    a.close("refund_cap", "환불 금액 허용 범위", "error");
    // 4. 원본 파일 연결
    {
        let mut st = conn.prepare("SELECT e.id, e.fiscal_month_id, e.program_id, u.status, u.file_name FROM ledger_entry e JOIN upload_file u ON u.id = e.upload_file_id WHERE e.school_year_id = ?1 AND e.state = 'active' AND u.status IN ('cancelled', 'replaced')")?;
        let rows: Vec<(i64, i64, Option<i64>, String, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        for (eid, m, p, status, file) in rows.into_iter().take(100) {
            a.item("upload_links", "error", "원본 파일 연결 손상", format!("거래 #{eid} 의 원본 파일 '{file}' 은 '{}' 인데 거래가 살아 있습니다.", crate::repo::upload::status_label(&status)), String::new(), program_name(conn, p), month_label(conn, m), vec![eid], "파일 취소·교체 때 거래가 함께 접히지 않음", "자료 업로드 → 파일 상세에서 거래 확인", FIX_NO_AUTO);
        }
        let dir = crate::repo::upload::uploads_dir(data_dir);
        let mut st = conn.prepare("SELECT id, file_name, stored_name FROM upload_file WHERE school_year_id = ?1 AND status NOT IN ('cancelled', 'replaced') AND stored_name <> ''")?;
        let rows: Vec<(i64, String, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        for (id, file, stored) in rows {
            if !dir.join(&stored).is_file() {
                a.item("upload_files", "check", "원본 파일 없음", format!("업로드 #{id} '{file}' 의 보관 원본이 자료 폴더에 없습니다."), String::new(), String::new(), String::new(), vec![], "보관 폴더에서 파일이 지워졌거나 백업에 원본이 없었음", "자료 폴더 uploads/ 확인, 원본 포함 백업에서 복원", "원본 파일을 다시 업로드(교체)하면 보관본이 다시 생깁니다.");
            }
        }
    }
    a.close("upload_links", "원본 파일 연결", "error");
    a.close("upload_files", "보관 원본 파일 존재", "check");
    // 5. 학생별 배분 금액
    {
        let mut st = conn.prepare("SELECT e.id, e.amount, sum(a.amount), e.fiscal_month_id, e.student_id FROM allocation a JOIN ledger_entry e ON e.id = a.entry_id WHERE e.school_year_id = ?1 AND a.status = 'active' AND e.state = 'active' GROUP BY e.id HAVING abs(sum(a.amount)) > abs(e.amount) OR sign(sum(a.amount)) <> sign(e.amount)")?;
        let rows: Vec<(i64, i64, i64, i64, Option<i64>)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        for (eid, amt, alloc, m, s) in rows {
            a.item("allocation", "error", "배분 금액 불일치", format!("거래 #{eid} 금액 {}원인데 배분 합이 {}원입니다.", w(amt), w(alloc)), student_name(conn, s), String::new(), month_label(conn, m), vec![eid], "배분이 거래 금액을 넘음", "월별 정산 → 징수액 배분에서 확인", FIX_NO_AUTO);
        }
        let mut st = conn.prepare("SELECT e.id, e.amount, coalesce((SELECT sum(x.amount) FROM allocation x WHERE x.entry_id = e.id AND x.status = 'active'), 0), e.fiscal_month_id, e.student_id FROM ledger_entry e WHERE e.school_year_id = ?1 AND e.state = 'active' AND e.stage = 'actual' AND e.flow = 'collection' AND e.program_id IS NULL")?;
        let rows: Vec<(i64, i64, i64, i64, Option<i64>)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.collect::<Result<_, _>>()?;
        for (eid, amt, alloc, m, s) in rows.into_iter().filter(|x| x.1 != x.2).take(100) {
            a.item("unallocated", "check", "배분 미확정 징수", format!("거래 #{eid} {}원 중 {}원만 프로그램에 배분되었습니다.", w(amt), w(alloc)), student_name(conn, s), String::new(), month_label(conn, m), vec![eid], "명단 연결 자료 부족", "월별 정산 → 징수액 배분", "배분을 확정해야 프로그램별 대조와 자유수강권 계산에 들어갑니다.");
        }
    }
    a.close("allocation", "학생별 배분 금액 일치", "error");
    a.close("unallocated", "배분 미확정 징수", "check");
    // 프로그램·재원 매핑 미확정
    {
        let n: i64 = conn.query_row("SELECT count(*) FROM ledger_entry WHERE school_year_id = ?1 AND state = 'active' AND fund_type_id IS NULL", [yid], |r| r.get(0))?;
        if n > 0 {
            a.item("unmapped_fund", "check", "재원 미확정 거래", format!("재원이 확정되지 않은 거래 {n}건이 있습니다."), String::new(), String::new(), String::new(), vec![], "업로드 때 재원 표기를 매핑하지 못함", "설정 → 재원 별칭, 자료 업로드 → 파일 상세", "재원 별칭을 등록하고 파일을 교체하면 재원이 확정됩니다.");
        }
    }
    a.close("unmapped_fund", "재원·프로그램 매핑", "check");
    // 10·11. 자유수강권 한도·기간 · 전입생 · 합산 중복
    {
        let usages = voucher::usage_all(conn, yid)?;
        for u in &usages {
            if u.over_total > 0 {
                let ids: Vec<i64> = vec![];
                a.item("voucher_limit", "check", "자유수강권 한도 초과 사용", format!("계산 지원액보다 실제 사용액이 {}원 많습니다.", w(u.over_total)), u.student.clone(), String::new(), String::new(), ids, "소급 경정 뒤 원본 징수가 취소되었거나 정책이 바뀜", "자유수강권 관리 → 사용액 현황", "자동으로 되돌리지 않습니다. 원본 회계자료로 확인한 뒤 회계 정정으로 바로잡아 주세요.");
            }
            for r in u.rows.iter().filter(|r| r.state == "over" && !r.in_window) {
                a.item("voucher_window", "check", "지원 기간 밖 자유수강권 사용", format!("{} {} 에 자유수강권 {}원이 있으나 적용 기간 밖입니다.", r.month_label, r.program_name, w(r.free_actual)), u.student.clone(), r.program_name.clone(), r.month_label.clone(), vec![], "적용 시작일 이전 징수 또는 대상자 기간 오류", "대상자 관리의 적용 시작·종료일 확인", "기간이 맞다면 원본 회계자료와 대조해 주세요.");
            }
            for r in u.rows.iter().filter(|r| r.state == "dup_check") {
                a.item("sum_item_dup", "check", "합산 항목 중복 여부 확인 필요", format!("{} {}: 교재·재료비(합산)과 개별 교재비/재료비가 함께 있습니다 ({}원).", r.month_label, r.program_name, w(r.tuition)), u.student.clone(), r.program_name.clone(), r.month_label.clone(), vec![], "합산 금액이 개별 항목의 합계 표시일 수 있음", "원본 파일의 행 구조 확인", "합산이 표시용이면 지원 대상 항목에서 '교재·재료비(합산)'을 빼거나 해당 파일을 교체해 주세요. 원장은 바꾸지 않습니다.");
            }
            if u.blocked.iter().any(|b| b.contains("이전 학교")) {
                a.item("voucher_prior", "check", "전입생 이전 학교 사용액 미확인", "이전 학교 사용액이 확인되지 않아 사용액을 계산하지 않습니다.".into(), u.student.clone(), String::new(), String::new(), vec![], "전입 자료 미도착", "자유수강권 관리 → 전입생 관리", "이전 학교 자료로 금액을 확인해 입력해 주세요.");
            }
            if u.voucher_total > 0 && u.voucher_target_status != "eligible" {
                a.item("av_unconfirmed", "check", "방과후이용권 실적 있으나 대상 미확인", format!("이용권 사용액 {}원이 있지만 대상 여부가 '{}' 입니다.", w(u.voucher_total), u.voucher_target_status_label), u.student.clone(), String::new(), String::new(), vec![], "대상 확인 기록이 없음", "자유수강권 관리 → 이용권 대상", "대상 근거를 확인해 등록해 주세요. 실적만으로 자동 확정하지 않습니다.");
            }
        }
    }
    a.close("voucher_limit", "자유수강권 지원 한도", "check");
    a.close("voucher_window", "지원 대상 기간 외 지원", "check");
    a.close("sum_item_dup", "합산 항목 중복 여부", "check");
    a.close("voucher_prior", "전입생 이전 학교 사용액", "check");
    a.close("av_unconfirmed", "방과후이용권 대상 확인", "check");
    // 수납 근거
    {
        let mut st = conn.prepare("SELECT a.id, a.fiscal_month_id, a.program_id, a.student_id, coalesce(v.collected_confirmed, 0) FROM adjustment a LEFT JOIN voucher_refund v ON v.adjustment_id = a.id WHERE a.school_year_id = ?1 AND a.kind = 'refund_return' AND a.status = 'done' AND coalesce(v.collected_confirmed, 0) = 0")?;
        let rows: Vec<(i64, i64, i64, Option<i64>)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        for (aid, m, p, s) in rows {
            a.item("refund_evidence", "check", "실제 수납 근거 미연결", format!("반환 이행 #{aid} 이 완료 상태인데 수납 확인 근거가 없습니다."), student_name(conn, s), program_name(conn, Some(p)), month_label(conn, m), vec![], "회계 정정 화면에서 직접 완료 처리됨", "정정 이력과 수납 자료 대조", "근거가 있으면 자유수강권 → 환불 처리의 완료 처리로 근거를 남겨 주세요.");
        }
    }
    a.close("refund_evidence", "환불 수납 근거", "check");
    // 12. 월 확정 상태와 변경
    {
        for m in year::months(conn, yid)? {
            if m.changed_after_close {
                a.item("closed_month", "check", "확정 뒤 변경된 달", format!("{} 은 확정 뒤 새 거래가 들어왔습니다.", m.label), String::new(), String::new(), format!("{} ({})", m.label, m.ym), vec![], "확정 → 재개 → 등록 뒤 재확정하지 않음", "월별 정산의 확정 이력", "내용을 확인한 뒤 다시 확정해 주세요.");
            }
            if m.status == "closed" {
                let n: i64 = conn.query_row(
                    "SELECT count(*) FROM adjustment_status_log l JOIN adjustment a ON a.id = l.adjustment_id WHERE a.fiscal_month_id = ?1 AND l.at > coalesce((SELECT max(c.created_at) FROM month_close c WHERE c.fiscal_month_id = ?1 AND c.action = 'close'), '')",
                    [m.id],
                    |r| r.get(0),
                )?;
                if n > 0 {
                    a.item("closed_month", "check", "확정 뒤 정정 상태 변경", format!("{} 확정 뒤 정정 상태 변경 {n}건이 있습니다.", m.label), String::new(), String::new(), format!("{} ({})", m.label, m.ym), vec![], "확정월 보호가 우회됨", "정정 이력의 상태 변경 시각", FIX_NO_AUTO);
                }
            }
        }
    }
    a.close("closed_month", "월 확정 상태와 정정 이력", "check");
    // 13. 정책 이력
    {
        let targets: i64 = conn.query_row("SELECT count(*) FROM voucher_target WHERE school_year_id = ?1", [yid], |r| r.get(0))?;
        let policies: i64 = conn.query_row("SELECT count(*) FROM voucher_policy WHERE school_year_id = ?1", [yid], |r| r.get(0))?;
        let current: i64 = conn.query_row("SELECT count(*) FROM voucher_policy WHERE school_year_id = ?1 AND superseded_at IS NULL", [yid], |r| r.get(0))?;
        if targets > 0 && policies == 0 {
            a.item("policy", "check", "지원 한도 정책 없음", format!("대상자 {targets}명이 있지만 정책이 없습니다."), String::new(), String::new(), String::new(), vec![], "정책 미등록", "자유수강권 관리 → 지원 한도 설정", "정책을 등록해야 사용액이 계산됩니다.");
        }
        if policies > 0 && current == 0 {
            a.item("policy", "check", "현재 정책 없음", "정책 이력은 있으나 현재 적용 정책이 없습니다.".into(), String::new(), String::new(), String::new(), vec![], "이력 손상", "정책 변경 이력 확인", "정책을 다시 저장해 주세요.");
        }
        if current > 1 {
            a.item("policy", "error", "현재 정책 중복", format!("현재 정책이 {current}개입니다."), String::new(), String::new(), String::new(), vec![], "유일 제약 우회", "정책 변경 이력 확인", FIX_NO_AUTO);
        }
    }
    a.close("policy", "정책 변경 이력 보존", "check");
    // 14. 고아 정정
    {
        let list = crate::repo::adjustment::list(conn, &AdjustmentFilter { school_year_id: yid, ..Default::default() })?;
        for adj in list.iter().filter(|x| x.orphan) {
            a.item("orphan", "check", "원본 재확인 필요 (고아 정정)", format!("{} #{} 의 관련 원본 거래 #{} 가 {} 상태입니다.", adj.kind_label, adj.id, adj.related_entry_id.unwrap_or(0), adj.related_entry_state), adj.student.clone(), adj.program_name.clone(), adj.month_label.clone(), adj.entry_ids.clone(), "원본 파일 교체·복원", "회계 정정 → 정정 이력", "정정을 취소하고 새 원본 거래에 다시 등록해 주세요.");
        }
    }
    a.close("orphan", "원본 교체·복원 후 고아 정정", "check");
    // 15. 감사 로그 연결
    {
        let mut st = conn.prepare("SELECT a.id, a.kind FROM adjustment a WHERE a.school_year_id = ?1 AND NOT EXISTS (SELECT 1 FROM audit_log l WHERE l.target_table = 'adjustment' AND l.target_id = a.id)")?;
        let rows: Vec<(i64, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        for (id, kind) in rows.into_iter().take(100) {
            a.item("audit", "check", "감사 이력 없는 정정", format!("{} #{id} 에 감사 로그가 없습니다.", crate::repo::adjustment::kind_label(&kind)), String::new(), String::new(), String::new(), vec![], "감사 로그 누락", "변경 이력 화면", "정정 경위를 확인 메모로 남겨 주세요.");
        }
        let mut st = conn.prepare("SELECT b.id FROM voucher_batch b WHERE b.school_year_id = ?1 AND NOT EXISTS (SELECT 1 FROM audit_log l WHERE l.target_table = 'voucher_batch' AND l.target_id = b.id)")?;
        let rows: Vec<i64> = st.query_map([yid], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for id in rows {
            a.item("audit", "check", "감사 이력 없는 승인 묶음", format!("자유수강권 묶음 #{id} 에 감사 로그가 없습니다."), String::new(), String::new(), String::new(), vec![], "감사 로그 누락", "변경 이력 화면", "처리 경위를 확인해 주세요.");
        }
    }
    a.close("audit", "감사 로그 연결", "check");
    // 합산 중복 (프로그램·달 단위, 학생 없이도)
    {
        let mut st = conn.prepare(
            "SELECT e.fiscal_month_id, e.program_id, e.student_id, group_concat(DISTINCT i.code) FROM ledger_entry e JOIN amount_item i ON i.id = e.amount_item_id
              WHERE e.school_year_id = ?1 AND e.state = 'active' AND e.flow = 'collection' AND e.program_id IS NOT NULL AND i.code IN ('textbook', 'material', 'textbook_material')
              GROUP BY e.fiscal_month_id, e.program_id, e.student_id",
        )?;
        let rows: Vec<(i64, i64, Option<i64>, String)> = st.query_map([yid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        let mut seen: BTreeMap<(i64, i64), bool> = BTreeMap::new();
        for (m, p, s, codes) in rows {
            if codes.contains("textbook_material") && (codes.contains("textbook,") || codes.ends_with("textbook") || codes.contains("material,") || codes.split(',').any(|c| c == "material" || c == "textbook")) {
                if s.is_none() && seen.insert((m, p), true).is_some() {
                    continue;
                }
                if s.is_some() {
                    continue; // 학생 단위는 자유수강권 검사(dup_check)에서 다룬다
                }
                a.item("sum_item_dup", "check", "합산 항목 중복 여부 확인 필요", "교재·재료비(합산) 징수와 개별 교재비/재료비 징수가 같은 달·프로그램에 함께 있습니다.".into(), String::new(), program_name(conn, Some(p)), month_label(conn, m), vec![], "합산이 개별 항목의 합계 표시일 수 있음", "원본 파일의 행 구조 확인", "원장은 바꾸지 않습니다. 중복이면 해당 파일을 취소·교체해 주세요.");
            }
        }
    }
    // 합산 중복 검사 항목 집계는 위에서 close 했으므로 건수만 갱신
    if let Some(c) = a.checks.iter_mut().find(|c| c.code == "sum_item_dup") {
        c.count = a.items.iter().filter(|i| i.code == "sum_item_dup").count() as i64;
        if c.count > 0 {
            c.level = "check".into();
        }
    }
    let ok_count = a.checks.iter().filter(|c| c.level == "ok").count() as i64;
    let check_count = a.items.iter().filter(|i| i.level == "check").count() as i64;
    let error_count = a.items.iter().filter(|i| i.level == "error").count() as i64;
    Ok(IntegrityReport { checked_at: crate::ops::history::now(), school_year_id: yid, ok_count, check_count, error_count, checks: a.checks, items: a.items })
}
