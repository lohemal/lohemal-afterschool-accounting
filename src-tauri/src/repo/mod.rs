//! SQL 계층. 함수는 `&Connection`(읽기) 또는 `&Transaction`(쓰기)을 받는다.
//! `Transaction` 은 `Connection` 으로 역참조되므로 읽기 함수는 쓰기 안에서도 쓸 수 있다.

pub mod allocation;
pub mod audit;
pub mod catalog;
pub mod fundalias;
pub mod ledger;
pub mod program;
pub mod recon;
pub mod setting;
pub mod student;
pub mod upload;
pub mod year;

#[cfg(test)]
pub(crate) mod testutil;

#[cfg(test)]
#[path = "real_tests.rs"]
mod real_tests;
