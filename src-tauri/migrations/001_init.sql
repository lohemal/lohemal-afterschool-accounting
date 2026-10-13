-- ============================================================================
-- 방과후 회계 검증 매니저 — 스키마 v1 (P0)
--
-- 설계 원칙 (docs/01-P0-설계.md)
--   · 금액은 모두 원 단위 정수. CHECK(typeof(amount) = 'integer') 로 못 박는다.
--   · 회계 거래(ledger_entry)는 덧붙이기만 한다. 금액·재원·프로그램은 고칠 수 없고
--     지울 수도 없다. 틀린 거래는 취소(void)하거나 새 거래로 대체(replace)하며
--     그 연결은 ledger_correction 에 남는다. 트리거가 이를 강제한다.
--   · 월별 합계는 저장하지 않고 ledger_entry 에서 언제든 다시 계산한다.
--     월 확정(month_close)은 그 시점의 합계를 JSON 으로 떠 두는 이력일 뿐이다.
--   · 업로드 원본(upload_file · source_row)과 그로부터 만든 거래를 분리한다.
-- ============================================================================

-- ── 학년도 · 회계월 ─────────────────────────────────────────────────────────

CREATE TABLE school_year (
  id          INTEGER PRIMARY KEY,
  year        INTEGER NOT NULL UNIQUE CHECK (year BETWEEN 2000 AND 2100),
  name        TEXT    NOT NULL,                  -- '2026학년도'
  start_ym    TEXT    NOT NULL,                  -- '2026-03'
  end_ym      TEXT    NOT NULL,                  -- '2027-02'
  is_current  INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
  created_at  TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
-- 현재 학년도는 하나뿐이다.
CREATE UNIQUE INDEX ux_school_year_current ON school_year (is_current) WHERE is_current = 1;

CREATE TABLE fiscal_month (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  ym              TEXT    NOT NULL CHECK (ym GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
  seq             INTEGER NOT NULL CHECK (seq BETWEEN 1 AND 12),   -- 3월 = 1 … 2월 = 12
  status          TEXT    NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
  UNIQUE (school_year_id, ym),
  UNIQUE (school_year_id, seq)
);

-- ── 재원 · 금액 항목 (설정에서 이름을 바꾸거나 더할 수 있다) ───────────────

CREATE TABLE fund_type (
  id          INTEGER PRIMARY KEY,
  code        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  parent_id   INTEGER REFERENCES fund_type (id),   -- 초3 수익자 부담금 → 수익자
  sort_order  INTEGER NOT NULL DEFAULT 0,
  is_builtin  INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
);

INSERT INTO fund_type (code, name, parent_id, sort_order, is_builtin) VALUES
  ('beneficiary',    '수익자',                 NULL, 10, 1),
  ('free_voucher',   '자유수강권',             NULL, 20, 1),
  ('g3_support',     '초3 지원금',             NULL, 30, 1),
  ('g3_excess',      '초3 수익자 부담금(초과금)', NULL, 40, 1),
  ('after_voucher',  '방과후이용권',           NULL, 50, 1),
  ('custom_support', '맞춤형지원',             NULL, 60, 1),
  ('other',          '기타',                   NULL, 90, 1);

-- 초3 수익자 부담금은 수익자 재원의 하위 구분이다 (실무상 별도 집계).
-- 여러 행 VALUES 안의 하위 질의는 앞 행이 들어가기 전에 평가되므로 따로 잇는다.
UPDATE fund_type
   SET parent_id = (SELECT id FROM fund_type WHERE code = 'beneficiary')
 WHERE code = 'g3_excess';

CREATE TABLE amount_item (
  id          INTEGER PRIMARY KEY,
  code        TEXT    NOT NULL UNIQUE,
  name        TEXT    NOT NULL,
  sort_order  INTEGER NOT NULL DEFAULT 0,
  is_builtin  INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1))
);

-- 교재·재료비 지급 파일은 두 항목이 합산되어 있어 임의로 나누지 않는다 → 합산 항목을 따로 둔다.
INSERT INTO amount_item (code, name, sort_order, is_builtin) VALUES
  ('instructor',        '강사료',          10, 1),
  ('admin',             '수용비',          20, 1),
  ('textbook',          '교재비',          30, 1),
  ('material',          '재료비',          40, 1),
  ('textbook_material', '교재·재료비(합산)', 50, 1),
  ('other',             '기타',            90, 1);

-- ── 프로그램 · 별칭 · 학생 ──────────────────────────────────────────────────

CREATE TABLE program (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),   -- 대표 표기 ('코딩')
  note            TEXT    NOT NULL DEFAULT '',
  is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, name)
);

-- 파일마다 다른 표기('코딩A', '코딩', '우쿨렐레&통기타')를 대표 프로그램에 잇는다.
CREATE TABLE program_alias (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  program_id      INTEGER NOT NULL REFERENCES program (id) ON DELETE CASCADE,
  alias           TEXT    NOT NULL CHECK (length(trim(alias)) > 0),
  source_kind     TEXT    NOT NULL DEFAULT '',   -- 어느 양식에서 온 표기인지 (비워도 됨)
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, alias)
);

CREATE TABLE student (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  grade           INTEGER NOT NULL CHECK (grade BETWEEN 1 AND 6),
  class_name      TEXT    NOT NULL DEFAULT '',
  student_no      TEXT    NOT NULL DEFAULT '',
  name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
  identity_key    TEXT    NOT NULL,   -- '학년|반|번호|이름' 정규화 키
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, identity_key)
);

-- ── 업로드 파일 · 원본 행 ───────────────────────────────────────────────────

CREATE TABLE upload_file (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  file_name       TEXT    NOT NULL,
  sha256          TEXT    NOT NULL CHECK (length(sha256) = 64),
  byte_size       INTEGER NOT NULL CHECK (byte_size >= 0),
  format_code     TEXT    NOT NULL DEFAULT 'unknown',   -- 양식 식별자 (P1 에서 채움)
  version_no      INTEGER NOT NULL DEFAULT 1 CHECK (version_no >= 1),
  status          TEXT    NOT NULL DEFAULT 'registered'
                  CHECK (status IN ('registered', 'previewed', 'applied', 'cancelled', 'replaced')),
  replaced_by_id  INTEGER REFERENCES upload_file (id),
  note            TEXT    NOT NULL DEFAULT '',
  uploaded_at     TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  applied_at      TEXT,
  cancelled_at    TEXT
);
-- 같은 내용의 파일은 한 학년도에 한 번만 살아 있다. 취소·대체된 것은 다시 올릴 수 있다.
CREATE UNIQUE INDEX ux_upload_file_live_sha
  ON upload_file (school_year_id, sha256)
  WHERE status NOT IN ('cancelled', 'replaced');

CREATE TABLE source_row (
  id              INTEGER PRIMARY KEY,
  upload_file_id  INTEGER NOT NULL REFERENCES upload_file (id) ON DELETE CASCADE,
  sheet_name      TEXT    NOT NULL,
  row_no          INTEGER NOT NULL CHECK (row_no >= 1),
  cells_json      TEXT    NOT NULL,   -- 셀 원문(JSON 배열). 해석 전 값을 그대로 보존
  row_hash        TEXT    NOT NULL,
  UNIQUE (upload_file_id, sheet_name, row_no)
);

-- ── 회계 거래 ──────────────────────────────────────────────────────────────

CREATE TABLE ledger_entry (
  id                INTEGER PRIMARY KEY,
  school_year_id    INTEGER NOT NULL REFERENCES school_year (id),
  fiscal_month_id   INTEGER NOT NULL REFERENCES fiscal_month (id),   -- 귀속월
  program_id        INTEGER NOT NULL REFERENCES program (id),
  student_id        INTEGER REFERENCES student (id),                 -- 필요한 거래만
  fund_type_id      INTEGER NOT NULL REFERENCES fund_type (id),
  amount_item_id    INTEGER NOT NULL REFERENCES amount_item (id),
  flow              TEXT    NOT NULL CHECK (flow IN ('collection', 'expenditure', 'transfer')),
  entry_kind        TEXT    NOT NULL CHECK (entry_kind IN (
                      'initial_collect',   -- 최초 징수
                      'extra_collect',     -- 추가 징수
                      'refund',            -- 환불 (음수 징수)
                      'fund_transfer',     -- 재원 간 이동 (과목경정) — 쌍으로 합이 0
                      'instructor_pay',    -- 강사료 지급
                      'vendor_pay',        -- 교재·재료비 지급
                      'adjustment'         -- 정정 거래
                    )),
  -- 원 단위 정수, 부호 있음. 징수 흐름에서 환불은 음수, 지출 흐름은 양수가 지출.
  amount            INTEGER NOT NULL CHECK (typeof(amount) = 'integer'),
  stage             TEXT    NOT NULL DEFAULT 'actual' CHECK (stage IN ('planned', 'actual')),
  state             TEXT    NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'voided', 'superseded')),
  processed_on      TEXT,   -- 실제 처리일 (YYYY-MM-DD). 소급 반영 시 귀속월과 다를 수 있다
  counterparty      TEXT    NOT NULL DEFAULT '',   -- 강사명·업체명
  transfer_group_id INTEGER,                        -- 재원 이동 쌍 묶음
  upload_file_id    INTEGER REFERENCES upload_file (id),
  source_row_id     INTEGER REFERENCES source_row (id),
  supersedes_id     INTEGER REFERENCES ledger_entry (id),   -- 이 거래가 대체한 원 거래
  reason            TEXT    NOT NULL DEFAULT '',
  created_at        TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  CHECK ((flow = 'transfer') = (entry_kind = 'fund_transfer')),
  CHECK (flow <> 'transfer' OR transfer_group_id IS NOT NULL)
);

CREATE INDEX ix_ledger_month   ON ledger_entry (fiscal_month_id, state);
CREATE INDEX ix_ledger_program ON ledger_entry (program_id, fiscal_month_id);
CREATE INDEX ix_ledger_upload  ON ledger_entry (upload_file_id);

-- 거래는 덧붙이기만 한다. 금액·분류·출처는 고칠 수 없다 (state·stage·processed_on 만 바뀐다).
CREATE TRIGGER trg_ledger_immutable
BEFORE UPDATE ON ledger_entry
WHEN NEW.amount          IS NOT OLD.amount
  OR NEW.school_year_id  IS NOT OLD.school_year_id
  OR NEW.fiscal_month_id IS NOT OLD.fiscal_month_id
  OR NEW.program_id      IS NOT OLD.program_id
  OR NEW.student_id      IS NOT OLD.student_id
  OR NEW.fund_type_id    IS NOT OLD.fund_type_id
  OR NEW.amount_item_id  IS NOT OLD.amount_item_id
  OR NEW.flow            IS NOT OLD.flow
  OR NEW.entry_kind      IS NOT OLD.entry_kind
  OR NEW.upload_file_id  IS NOT OLD.upload_file_id
  OR NEW.source_row_id   IS NOT OLD.source_row_id
  OR NEW.supersedes_id   IS NOT OLD.supersedes_id
  OR NEW.created_at      IS NOT OLD.created_at
BEGIN
  SELECT RAISE(ABORT, 'LEDGER_IMMUTABLE');
END;

-- 거래는 지우지 않는다. 취소·대체만 한다.
CREATE TRIGGER trg_ledger_no_delete
BEFORE DELETE ON ledger_entry
BEGIN
  SELECT RAISE(ABORT, 'LEDGER_NO_DELETE');
END;

-- 정정·취소 이력. 원 거래 → (대체 거래) 연결과 사유를 남긴다.
CREATE TABLE ledger_correction (
  id                    INTEGER PRIMARY KEY,
  entry_id              INTEGER NOT NULL REFERENCES ledger_entry (id),
  replacement_entry_id  INTEGER REFERENCES ledger_entry (id),
  action                TEXT    NOT NULL CHECK (action IN ('void', 'replace')),
  -- input_error: 프로그램 입력 오류 (실제 회계는 정상) / accounting_error: 에듀파인 처리 자체의 오류
  category              TEXT    NOT NULL CHECK (category IN ('input_error', 'accounting_error')),
  reason                TEXT    NOT NULL CHECK (length(trim(reason)) > 0),
  -- 회계 처리 오류는 후속 징수·환불이 실제로 끝났는지 따로 관리한다.
  completion            TEXT    NOT NULL DEFAULT 'none' CHECK (completion IN ('none', 'pending', 'done')),
  completed_on          TEXT,
  created_at            TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  CHECK ((action = 'replace') = (replacement_entry_id IS NOT NULL))
);

CREATE INDEX ix_correction_entry ON ledger_correction (entry_id);

-- ── 월별 정산 확정 이력 ─────────────────────────────────────────────────────

CREATE TABLE month_close (
  id               INTEGER PRIMARY KEY,
  fiscal_month_id  INTEGER NOT NULL REFERENCES fiscal_month (id) ON DELETE CASCADE,
  action           TEXT    NOT NULL CHECK (action IN ('close', 'reopen')),
  totals_json      TEXT    NOT NULL DEFAULT '{}',   -- 확정 시점 합계 스냅샷
  note             TEXT    NOT NULL DEFAULT '',
  created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);

-- ── 변경 이력 · 설정 ───────────────────────────────────────────────────────

CREATE TABLE audit_log (
  id            INTEGER PRIMARY KEY,
  at            TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  kind          TEXT    NOT NULL,            -- 'year.create' 등
  target_table  TEXT    NOT NULL DEFAULT '',
  target_id     INTEGER,
  summary       TEXT    NOT NULL,            -- 사람이 읽는 한 줄 (개인정보 없음)
  detail_json   TEXT    NOT NULL DEFAULT '{}'
);

CREATE TABLE app_setting (
  key    TEXT PRIMARY KEY,
  value  TEXT NOT NULL
);
