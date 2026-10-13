-- ============================================================================
-- 스키마 v5 (P3) — 수동 회계 정정: 재원경정 · 환불 · 추가징수
--
--   · adjustment: 담당자가 등록한 정정 거래 묶음. 원장에는 기존 ledger_entry 만 쓴다
--     (재원경정 = fund_transfer 쌍, 수강료 환불 = refund, 추가징수 = extra_collect).
--     재원경정에 따른 실제 반환 이행(refund_return)은 원장에 넣지 않는다 — 경정이 이미 수익자
--     금액을 줄였으므로 다시 차감하면 중복이다. 이력에만 남긴다.
--   · 처리 예정(planned)은 원장 거래의 stage='planned' 로 남고 집계에서 빠진다. 완료(done)로 바꾸면
--     stage='actual' 이 된다(원장 불변 트리거가 stage·processed_on 변경은 허용한다).
--   · 취소는 원장 거래를 void 하고 묶음을 cancelled 로 표시한다. 지우지 않는다.
--   · 귀속월(fiscal_month_id)과 처리일(processed_on)은 다른 칸이다. 소급 환불은 원래 달에 귀속된다.
-- ============================================================================

CREATE TABLE adjustment (
  id                     INTEGER PRIMARY KEY,
  school_year_id         INTEGER NOT NULL REFERENCES school_year (id),
  fiscal_month_id        INTEGER NOT NULL REFERENCES fiscal_month (id),
  kind                   TEXT    NOT NULL CHECK (kind IN ('fund_transfer', 'refund', 'refund_return', 'extra_collect')),
  -- student: 학생 단위 근거 있음 · program: 프로그램 단위(학생 연결 불가)
  scope                  TEXT    NOT NULL CHECK (scope IN ('student', 'program')),
  program_id             INTEGER NOT NULL REFERENCES program (id),
  student_id             INTEGER REFERENCES student (id),
  from_fund_id           INTEGER REFERENCES fund_type (id),   -- 재원경정: 변경 전
  to_fund_id             INTEGER REFERENCES fund_type (id),   -- 재원경정: 변경 후
  fund_type_id           INTEGER REFERENCES fund_type (id),   -- 환불·추가징수 재원
  amount_item_id         INTEGER NOT NULL REFERENCES amount_item (id),
  amount                 INTEGER NOT NULL CHECK (typeof(amount) = 'integer' AND amount > 0),
  decided_on             TEXT,                                -- 결정일
  processed_on           TEXT,                                -- 실제 처리일
  status                 TEXT    NOT NULL CHECK (status IN ('planned', 'done', 'cancelled')),
  reason                 TEXT    NOT NULL CHECK (length(trim(reason)) > 0),
  related_entry_id       INTEGER REFERENCES ledger_entry (id),   -- 관련 원본 거래 (최초 징수 등)
  related_adjustment_id  INTEGER REFERENCES adjustment (id),     -- 환불 이행이 가리키는 재원경정
  transfer_group_id      INTEGER,                                -- 재원경정 쌍 (ledger_entry.transfer_group_id)
  created_at             TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  cancelled_at           TEXT,
  cancel_reason          TEXT    NOT NULL DEFAULT '',
  CHECK ((kind = 'fund_transfer') = (from_fund_id IS NOT NULL AND to_fund_id IS NOT NULL)),
  CHECK (kind = 'fund_transfer' OR from_fund_id IS NULL),
  CHECK (kind <> 'refund_return' OR related_adjustment_id IS NOT NULL)
);
CREATE INDEX ix_adjustment_month ON adjustment (fiscal_month_id, status);
CREATE INDEX ix_adjustment_processed ON adjustment (processed_on);
CREATE INDEX ix_adjustment_related ON adjustment (related_entry_id);

-- 묶음이 만든 원장 거래
CREATE TABLE adjustment_entry (
  adjustment_id  INTEGER NOT NULL REFERENCES adjustment (id),
  entry_id       INTEGER NOT NULL REFERENCES ledger_entry (id),
  PRIMARY KEY (adjustment_id, entry_id)
);
CREATE INDEX ix_adjustment_entry_entry ON adjustment_entry (entry_id);

-- 상태 변경 이력
CREATE TABLE adjustment_status_log (
  id             INTEGER PRIMARY KEY,
  adjustment_id  INTEGER NOT NULL REFERENCES adjustment (id),
  from_status    TEXT    NOT NULL,
  to_status      TEXT    NOT NULL,
  note           TEXT    NOT NULL DEFAULT '',
  at             TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- 정정 묶음은 금액·분류를 고칠 수 없다. 상태·처리일·취소 정보만 바뀐다.
CREATE TRIGGER trg_adjustment_immutable
BEFORE UPDATE ON adjustment
WHEN NEW.kind IS NOT OLD.kind OR NEW.scope IS NOT OLD.scope OR NEW.program_id IS NOT OLD.program_id
  OR NEW.student_id IS NOT OLD.student_id OR NEW.from_fund_id IS NOT OLD.from_fund_id OR NEW.to_fund_id IS NOT OLD.to_fund_id
  OR NEW.fund_type_id IS NOT OLD.fund_type_id OR NEW.amount_item_id IS NOT OLD.amount_item_id OR NEW.amount IS NOT OLD.amount
  OR NEW.fiscal_month_id IS NOT OLD.fiscal_month_id OR NEW.school_year_id IS NOT OLD.school_year_id
  OR NEW.related_entry_id IS NOT OLD.related_entry_id OR NEW.related_adjustment_id IS NOT OLD.related_adjustment_id
  OR NEW.transfer_group_id IS NOT OLD.transfer_group_id OR NEW.created_at IS NOT OLD.created_at OR NEW.reason IS NOT OLD.reason
BEGIN
  SELECT RAISE(ABORT, 'ADJUSTMENT_IMMUTABLE');
END;

CREATE TRIGGER trg_adjustment_no_delete
BEFORE DELETE ON adjustment
BEGIN
  SELECT RAISE(ABORT, 'ADJUSTMENT_NO_DELETE');
END;
