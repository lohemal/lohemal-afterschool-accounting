//! P7-1.2 — 월 확정 · 프로그램 매핑 변경 안전성 (지시서 8장 A~F). 가명·가상 금액만 쓴다.
//!
//! 원칙: 원장 거래를 바꾸지 않아도 월 확정 당시의 회계 검증 결과가 달라질 수 있으면 막는다.
//! 영향 월 = 연결 변경 전후로 실제 검증 결과(프로그램×항목 행)가 달라지는 달. 지문 형식 2 는 연결·그룹 구성을 담는다.

use super::*;
use crate::db::Db;
use crate::model::{LinkItem, NewEntry};
use crate::repo::{adjustment, catalog, ledger, program, recon, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    oct: i64,
    fund_ben: i64,
    fund_voucher: i64,
    item_instructor: i64,
    item_tm: i64,
}

fn setup() -> T {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months = db.read(|c| year::months(c, y.id)).unwrap();
    let sep = months.iter().find(|m| m.ym == "2026-09").unwrap().id;
    let oct = months.iter().find(|m| m.ym == "2026-10").unwrap().id;
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    T { db, year_id: y.id, sep, oct, fund_ben: fund("beneficiary"), fund_voucher: fund("after_voucher"), item_instructor: item("instructor"), item_tm: item("textbook_material") }
}

fn li(program_id: i64, parent_id: Option<i64>) -> LinkItem {
    LinkItem { program_id, parent_id, expected_known: false, expected_parent_id: None }
}

impl T {
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    fn entry(&self, month: i64, program: Option<i64>, group: Option<i64>, fund: i64, item: i64, flow: &str, kind: &str, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: month, program_id: program, program_group_id: group, student_id: None,
                    fund_type_id: Some(fund), amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None,
                    counterparty: Some("가명".into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn collect(&self, month: i64, program: i64, amount: i64) -> i64 {
        self.entry(month, Some(program), None, self.fund_ben, self.item_instructor, "collection", "initial_collect", amount)
    }
    fn pay(&self, month: i64, program: i64, amount: i64) -> i64 {
        self.entry(month, Some(program), None, self.fund_ben, self.item_instructor, "expenditure", "instructor_pay", amount)
    }
    fn link(&self, child: i64, parent: Option<i64>) -> AppResult<LinkApplyResult> {
        self.db.write(|tx| apply(tx, self.year_id, &[li(child, parent)], "시험", "manual"))
    }
    fn preview(&self, items: &[LinkItem]) -> LinkPreview {
        self.db.read(|c| preview(c, self.year_id, items)).unwrap()
    }
    fn close(&self, m: i64) {
        self.db.write(|tx| year::month_close(tx, m, "확정")).unwrap()
    }
    fn reopen(&self, m: i64) {
        self.db.write(|tx| year::month_reopen(tx, m, "매핑 수정")).unwrap()
    }
    fn fp(&self, m: i64) -> String {
        self.db.read(|c| year::month_fingerprint(c, m)).unwrap().0
    }
    fn audit_count(&self) -> i64 {
        self.db.read(|c| c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).map_err(Into::into)).unwrap()
    }
    fn log_count(&self) -> i64 {
        self.db.read(|c| c.query_row("SELECT count(*) FROM program_link_log", [], |r| r.get(0)).map_err(Into::into)).unwrap()
    }
    /// 로봇과학A → 로봇과학 (9월 징수·지급), 코딩 (대안 통합)
    fn robot(&self) -> (i64, i64, i64) {
        let robot = self.program("로봇과학");
        let a = self.program("로봇과학A");
        let coding = self.program("코딩");
        self.collect(self.sep, a, 500_000);
        self.pay(self.sep, robot, 500_000);
        self.pay(self.sep, coding, 10_000);
        self.link(a, Some(robot)).unwrap();
        (robot, a, coding)
    }
}

// ── A. 직접 영향 ────────────────────────────────────────────────────────

#[test]
fn a1_a5_closed_month_blocks_change_unlink_new_manual_and_batch() {
    let t = setup();
    let (robot, a, coding) = t.robot();
    let b = t.program("로봇과학B");
    t.collect(t.sep, b, 1_000);
    t.close(t.sep);
    let before_audit = t.audit_count();
    let before_log = t.log_count();
    // 1. 변경 · 2. 해제 · 3. 신규 연결(추천) · 4. 수동 연결(다른 통합) · 5. 일괄
    for (label, items, rule) in [
        ("변경", vec![li(a, Some(coding))], "manual"),
        ("해제", vec![li(a, None)], "manual"),
        ("신규(추천)", vec![li(b, Some(robot))], "suggested"),
        ("수동", vec![li(b, Some(coding))], "manual"),
        ("일괄", vec![li(a, Some(coding)), li(b, Some(robot))], "suggested"),
    ] {
        let pv = t.preview(&items);
        assert!(pv.blocked, "{label}: {:?}", pv);
        assert!(pv.months.iter().any(|m| m.closed && m.label == "2026년 9월"), "{label}");
        assert!(pv.block_reasons.iter().all(|r| r.contains("2026년 9월 회계가 확정되어 있어") && r.contains("재개한 후 매핑을 변경하고 다시 확정")), "{label}: {:?}", pv.block_reasons);
        let err = t.db.write(|tx| apply(tx, t.year_id, &items, "", rule)).unwrap_err();
        assert_eq!(err.code, "INVALID", "{label}");
    }
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), Some(robot), "아무것도 바뀌지 않는다");
    assert_eq!(t.db.read(|c| parent_of(c, b)).unwrap(), None);
    assert_eq!((t.audit_count(), t.log_count()), (before_audit, before_log), "차단된 변경은 이력에 남지 않는다");
    // 차단 사유에는 세부반·이전·이후·확정 월·조치가 모두 들어 있다
    let pv = t.preview(&[li(a, Some(coding))]);
    let r = &pv.block_reasons[0];
    assert!(r.contains("'로봇과학A'") && r.contains("로봇과학 → 코딩") && r.contains("2026년 9월") && r.contains("재개"), "{r}");
}

// ── B. 간접 영향 ────────────────────────────────────────────────────────

#[test]
fn b6_b10_indirect_effects_through_parent_group_and_direct_payments() {
    let t = setup();
    // 6·8·9. 통합 프로그램에 직접 지급만 있는 10월: 세부반 거래가 없으면 10월 결과는 바뀌지 않는다 → 10월이 확정돼도 9월만 영향
    let (robot, a, coding) = t.robot();
    t.pay(t.oct, robot, 300_000);
    t.pay(t.oct, coding, 1);
    t.close(t.oct);
    let pv = t.preview(&[li(a, Some(coding))]);
    assert!(!pv.blocked, "{:?}", pv.block_reasons);
    assert_eq!(pv.months.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), vec!["2026년 9월"]);
    // 9월을 확정하면 기존 통합(로봇과학) → 새 통합(코딩) 변경이 막힌다 — 둘 다 9월에 거래가 있고 결과가 바뀐다
    t.close(t.sep);
    assert!(t.preview(&[li(a, Some(coding))]).blocked);
    t.reopen(t.sep);
    // 7. 그룹 구성원 거래: 통합 프로그램이 그룹 구성원이면 그룹 행도 바뀐다 (영향 행 수에 그룹 행 포함)
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    let uka = t.program("우쿨렐레A");
    let g = t.db.write(|tx| program::group_create(tx, t.year_id, "우쿨렐레&통기타", &[uk, gt])).unwrap();
    t.collect(t.sep, uka, 300_000);
    t.collect(t.sep, gt, 200_000);
    t.entry(t.sep, None, Some(g), t.fund_ben, t.item_instructor, "expenditure", "instructor_pay", 500_000);
    let pv = t.preview(&[li(uka, Some(uk))]);
    assert!(!pv.blocked);
    let m = pv.months.iter().find(|m| m.label == "2026년 9월").unwrap();
    assert!(m.changed_rows >= 3, "그룹·우쿨렐레(group_only)·우쿨렐레A 행이 바뀐다: {:?}", m);
    t.close(t.sep);
    assert!(t.preview(&[li(uka, Some(uk))]).blocked, "그룹을 통한 간접 영향도 보호");
    t.reopen(t.sep);
    // 10. 세부반 직접 지급: 세부반에 지급만 있어도 그 달은 영향 월
    let c = t.program("코딩A");
    t.pay(t.oct, c, 7_000);
    let pv = t.preview(&[li(c, Some(coding))]);
    assert!(pv.months.iter().any(|m| m.label == "2026년 10월" && m.closed), "{:?}", pv.months);
    assert!(pv.blocked);
}

#[test]
fn b11_b12_transfer_and_refund_entries_count_as_impact() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    t.pay(t.sep, robot, 100_000);
    // 11. 재원경정만 있는 세부반 (쌍의 합 0) — 10월
    t.collect(t.oct, a, 50_000);
    t.pay(t.oct, robot, 50_000);
    let input = crate::model::AdjustmentInput {
        school_year_id: t.year_id, fiscal_month_id: t.oct, kind: "fund_transfer".into(), program_id: a, student_id: None,
        from_fund_id: Some(t.fund_ben), to_fund_id: Some(t.fund_voucher), fund_type_id: None, amount_item_id: t.item_instructor, amount: 10_000,
        decided_on: Some("2026-10-13".into()), processed_on: Some("2026-10-13".into()), status: "done".into(),
        reason: "시험".into(), related_entry_id: None, related_adjustment_id: None, confirm_duplicate: false,
    };
    t.db.write(|tx| adjustment::create(tx, &input)).unwrap();
    // 12. 환불만 있는 세부반 — 9월
    t.entry(t.sep, Some(a), None, t.fund_ben, t.item_instructor, "collection", "refund", -20_000);
    let pv = t.preview(&[li(a, Some(robot))]);
    let labels: Vec<&str> = pv.months.iter().map(|m| m.label.as_str()).collect();
    assert_eq!(labels, vec!["2026년 9월", "2026년 10월"], "{:?}", pv.months);
    t.close(t.oct);
    let pv = t.preview(&[li(a, Some(robot))]);
    assert!(pv.blocked && pv.block_reasons.iter().any(|r| r.contains("2026년 10월")));
}

// ── C. 영향 월 ──────────────────────────────────────────────────────────

#[test]
fn c13_c17_affected_months_are_exactly_where_results_change() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    let coding = t.program("코딩");
    // 14. 여러 달에 걸친 세부반 (9·10월) · 13. 같은 학년도의 다른 달(11월)은 거래가 없어 영향 없음
    t.collect(t.sep, a, 1_000);
    t.collect(t.oct, a, 2_000);
    t.pay(t.sep, robot, 1_000);
    let nov = t.db.read(|c| year::months(c, t.year_id)).unwrap().iter().find(|m| m.ym == "2026-11").unwrap().id;
    t.pay(nov, robot, 5);
    t.pay(nov, coding, 5);
    let pv = t.preview(&[li(a, Some(robot))]);
    assert_eq!(pv.months.iter().map(|m| m.label.as_str()).collect::<Vec<_>>(), vec!["2026년 9월", "2026년 10월"], "{:?}", pv.months);
    t.link(a, Some(robot)).unwrap();
    // 15·16. 기존 통합(로봇과학)에만 / 새 통합(코딩)에만 거래가 있는 11월: 세부반 거래가 없으므로 결과가 바뀌지 않는다 → 영향 월 아님
    t.close(nov);
    let pv = t.preview(&[li(a, Some(coding))]);
    assert!(!pv.blocked, "{:?}", pv.block_reasons);
    assert!(!pv.months.iter().any(|m| m.label == "2026년 11월"));
    // 17. 다른 학년도의 확정 월은 무관
    let y2 = t.db.write(|tx| year::create(tx, 2027)).unwrap();
    let m2 = t.db.read(|c| year::months(c, y2.id)).unwrap()[0].id;
    let p2 = t.db.write(|tx| program::create(tx, y2.id, "로봇과학")).unwrap();
    t.db.write(|tx| ledger::insert(tx, &NewEntry { school_year_id: y2.id, fiscal_month_id: m2, program_id: Some(p2), program_group_id: None, student_id: None, fund_type_id: Some(t.fund_ben), amount_item_id: t.item_instructor, flow: "expenditure".into(), entry_kind: "instructor_pay".into(), amount: 1, stage: None, processed_on: None, counterparty: None, upload_file_id: None, source_row_id: None, reason: None })).unwrap();
    t.db.write(|tx| year::month_close(tx, m2, "2027")).unwrap();
    let pv = t.preview(&[li(a, Some(coding))]);
    assert!(!pv.blocked && pv.months.iter().all(|m| m.label.starts_with("2026년")));
}

// ── D. 지문 ─────────────────────────────────────────────────────────────

#[test]
fn d18_d23_fingerprint_reflects_links_and_groups_deterministically() {
    let t = setup();
    let (robot, a, coding) = t.robot();
    t.collect(t.oct, robot, 1);
    // 18. 같은 상태 → 같은 지문 (두 번 계산, 조회 순서 무관)
    let f1 = t.fp(t.sep);
    assert_eq!(f1, t.fp(t.sep));
    // 19·22. 연결 변경 → 지문 변경 (시각을 쓰지 않으니 같은 초라도 잡힌다)
    t.link(a, Some(coding)).unwrap();
    let f2 = t.fp(t.sep);
    assert_ne!(f1, f2);
    t.link(a, Some(robot)).unwrap();
    assert_eq!(t.fp(t.sep), f1, "같은 연결로 돌아오면 같은 지문");
    // 20. 관련 없는 연결 변경(이 달에 거래 없는 세부반) → 지문 불변
    let x = t.program("체스A");
    let chess = t.program("체스");
    t.collect(t.oct, x, 1);
    t.link(x, Some(chess)).unwrap();
    assert_eq!(t.fp(t.sep), f1);
    assert_ne!(t.fp(t.oct), f1, "거래가 있는 10월 지문은 다르다");
    // 21. 그룹 관계 변경 → 이 달에 거래가 있는 프로그램이 그룹에 들면 지문 변경
    let g_before = t.fp(t.sep);
    let gt = t.program("통기타");
    t.db.write(|tx| program::group_create(tx, t.year_id, "로봇&통기타", &[robot, gt])).unwrap();
    assert_ne!(t.fp(t.sep), g_before);
    // 23. 기존 거래 지문 검증 유지: 거래 상태 변경도 여전히 잡힌다
    let f3 = t.fp(t.sep);
    let e = t.collect(t.sep, a, 1);
    assert_ne!(t.fp(t.sep), f3);
    t.db.write(|tx| ledger::void(tx, e, "input_error", "시험")).unwrap();
    assert_ne!(t.fp(t.sep), f3, "취소된 거래도 지문에 든다 (상태 포함)");
}

#[test]
fn d24_legacy_v1_close_is_verified_with_v1_and_not_flagged() {
    let t = setup();
    let (robot, a, _coding) = t.robot();
    // P6 시절(형식 1) 확정을 흉내 낸다
    let (fp, snap) = t.db.read(|c| year::month_fingerprint_v(c, t.sep, 1)).unwrap();
    t.db.write(|tx| {
        tx.execute("UPDATE fiscal_month SET status = 'closed' WHERE id = ?1", [t.sep])?;
        tx.execute("INSERT INTO month_close (fiscal_month_id, action, totals_json, note, fingerprint, snapshot_json) VALUES (?1, 'close', '{}', '옛 확정', ?2, ?3)", rusqlite::params![t.sep, fp, snap.to_string()])?;
        Ok(())
    }).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, t.sep)).unwrap();
    assert!(ch.has_fingerprint && ch.fingerprint_version == 1 && !ch.changed, "{}", ch.summary);
    assert!(!t.db.read(|c| year::months(c, t.year_id)).unwrap().iter().find(|m| m.id == t.sep).unwrap().changed_after_close);
    // 구형 확정은 연결 변경을 지문으로 잡지 못하지만, 매핑 변경 자체는 확정 월이라 차단된다
    assert!(t.preview(&[li(a, None)]).blocked);
    // 거래 변경은 여전히 잡힌다 (구형 검증 유지)
    t.reopen(t.sep);
    t.collect(t.sep, robot, 1);
    t.db.write(|tx| tx.execute("UPDATE fiscal_month SET status = 'closed' WHERE id = ?1", [t.sep]).map(|_| ()).map_err(Into::into)).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, t.sep)).unwrap();
    assert!(ch.changed && ch.fingerprint_version == 1 && ch.new_entry_ids.len() == 1);
    // 옛 확정 행은 그대로 남아 있다 (다시 쓰지 않음)
    let kept: String = t.db.read(|c| c.query_row("SELECT fingerprint FROM month_close WHERE action = 'close' ORDER BY id LIMIT 1", [], |r| r.get(0)).map_err(Into::into)).unwrap();
    assert_eq!(kept, fp);
}

// ── E. 미리보기 ─────────────────────────────────────────────────────────

#[test]
fn e25_e29_preview_matches_apply_detects_conflicts_and_rolls_back_batches() {
    let t = setup();
    let (robot, a, coding) = t.robot();
    let b = t.program("로봇과학B");
    t.collect(t.sep, b, 100);
    // 25·26. 영향 월 · 확정 월 차단 사유
    let pv = t.preview(&[li(b, Some(robot))]);
    assert_eq!(pv.months.len(), 1);
    assert!(pv.months[0].changed_rows >= 2 && !pv.months[0].closed);
    t.close(t.sep);
    let pv = t.preview(&[li(b, Some(robot))]);
    assert!(pv.blocked && pv.months[0].closed && pv.block_reasons[0].contains("확정되어 있어"));
    t.reopen(t.sep);
    // 28. 미리보기 이후 상태 변경 — 미리보기 때 본 연결(expected)이 지금과 다르면 충돌
    let item = LinkItem { program_id: a, parent_id: Some(coding), expected_known: true, expected_parent_id: Some(robot) };
    assert!(!t.preview(&[item.clone()]).blocked);
    t.link(a, None).unwrap(); // 다른 사용자가 먼저 해제
    let pv = t.preview(&[item.clone()]);
    assert!(pv.blocked && pv.block_reasons[0].contains("미리보기 이후 연결이 바뀌었습니다"), "{:?}", pv.block_reasons);
    let err = t.db.write(|tx| apply(tx, t.year_id, &[item], "", "manual")).unwrap_err();
    assert!(err.message.contains("미리보기 이후"));
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), None);
    // 27. 적용 직전 재검증은 미리보기와 같은 함수 — 미리보기가 통과하면 적용되고, 그 사이 확정되면 적용이 막힌다
    let ok_item = LinkItem { program_id: a, parent_id: Some(robot), expected_known: true, expected_parent_id: None };
    assert!(!t.preview(&[ok_item.clone()]).blocked);
    t.close(t.sep);
    assert!(t.db.write(|tx| apply(tx, t.year_id, &[ok_item.clone()], "", "manual")).is_err());
    t.reopen(t.sep);
    t.db.write(|tx| apply(tx, t.year_id, &[ok_item], "", "manual")).unwrap();
    // 29. 일괄: 하나라도 막히면 전부 롤백 (연결표·이력·감사 모두)
    let (audit0, log0) = (t.audit_count(), t.log_count());
    let items = vec![li(b, Some(robot)), li(robot, Some(coding))]; // 둘째는 깊이 위반
    let err = t.db.write(|tx| apply(tx, t.year_id, &items, "", "manual")).unwrap_err();
    assert_eq!(err.code, "INVALID");
    assert_eq!(t.db.read(|c| parent_of(c, b)).unwrap(), None, "첫 항목도 적용되지 않는다");
    assert_eq!((t.audit_count(), t.log_count()), (audit0, log0));
}

// ── F. 재개 → 변경 → 재확정 ──────────────────────────────────────────────

#[test]
fn f30_f34_reopen_change_reclose_makes_new_fingerprint_and_keeps_history() {
    let t = setup();
    let (robot, a, coding) = t.robot();
    t.close(t.sep);
    let fp1: String = t.db.read(|c| c.query_row("SELECT fingerprint FROM month_close WHERE fiscal_month_id = ?1 AND action = 'close'", [t.sep], |r| r.get(0)).map_err(Into::into)).unwrap();
    let r_before = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    assert_eq!(r_before.rows.iter().find(|r| r.unit_name == "로봇과학").unwrap().status, "ok");
    // 확정 상태: 변경·해제 차단
    assert!(t.link(a, Some(coding)).is_err());
    assert!(t.link(a, None).is_err());
    // 30. 재개 → 31. 변경 → 검증 결과 재계산·전후 차이
    t.reopen(t.sep);
    let ch = t.db.read(|c| year::month_changes(c, t.sep)).unwrap();
    assert!(ch.has_fingerprint && ch.fingerprint_version == 2 && !ch.changed);
    t.link(a, Some(coding)).unwrap();
    let ch = t.db.read(|c| year::month_changes(c, t.sep)).unwrap();
    assert!(ch.changed && ch.link_changed && ch.summary.contains("연결 변경"), "{:?}", ch.summary);
    let r_after = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    assert_eq!(r_after.rows.iter().find(|r| r.unit_name == "로봇과학").unwrap().status, "missing");
    assert_eq!(r_after.rows.iter().find(|r| r.unit_name == "코딩").unwrap().status, "mismatch");
    assert_eq!(r_after.collection_total, r_before.collection_total, "원장 총액 불변");
    // 32·33. 재확정 → 새 지문 (형식 2) 생성, 확정 뒤 변경 없음
    t.close(t.sep);
    let closes: Vec<(String, String)> = t.db.read(|c| {
        let mut st = c.prepare("SELECT action, fingerprint FROM month_close WHERE fiscal_month_id = ?1 ORDER BY id")?;
        let v = st.query_map([t.sep], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }).unwrap();
    assert_eq!(closes.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["close", "reopen", "close"], "기존 확정 이력은 지우지 않는다");
    assert_ne!(closes[2].1, fp1);
    assert_eq!(closes[0].1, fp1, "첫 확정 지문 그대로");
    let ch = t.db.read(|c| year::month_changes(c, t.sep)).unwrap();
    assert!(!ch.changed && ch.fingerprint_version == 2);
    // 재확정 뒤 추가 변경 차단
    assert!(t.link(a, Some(robot)).is_err());
    // 34. 감사 이력: 확정·재개·연결 변경이 모두 남아 있다
    let kinds: Vec<String> = t.db.read(|c| {
        let mut st = c.prepare("SELECT kind FROM audit_log ORDER BY id")?;
        let v = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
        Ok(v)
    }).unwrap();
    for k in ["month.close", "month.reopen", "program.link.change", "program.link.confirm"] {
        assert!(kinds.iter().any(|x| x == k), "{k} 없음: {:?}", kinds);
    }
    assert_eq!(t.log_count(), 2);
}
