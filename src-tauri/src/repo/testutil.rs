//! 시험 공용 — 학년도·프로그램이 갖춰진 메모리 DB 를 만든다. 가명·가상 자료만 쓴다.

use crate::db::Db;
use crate::model::NewEntry;
use crate::repo::{catalog, program, year};

#[allow(dead_code)] // 뒤 단계 시험에서 쓸 자리까지 미리 갖춘다
pub struct Fixture {
    pub db: Db,
    pub year_id: i64,
    /// 3월 … 2월 회계월 id (차례 1..12)
    pub months: Vec<i64>,
    pub program_coding: i64,
    pub program_robot: i64,
    pub fund_beneficiary: i64,
    pub fund_free_voucher: i64,
    pub fund_g3_support: i64,
    pub item_instructor: i64,
    pub item_textbook_material: i64,
}

pub fn fixture() -> Fixture {
    let db = Db::open_memory().unwrap();
    let y = db.write(|tx| year::create(tx, 2026)).unwrap();
    let months: Vec<i64> = db.read(|c| year::months(c, y.id)).unwrap().iter().map(|m| m.id).collect();
    let program_coding = db.write(|tx| program::create(tx, y.id, "코딩")).unwrap();
    let program_robot = db.write(|tx| program::create(tx, y.id, "로봇과학")).unwrap();
    let cat = db.read(catalog::all).unwrap();
    let fund = |code: &str| cat.fund_types.iter().find(|f| f.code == code).unwrap().id;
    let item = |code: &str| cat.amount_items.iter().find(|f| f.code == code).unwrap().id;
    Fixture {
        year_id: y.id,
        months,
        program_coding,
        program_robot,
        fund_beneficiary: fund("beneficiary"),
        fund_free_voucher: fund("free_voucher"),
        fund_g3_support: fund("g3_support"),
        item_instructor: item("instructor"),
        item_textbook_material: item("textbook_material"),
        db,
    }
}

impl Fixture {
    pub fn entry(&self, month_seq: usize, flow: &str, kind: &str, amount: i64) -> NewEntry {
        NewEntry {
            school_year_id: self.year_id,
            fiscal_month_id: self.months[month_seq - 1],
            program_id: self.program_coding,
            student_id: None,
            fund_type_id: self.fund_beneficiary,
            amount_item_id: self.item_instructor,
            flow: flow.into(),
            entry_kind: kind.into(),
            amount,
            stage: None,
            processed_on: None,
            counterparty: None,
            upload_file_id: None,
            source_row_id: None,
            reason: None,
        }
    }
}
