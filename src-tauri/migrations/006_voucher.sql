-- ============================================================================
-- 스키마 v6 (P4) — 자유수강권 관리
--
--   · voucher_policy     학년도별 지원 한도 정책. 바꾸면 이전 행을 superseded_at 로 닫고 새 행을 넣는다 (이력).
--   · voucher_exception  학생별 예외 한도. 같은 방식의 이력.
--   · voucher_target     대상자 (학생당 학년도에 한 행). 상태·날짜·전입 정보. 변경은 voucher_target_log 에 남긴다.
--   · voucher_batch      소급·일괄 재원경정 묶음. 실제 경정은 P3 의 adjustment(fund_transfer) 로 등록되고
--                        voucher_batch_item 이 잇는다. 원장에는 새 표를 쓰지 않는다.
--   · voucher_refund     환불 처리 근거 — P3 의 adjustment(refund_return) 에 '실제 수납 확인' 근거를 덧붙인다.
--   사용액·잔액은 저장하지 않고 원장(ledger_entry)에서 그때그때 계산한다. 지원액을 자동으로 확정하지 않는다.
-- ============================================================================

CREATE TABLE voucher_policy (
  id               INTEGER PRIMARY KEY,
  school_year_id   INTEGER NOT NULL REFERENCES school_year (id),
  -- annual: 연간 한도 · semester: 학기별 한도 · unlimited: 제한 없음
  basis            TEXT    NOT NULL CHECK (basis IN ('annual', 'semester', 'unlimited')),
  annual_limit     INTEGER CHECK (annual_limit IS NULL OR (typeof(annual_limit) = 'integer' AND annual_limit >= 0)),
  semester1_limit  INTEGER CHECK (semester1_limit IS NULL OR (typeof(semester1_limit) = 'integer' AND semester1_limit >= 0)),
  semester2_limit  INTEGER CHECK (semester2_limit IS NULL OR (typeof(semester2_limit) = 'integer' AND semester2_limit >= 0)),
  -- 1학기 잔액을 2학기로 이월하는가 (학기별 한도일 때만 뜻이 있다)
  carry_over       INTEGER NOT NULL DEFAULT 0 CHECK (carry_over IN (0, 1)),
  note             TEXT    NOT NULL DEFAULT '',
  created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  superseded_at    TEXT,
  CHECK (basis <> 'annual' OR annual_limit IS NOT NULL),
  CHECK (basis <> 'semester' OR (semester1_limit IS NOT NULL AND semester2_limit IS NOT NULL))
);
-- 학년도마다 현재 정책은 하나뿐이다.
CREATE UNIQUE INDEX ux_voucher_policy_current ON voucher_policy (school_year_id) WHERE superseded_at IS NULL;

CREATE TABLE voucher_exception (
  id               INTEGER PRIMARY KEY,
  school_year_id   INTEGER NOT NULL REFERENCES school_year (id),
  student_id       INTEGER NOT NULL REFERENCES student (id),
  basis            TEXT    NOT NULL CHECK (basis IN ('annual', 'semester', 'unlimited')),
  annual_limit     INTEGER CHECK (annual_limit IS NULL OR (typeof(annual_limit) = 'integer' AND annual_limit >= 0)),
  semester1_limit  INTEGER CHECK (semester1_limit IS NULL OR (typeof(semester1_limit) = 'integer' AND semester1_limit >= 0)),
  semester2_limit  INTEGER CHECK (semester2_limit IS NULL OR (typeof(semester2_limit) = 'integer' AND semester2_limit >= 0)),
  carry_over       INTEGER NOT NULL DEFAULT 0 CHECK (carry_over IN (0, 1)),
  reason           TEXT    NOT NULL CHECK (length(trim(reason)) > 0),
  created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  superseded_at    TEXT,
  CHECK (basis <> 'annual' OR annual_limit IS NOT NULL),
  CHECK (basis <> 'semester' OR (semester1_limit IS NOT NULL AND semester2_limit IS NOT NULL))
);
CREATE UNIQUE INDEX ux_voucher_exception_current ON voucher_exception (student_id) WHERE superseded_at IS NULL;

CREATE TABLE voucher_target (
  id               INTEGER PRIMARY KEY,
  school_year_id   INTEGER NOT NULL REFERENCES school_year (id),
  student_id       INTEGER NOT NULL REFERENCES student (id),
  -- pending: 확인 전 · confirmed: 대상자 확정 · ended: 지원 종료 · cancelled: 대상 취소
  status           TEXT    NOT NULL CHECK (status IN ('pending', 'confirmed', 'ended', 'cancelled')),
  confirmed_on     TEXT,                      -- 확정일
  apply_from       TEXT,                      -- 적용 시작일
  apply_to         TEXT,                      -- 적용 종료일 (지원 종료)
  transfer_in      INTEGER NOT NULL DEFAULT 0 CHECK (transfer_in IN (0, 1)),
  transfer_in_on   TEXT,                      -- 전입일
  -- 이전 학교 사용액. NULL 이면 '미입력'. 0 으로 자동 확정하지 않는다.
  prior_used       INTEGER CHECK (prior_used IS NULL OR (typeof(prior_used) = 'integer' AND prior_used >= 0)),
  prior_status     TEXT    NOT NULL DEFAULT 'unconfirmed' CHECK (prior_status IN ('unconfirmed', 'confirmed')),
  prior_note       TEXT    NOT NULL DEFAULT '',
  note             TEXT    NOT NULL DEFAULT '',
  created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  updated_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, student_id),
  CHECK (transfer_in = 1 OR transfer_in_on IS NULL)
);

CREATE TABLE voucher_target_log (
  id           INTEGER PRIMARY KEY,
  target_id    INTEGER NOT NULL REFERENCES voucher_target (id),
  action       TEXT    NOT NULL,              -- register · status · dates · prior · note
  from_status  TEXT    NOT NULL DEFAULT '',
  to_status    TEXT    NOT NULL DEFAULT '',
  detail       TEXT    NOT NULL DEFAULT '',
  at           TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX ix_voucher_target_log ON voucher_target_log (target_id);

CREATE TABLE voucher_batch (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id),
  -- retro: 소급 지원 경정 · monthly: 당월 사용액 경정
  kind            TEXT    NOT NULL CHECK (kind IN ('retro', 'monthly')),
  student_id      INTEGER REFERENCES student (id),    -- NULL 이면 여러 학생
  reason          TEXT    NOT NULL CHECK (length(trim(reason)) > 0),
  item_count      INTEGER NOT NULL CHECK (item_count > 0),
  total_amount    INTEGER NOT NULL CHECK (typeof(total_amount) = 'integer' AND total_amount > 0),
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);

CREATE TABLE voucher_batch_item (
  batch_id         INTEGER NOT NULL REFERENCES voucher_batch (id),
  adjustment_id    INTEGER NOT NULL REFERENCES adjustment (id),
  student_id       INTEGER NOT NULL REFERENCES student (id),
  fiscal_month_id  INTEGER NOT NULL REFERENCES fiscal_month (id),
  program_id       INTEGER NOT NULL REFERENCES program (id),
  amount           INTEGER NOT NULL CHECK (typeof(amount) = 'integer' AND amount > 0),
  PRIMARY KEY (batch_id, adjustment_id)
);
CREATE INDEX ix_voucher_batch_item_adj ON voucher_batch_item (adjustment_id);

CREATE TABLE voucher_refund (
  id                      INTEGER PRIMARY KEY,
  adjustment_id           INTEGER NOT NULL UNIQUE REFERENCES adjustment (id),   -- refund_return
  transfer_adjustment_id  INTEGER NOT NULL REFERENCES adjustment (id),          -- 근거 재원경정
  collected_confirmed     INTEGER NOT NULL DEFAULT 0 CHECK (collected_confirmed IN (0, 1)),
  evidence                TEXT    NOT NULL DEFAULT '',
  created_at              TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- 이력은 지우지 않는다.
CREATE TRIGGER trg_voucher_target_no_delete BEFORE DELETE ON voucher_target BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_voucher_target_log_no_delete BEFORE DELETE ON voucher_target_log BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_voucher_batch_no_delete BEFORE DELETE ON voucher_batch BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_voucher_batch_item_no_delete BEFORE DELETE ON voucher_batch_item BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
CREATE TRIGGER trg_voucher_refund_no_delete BEFORE DELETE ON voucher_refund BEGIN SELECT RAISE(ABORT, 'VOUCHER_NO_DELETE'); END;
-- 묶음과 환불 근거는 고치지 않는다.
CREATE TRIGGER trg_voucher_batch_immutable BEFORE UPDATE ON voucher_batch BEGIN SELECT RAISE(ABORT, 'VOUCHER_IMMUTABLE'); END;
CREATE TRIGGER trg_voucher_batch_item_immutable BEFORE UPDATE ON voucher_batch_item BEGIN SELECT RAISE(ABORT, 'VOUCHER_IMMUTABLE'); END;
-- 환불 근거는 수납 확인·근거만 채워진다 (연결은 바꿀 수 없다).
CREATE TRIGGER trg_voucher_refund_immutable BEFORE UPDATE ON voucher_refund
WHEN NEW.adjustment_id IS NOT OLD.adjustment_id OR NEW.transfer_adjustment_id IS NOT OLD.transfer_adjustment_id OR NEW.created_at IS NOT OLD.created_at
BEGIN SELECT RAISE(ABORT, 'VOUCHER_IMMUTABLE'); END;
