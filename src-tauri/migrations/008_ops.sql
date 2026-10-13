-- ============================================================================
-- 스키마 v8 (P5) — 방과후이용권 대상 여부 · 학기 공백 확인
--
--   · after_voucher_target: 학생별 방과후이용권 대상 여부(미확인·대상자·비대상자·지원 종료). 사용 실적(원장)과 별개다.
--     실적이 있다는 이유로 자동 생성하지 않는다. 변경은 after_voucher_target_log 에 남는다.
--   · voucher_policy.gap_confirmed: 학기 사이 공백 달을 담당자가 확인하고 저장했다는 표시(이력).
--   원장·정정·감사 표는 건드리지 않는다.
-- ============================================================================

CREATE TABLE after_voucher_target (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id),
  student_id      INTEGER NOT NULL REFERENCES student (id),
  -- unconfirmed: 미확인 · eligible: 대상자 · ineligible: 비대상자 · ended: 지원 종료
  status          TEXT    NOT NULL CHECK (status IN ('unconfirmed', 'eligible', 'ineligible', 'ended')),
  apply_from      TEXT,
  apply_to        TEXT,
  confirmed_on    TEXT,
  evidence        TEXT    NOT NULL DEFAULT '',
  note            TEXT    NOT NULL DEFAULT '',
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  updated_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, student_id)
);

CREATE TABLE after_voucher_target_log (
  id           INTEGER PRIMARY KEY,
  target_id    INTEGER NOT NULL REFERENCES after_voucher_target (id),
  action       TEXT    NOT NULL,
  from_status  TEXT    NOT NULL DEFAULT '',
  to_status    TEXT    NOT NULL DEFAULT '',
  detail       TEXT    NOT NULL DEFAULT '',
  at           TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX ix_after_voucher_target_log ON after_voucher_target_log (target_id);

CREATE TRIGGER trg_after_voucher_target_no_delete BEFORE DELETE ON after_voucher_target BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_after_voucher_target_log_no_delete BEFORE DELETE ON after_voucher_target_log BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;

ALTER TABLE voucher_policy ADD COLUMN gap_confirmed INTEGER NOT NULL DEFAULT 0 CHECK (gap_confirmed IN (0, 1));
