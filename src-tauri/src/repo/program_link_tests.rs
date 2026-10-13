//! P7-1.1 세부반 → 통합 프로그램 연결 시험 (지시서 14장 A·B·D·E 일부). 가명·가상 금액만 쓴다.

use super::*;
use crate::db::Db;
use crate::model::{LinkItem, NewEntry};
use crate::repo::{catalog, ledger, program, recon, year};

struct T {
    db: Db,
    year_id: i64,
    sep: i64,
    oct: i64,
    fund_ben: i64,
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
    T { db, year_id: y.id, sep, oct, fund_ben: fund("beneficiary"), item_instructor: item("instructor"), item_tm: item("textbook_material") }
}

impl T {
    fn program(&self, name: &str) -> i64 {
        self.db.write(|tx| program::create(tx, self.year_id, name)).unwrap()
    }
    fn entry(&self, month: i64, program: i64, item: i64, flow: &str, kind: &str, amount: i64) -> i64 {
        self.db
            .write(|tx| {
                ledger::insert(tx, &NewEntry {
                    school_year_id: self.year_id, fiscal_month_id: month, program_id: Some(program), program_group_id: None, student_id: None,
                    fund_type_id: Some(self.fund_ben), amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None,
                    counterparty: Some("가명".into()), upload_file_id: None, source_row_id: None, reason: None,
                }).map(|e| e.id)
            })
            .unwrap()
    }
    fn collect(&self, program: i64, amount: i64) -> i64 {
        self.entry(self.sep, program, self.item_instructor, "collection", "initial_collect", amount)
    }
    fn pay(&self, program: i64, amount: i64) -> i64 {
        self.entry(self.sep, program, self.item_instructor, "expenditure", "instructor_pay", amount)
    }
    fn link(&self, child: i64, parent: Option<i64>) -> AppResult<LinkApplyResult> {
        self.db.write(|tx| apply(tx, self.year_id, &[LinkItem { program_id: child, parent_id: parent }], "시험", "manual"))
    }
    fn board(&self) -> LinkBoard {
        self.db.read(|c| board(c, self.year_id)).unwrap()
    }
    fn row<'a>(&self, b: &'a LinkBoard, name: &str) -> &'a LinkRow {
        b.rows.iter().find(|r| r.name == name).unwrap()
    }
}

// ── A. 프로그램 그룹(통합 프로그램) ─────────────────────────────────────

#[test]
fn a1_a3_umbrella_create_links_several_classes() {
    let t = setup();
    let a = t.program("로봇과학A");
    let b = t.program("로봇과학B");
    let id = t.db.write(|tx| umbrella_create(tx, t.year_id, "로봇과학", &[a, b], "직접 생성")).unwrap();
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), Some(id));
    assert_eq!(t.db.read(|c| children_of(c, id)).unwrap(), vec![a, b]);
    let bd = t.board();
    assert_eq!(t.row(&bd, "로봇과학").role, "parent");
    assert_eq!(t.row(&bd, "로봇과학A").status, "confirmed");
    assert_eq!(bd.parents.len(), 1);
    assert_eq!(bd.parents[0].children.len(), 2);
    assert_eq!(bd.confirmed_count, 2);
    // 같은 이름의 통합 프로그램을 다시 만들면 기존 프로그램에 잇는다 (중복 생성 없음)
    let c = t.program("로봇과학C");
    let again = t.db.write(|tx| umbrella_create(tx, t.year_id, "로봇과학", &[c], "")).unwrap();
    assert_eq!(again, id);
    assert_eq!(t.db.read(|c| children_of(c, id)).unwrap().len(), 3);
    let programs = t.db.read(|c| program::list(c, t.year_id)).unwrap();
    assert_eq!(programs.iter().find(|p| p.name == "로봇과학").unwrap().child_count, 3);
    assert_eq!(programs.iter().find(|p| p.name == "로봇과학A").unwrap().parent_name.as_deref(), Some("로봇과학"));
}

#[test]
fn a4_one_class_belongs_to_one_umbrella_and_depth_is_one() {
    let t = setup();
    let robot = t.program("로봇과학");
    let coding = t.program("코딩");
    let a = t.program("로봇과학A");
    t.link(a, Some(robot)).unwrap();
    // 같은 세부반을 다른 통합에 '추가'로 잇는 것은 변경(change)이지 중복이 아니다 — 연결은 하나뿐
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: a, parent_id: Some(coding) }])).unwrap();
    assert_eq!(pv.items[0].action, "change");
    assert!(!pv.blocked);
    assert_eq!(t.db.read(|c| links(c, t.year_id)).unwrap().len(), 1);
    // 깊이 한 단계: 통합 프로그램 '로봇과학' 을 다시 세부반으로 → 막힘
    let err = t.link(robot, Some(coding)).unwrap_err();
    assert_eq!(err.code, "INVALID");
    assert!(err.message.contains("세부반을 가진 통합 프로그램"), "{}", err.message);
    // 세부반 '로봇과학A' 를 통합 프로그램으로 → 막힘
    let b = t.program("로봇과학B");
    let err = t.link(b, Some(a)).unwrap_err();
    assert!(err.message.contains("다른 통합 프로그램의 세부반"), "{}", err.message);
    // 자기 자신 · 한 묶음에 같은 세부반 두 번 · 다른 학년도
    assert!(t.link(b, Some(b)).is_err());
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: b, parent_id: Some(robot) }, LinkItem { program_id: b, parent_id: Some(coding) }])).unwrap();
    assert!(pv.blocked && pv.block_reasons.iter().any(|r| r.contains("두 번")));
    let y2 = t.db.write(|tx| year::create(tx, 2027)).unwrap();
    let other = t.db.write(|tx| program::create(tx, y2.id, "로봇과학")).unwrap();
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: b, parent_id: Some(other) }])).unwrap();
    assert!(pv.blocked && pv.block_reasons.iter().any(|r| r.contains("학년도")));
    // 그룹 구성원은 세부반이 될 수 없고, 세부반은 그룹에 들어갈 수 없다
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    t.db.write(|tx| program::group_create(tx, t.year_id, "우쿨렐레&통기타", &[uk, gt])).unwrap();
    let err = t.link(uk, Some(robot)).unwrap_err();
    assert!(err.message.contains("그룹"), "{}", err.message);
    let err = t.db.write(|tx| program::group_create(tx, t.year_id, "그룹2", &[a, coding])).unwrap_err();
    assert!(err.message.contains("세부반"), "{}", err.message);
}

#[test]
fn a5_a7_change_unlink_and_history() {
    let t = setup();
    let robot = t.program("로봇과학");
    let coding = t.program("코딩");
    let a = t.program("로봇과학A");
    t.link(a, Some(robot)).unwrap();
    t.link(a, Some(coding)).unwrap();
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), Some(coding));
    let r = t.link(a, None).unwrap();
    assert_eq!(r.applied, 1);
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), None);
    // 변경 없음(이미 해제) 은 아무것도 기록하지 않는다
    let r = t.link(a, None).unwrap();
    assert_eq!(r.applied, 0);
    let h = t.db.read(|c| history(c, t.year_id)).unwrap();
    assert_eq!(h.iter().map(|l| l.action.as_str()).collect::<Vec<_>>(), vec!["unlink", "change", "confirm"]);
    assert_eq!((h[1].from_parent_name.as_deref(), h[1].to_parent_name.as_deref()), (Some("로봇과학"), Some("코딩")));
    assert!(h.iter().all(|l| l.confirmed && l.note == "시험"));
    // 이력은 지울 수 없다
    let err = t.db.write(|tx| tx.execute("DELETE FROM program_link_log", []).map_err(Into::into)).unwrap_err();
    assert!(err.detail.unwrap_or_default().contains("LINK_LOG_NO_DELETE"));
    // 감사 로그에도 남는다
    let audit = t.db.read(|c| crate::repo::audit::list(c, 50)).unwrap();
    assert_eq!(audit.iter().filter(|a| a.kind.starts_with("program.link.")).count(), 3);
}

// ── B. 자동 추천 ────────────────────────────────────────────────────────

#[test]
fn b8_b10_suggests_classes_of_programs_that_have_payments() {
    let t = setup();
    let robot = t.program("로봇과학");
    let anim = t.program("만화애니메이션");
    let a = t.program("로봇과학A");
    let b = t.program("로봇과학B");
    let ma = t.program("만화애니메이션A");
    t.pay(robot, 900_000);
    t.pay(anim, 500_000);
    let s = t.db.read(|c| pending(c, t.year_id)).unwrap();
    let got: Vec<(i64, i64)> = s.iter().map(|x| (x.program_id, x.parent_id)).collect();
    assert_eq!(got, vec![(a, robot), (b, robot), (ma, anim)], "이름순");
    assert!(s.iter().all(|x| x.recommended && x.reason.contains("지급 자료")));
    // 추천은 적용되지 않는다 (B14)
    assert!(t.db.read(|c| links(c, t.year_id)).unwrap().is_empty());
    let bd = t.board();
    assert_eq!(bd.pending_count, 3);
    assert_eq!(t.row(&bd, "로봇과학A").status, "suggested");
    // 확정하면 사라진다
    t.db.write(|tx| apply(tx, t.year_id, &[LinkItem { program_id: a, parent_id: Some(robot) }, LinkItem { program_id: b, parent_id: Some(robot) }], "", "suggested")).unwrap();
    let s = t.db.read(|c| pending(c, t.year_id)).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].program_id, ma);
    // 통합 프로그램이 된 뒤에는 지급 없이도 세부반 추천이 된다 (로봇과학C)
    let c = t.program("로봇과학C");
    let s = t.db.read(|c2| pending(c2, t.year_id)).unwrap();
    assert!(s.iter().any(|x| x.program_id == c && x.parent_id == robot && x.reason.contains("이미 세부반")));
}

#[test]
fn b11_b13_no_payment_name_conflict_and_bad_suffix() {
    let t = setup();
    // B11: 대응 지급이 없으면 후보일 뿐 추천이 아니다
    let eng = t.program("영어");
    let ea = t.program("영어A");
    t.collect(eng, 10_000);
    let s = t.db.read(|c| suggestions(c, t.year_id)).unwrap();
    let x = s.iter().find(|x| x.program_id == ea).unwrap();
    assert!(!x.recommended && x.parent_id == eng && x.reason.contains("지급 자료가 없어"));
    assert!(t.db.read(|c| pending(c, t.year_id)).unwrap().is_empty());
    assert_eq!(t.row(&t.board(), "영어A").status, "candidate");
    // 바탕 이름의 프로그램이 아예 없으면 후보도 없다
    t.program("바둑A");
    assert!(!t.db.read(|c| suggestions(c, t.year_id)).unwrap().iter().any(|x| x.program_name == "바둑A"));
    // B12: 이름 충돌 — 바탕 이름이 그룹이면 추천하지 않는다
    let uk = t.program("우쿨렐레");
    let gt = t.program("통기타");
    t.db.write(|tx| program::group_create(tx, t.year_id, "합주", &[uk, gt])).unwrap();
    let ha = t.program("합주A");
    let s = t.db.read(|c| suggestions(c, t.year_id)).unwrap();
    let x = s.iter().find(|x| x.program_id == ha).unwrap();
    assert!(!x.recommended && x.reason.contains("그룹"));
    // 바탕 이름이 이미 세부반이면 추천하지 않는다 (로봇과학A 의 'A' 를 뗀 '로봇과학' 이 세부반인 경우)
    let robot = t.program("로봇과학");
    let coding = t.program("코딩");
    t.pay(coding, 1);
    t.link(robot, Some(coding)).unwrap();
    let ra = t.program("로봇과학A");
    let s = t.db.read(|c| suggestions(c, t.year_id)).unwrap();
    let x = s.iter().find(|x| x.program_id == ra).unwrap();
    assert!(!x.recommended && x.reason.contains("이미 다른 통합 프로그램의 세부반"));
    // B13: 잘못된 접미 — 'ABC' 는 'AB' 의 세부반이 아니다
    let ab = t.program("AB");
    t.pay(ab, 100);
    t.program("ABC");
    assert!(!t.db.read(|c| suggestions(c, t.year_id)).unwrap().iter().any(|x| x.program_name == "ABC"));
    // 이미 확정된 세부반·그룹 구성원·통합 프로그램 자신은 추천 목록에 없다
    let s = t.db.read(|c| suggestions(c, t.year_id)).unwrap();
    assert!(!s.iter().any(|x| x.program_id == robot || x.program_id == uk || x.program_id == coding));
}

#[test]
fn b14_upload_suggestion_prefers_child_when_payment_exists() {
    let t = setup();
    let robot = t.program("로봇과학");
    // 지급 자료가 없으면 표기 잇기(alias) 제안, 있으면 세부반(child) 제안
    let s = t.db.read(|c| program::suggest(c, t.year_id, "로봇과학A")).unwrap().unwrap();
    assert_eq!((s.0, s.2.as_str()), (robot, "alias"));
    t.pay(robot, 1);
    let s = t.db.read(|c| program::suggest(c, t.year_id, "로봇과학A")).unwrap().unwrap();
    assert_eq!((s.0, s.2.as_str()), (robot, "child"));
    assert_eq!(t.db.read(|c| program::suggest(c, t.year_id, "ABC")).unwrap(), None);
    // 제안은 적용되지 않는다
    assert_eq!(t.db.read(|c| program::resolve(c, t.year_id, "로봇과학A")).unwrap(), None);
    // new_child 로 확정하면 세부반 프로그램이 생기고 연결된다 (업로드 미리보기 흐름)
    let input = crate::model::ProgramMapInput { school_year_id: t.year_id, alias: "로봇과학A".into(), target: "new_child".into(), program_id: None, group_id: None, new_name: None, member_ids: None, parent_id: Some(robot) };
    let program::Target::Program(a) = t.db.write(|tx| program::map_save(tx, &input)).unwrap() else { panic!() };
    assert_eq!(t.db.read(|c| program::resolve(c, t.year_id, "로봇과학A")).unwrap(), Some(a));
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), Some(robot));
    let h = t.db.read(|c| history(c, t.year_id)).unwrap();
    assert_eq!((h[0].rule.as_str(), h[0].action.as_str()), ("suggested", "confirm"));
}

// ── D·E. 월 확정 보호 · 미리보기 영향 ───────────────────────────────────

#[test]
fn e34_closed_month_blocks_link_change_until_reopened() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    t.pay(robot, 900_000);
    t.collect(a, 500_000);
    t.entry(t.oct, a, t.item_tm, "collection", "initial_collect", 100);
    t.db.write(|tx| year::month_close(tx, t.sep, "9월 확정")).unwrap();
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: a, parent_id: Some(robot) }])).unwrap();
    assert!(pv.blocked, "{:?}", pv.block_reasons);
    assert!(pv.block_reasons.iter().any(|r| r.contains("확정된 달")));
    assert_eq!(pv.months.len(), 2);
    assert!(pv.months.iter().any(|m| m.closed && m.label.contains("9월")));
    let err = t.link(a, Some(robot)).unwrap_err();
    assert_eq!(err.code, "INVALID");
    assert!(t.db.read(|c| links(c, t.year_id)).unwrap().is_empty(), "막히면 아무것도 바뀌지 않는다");
    // 확정은 그대로이고 지문도 그대로 — 매핑은 월 확정을 우회하지 않는다
    assert!(t.db.read(|c| year::month_is_closed(c, t.sep)).unwrap());
    assert!(!t.db.read(|c| year::month_changes(c, t.sep)).unwrap().changed);
    // 재개하면 된다
    t.db.write(|tx| year::month_reopen(tx, t.sep, "매핑 수정")).unwrap();
    t.link(a, Some(robot)).unwrap();
    assert_eq!(t.db.read(|c| parent_of(c, a)).unwrap(), Some(robot));
    // 다시 확정한 뒤 해제도 막힌다
    t.db.write(|tx| year::month_close(tx, t.sep, "재확정")).unwrap();
    assert!(t.link(a, None).is_err());
    // 거래가 없는 새 세부반은 확정된 달이 있어도 연결된다 (영향 받는 달이 없다)
    let b = t.program("로봇과학B");
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: b, parent_id: Some(robot) }])).unwrap();
    assert!(!pv.blocked && pv.months.is_empty());
}

#[test]
fn preview_warns_about_direct_payments_and_reports_impact() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    t.pay(robot, 900_000);
    t.collect(a, 500_000);
    t.pay(a, 50_000);
    t.collect(robot, 30_000);
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: a, parent_id: Some(robot) }])).unwrap();
    assert!(!pv.blocked);
    assert_eq!(pv.change_count, 1);
    let it = &pv.items[0];
    assert_eq!((it.action.as_str(), it.collection_total, it.expense_total), ("confirm", 500_000, 50_000));
    assert!(it.warnings.iter().any(|w| w.contains("직접 연결된 지급")));
    assert!(it.warnings.iter().any(|w| w.contains("직접 귀속된 징수")));
    assert_eq!(pv.months.len(), 1);
    assert!(pv.warnings.iter().any(|w| w.contains("다시 계산")));
    // 지급 자료가 없는 통합 프로그램에 잇는 것은 경고만
    let eng = t.program("영어");
    let ea = t.program("영어A");
    t.collect(ea, 10);
    let pv = t.db.read(|c| preview(c, t.year_id, &[LinkItem { program_id: ea, parent_id: Some(eng) }])).unwrap();
    assert!(!pv.blocked && pv.items[0].warnings.iter().any(|w| w.contains("아직 지급 자료가 없습니다")));
}

// ── E. 자료 보존 ────────────────────────────────────────────────────────

#[test]
fn e31_e33_linking_keeps_names_ledger_and_audit() {
    let t = setup();
    let robot = t.program("로봇과학");
    let a = t.program("로봇과학A");
    let e1 = t.collect(a, 500_000);
    let e2 = t.pay(robot, 900_000);
    let audit_before: i64 = t.db.read(|c| c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).map_err(Into::into)).unwrap();
    let ledger_before: Vec<(i64, Option<i64>, i64, String)> = t.db.read(|c| {
        let mut st = c.prepare("SELECT id, program_id, amount, state FROM ledger_entry ORDER BY id")?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }).unwrap();
    t.link(a, Some(robot)).unwrap();
    t.link(a, None).unwrap();
    t.link(a, Some(robot)).unwrap();
    let ledger_after: Vec<(i64, Option<i64>, i64, String)> = t.db.read(|c| {
        let mut st = c.prepare("SELECT id, program_id, amount, state FROM ledger_entry ORDER BY id")?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        Ok(v)
    }).unwrap();
    assert_eq!(ledger_before, ledger_after, "원장 거래는 바뀌지 않는다");
    assert_eq!(ledger_after[0].1, Some(a), "세부반 거래의 프로그램은 세부반 그대로");
    assert_eq!(ledger_after[1].1, Some(robot));
    assert_eq!(t.db.read(|c| program::name_of(c, a)).unwrap(), "로봇과학A");
    let audit_after: i64 = t.db.read(|c| c.query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0)).map_err(Into::into)).unwrap();
    assert_eq!(audit_after, audit_before + 3, "감사 이력은 덧붙기만 한다");
    // 대조에서 세부반 거래는 통합 행에 한 번만 든다
    let r = t.db.read(|c| recon::month(c, t.sep)).unwrap();
    let row = r.rows.iter().find(|x| x.unit_name == "로봇과학" && x.item_set == "instructor").unwrap();
    assert_eq!((row.basis, row.expense), (500_000, 900_000));
    assert!(row.children.iter().any(|c| c.program_id == a && c.basis == 500_000));
    let _ = (e1, e2);
}

// ── F. 마이그레이션 v9 → v10 · v1 → v10 · 실패 시 v9 유지 ────────────────

fn v9_db_with_data() -> (rusqlite::Connection, i64, i64, i64) {
    use crate::db::migrate;
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::run_up_to(&mut c, 9).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), 9);
    let y = year::create(&c, 2026).unwrap();
    let sep: i64 = c.query_row("SELECT id FROM fiscal_month WHERE school_year_id = ?1 AND seq = 7", [y.id], |r| r.get(0)).unwrap();
    let robot = program::create(&c, y.id, "로봇과학").unwrap();
    let a = program::create(&c, y.id, "로봇과학A").unwrap();
    let cat = catalog::all(&c).unwrap();
    let fund = cat.fund_types.iter().find(|f| f.code == "beneficiary").unwrap().id;
    let item = cat.amount_items.iter().find(|f| f.code == "instructor").unwrap().id;
    let mk = |p: i64, flow: &str, kind: &str, amount: i64| NewEntry { school_year_id: y.id, fiscal_month_id: sep, program_id: Some(p), program_group_id: None, student_id: None, fund_type_id: Some(fund), amount_item_id: item, flow: flow.into(), entry_kind: kind.into(), amount, stage: None, processed_on: None, counterparty: Some("가명".into()), upload_file_id: None, source_row_id: None, reason: None };
    ledger::insert(&c, &mk(a, "collection", "initial_collect", 500_000)).unwrap();
    ledger::insert(&c, &mk(robot, "expenditure", "instructor_pay", 500_000)).unwrap();
    year::month_close(&c, sep, "v9 확정").unwrap();
    (c, y.id, robot, a)
}

fn snapshot(c: &rusqlite::Connection) -> Vec<String> {
    let mut out = Vec::new();
    for sql in [
        "SELECT count(*) || '|' || coalesce(sum(amount), 0) FROM ledger_entry WHERE state = 'active'",
        "SELECT count(*) FROM audit_log",
        "SELECT count(*) FROM month_close",
        "SELECT group_concat(name) FROM program ORDER BY id",
        "SELECT group_concat(status) FROM fiscal_month ORDER BY id",
    ] {
        out.push(c.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap_or_default());
    }
    out
}

#[test]
fn f37_v9_to_v10_keeps_data_and_adds_link_tables() {
    use crate::db::migrate;
    let (mut c, yid, robot, a) = v9_db_with_data();
    let before = snapshot(&c);
    migrate::run(&mut c).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), 10);
    assert_eq!(snapshot(&c), before, "원장·감사·확정 이력·프로그램 보존");
    assert_eq!(links(&c, yid).unwrap().len(), 0);
    let ic: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap();
    assert_eq!(ic, "ok");
    // 옛 자료 위에서 추천·확정이 된다 (확정된 달이라 재개 먼저)
    assert!(pending(&c, yid).unwrap().iter().any(|s| s.program_id == a && s.parent_id == robot));
    let sep: i64 = c.query_row("SELECT fiscal_month_id FROM ledger_entry LIMIT 1", [], |r| r.get(0)).unwrap();
    assert!(apply(&c, yid, &[LinkItem { program_id: a, parent_id: Some(robot) }], "", "suggested").is_err(), "v9 때 확정한 달은 여전히 보호된다");
    year::month_reopen(&c, sep, "매핑").unwrap();
    apply(&c, yid, &[LinkItem { program_id: a, parent_id: Some(robot) }], "", "suggested").unwrap();
    let r = recon::month(&c, sep).unwrap();
    let row = r.rows.iter().find(|x| x.unit_name == "로봇과학" && x.item_set == "instructor").unwrap();
    assert_eq!((row.status.as_str(), row.basis, row.expense), ("ok", 500_000, 500_000));
}

#[test]
fn f38_v1_to_v10_in_one_go() {
    use crate::db::migrate;
    let mut c = rusqlite::Connection::open_in_memory().unwrap();
    c.pragma_update(None, "foreign_keys", "ON").unwrap();
    migrate::run_up_to(&mut c, 1).unwrap();
    let y = year::create(&c, 2026).unwrap();
    c.execute("INSERT INTO program (school_year_id, name) VALUES (?1, '코딩')", [y.id]).unwrap();
    migrate::run(&mut c).unwrap();
    assert_eq!(migrate::current_version(&c).unwrap(), 10);
    let n: i64 = c.query_row("SELECT count(*) FROM program WHERE name = '코딩'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
    assert!(board(&c, y.id).unwrap().rows.iter().any(|r| r.name == "코딩" && r.status == "none"));
    let fk = c.prepare("PRAGMA foreign_key_check").unwrap().query_map([], |_| Ok(())).unwrap().count();
    assert_eq!(fk, 0);
}

#[test]
fn f39_failed_v10_migration_leaves_v9_untouched() {
    use crate::db::migrate;
    let (mut c, _yid, _robot, _a) = v9_db_with_data();
    let before = snapshot(&c);
    // 010 이 만들 표를 미리 다른 구조로 두어 SQL 을 실패시킨다
    c.execute("CREATE TABLE program_link (x TEXT)", []).unwrap();
    let err = migrate::run(&mut c).unwrap_err();
    assert_eq!(err.code, "MIGRATE");
    assert!(err.detail.unwrap_or_default().contains("010_program_link"));
    assert_eq!(migrate::current_version(&c).unwrap(), 9, "실패하면 버전이 오르지 않는다");
    assert_eq!(snapshot(&c), before, "자료는 그대로");
    let has_log = c.query_row("SELECT count(*) FROM sqlite_master WHERE name = 'program_link_log'", [], |r| r.get::<_, i64>(0)).unwrap();
    assert_eq!(has_log, 0, "부분 적용 없음 (트랜잭션 롤백)");
}
