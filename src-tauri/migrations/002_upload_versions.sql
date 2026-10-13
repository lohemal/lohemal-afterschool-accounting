-- ============================================================================
-- 스키마 v2 (P1) — 엑셀 업로드 · 파일 판(version) · 되돌리기 · 재원/프로그램 별칭
--
--   · 재원 통합: '초3 지원금'(g3_support)은 방과후이용권(after_voucher)과 같은 재원이다.
--     행을 지우지 않고 merged_into_id 로 접는다 → 기존 ID 와 거래가 그대로 남는다.
--   · 재원 별칭표(fund_type_alias): 엑셀 머리글·지원유형 글자 → 재원.
--   · 프로그램 그룹(program_group): '우쿨렐레&통기타' 처럼 한 지출 행에 합산된 묶음.
--   · ledger_entry 재생성: program_id · fund_type_id 를 NULL 허용(미배분 · 미확정),
--     program_group_id 추가. 외래키를 끈 채 돌리고 커밋 전에 foreign_key_check 로 확인한다.
--   · upload_file: 자료 유형 · 귀속월 · 계보(lineage) · 보관 파일명. upload_row: 정규화 행(복원 근거).
-- ============================================================================

-- ── 재원 통합 · 별칭 ─────────────────────────────────────────────────────────

ALTER TABLE fund_type ADD COLUMN merged_into_id INTEGER REFERENCES fund_type (id);

UPDATE fund_type
   SET merged_into_id = (SELECT id FROM fund_type WHERE code = 'after_voucher'),
       is_active = 0
 WHERE code = 'g3_support' AND merged_into_id IS NULL;

CREATE TABLE fund_type_alias (
  id            INTEGER PRIMARY KEY,
  fund_type_id  INTEGER NOT NULL REFERENCES fund_type (id) ON DELETE CASCADE,
  alias         TEXT    NOT NULL CHECK (length(trim(alias)) > 0),   -- 공백을 뗀 원문
  is_builtin    INTEGER NOT NULL DEFAULT 0 CHECK (is_builtin IN (0, 1)),
  created_at    TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (alias)
);

WITH a(code, alias) AS (VALUES
    ('after_voucher',  '방과후이용권'),
    ('after_voucher',  '초3지원금'),
    ('after_voucher',  '3학년지원금'),
    ('after_voucher',  '이용권'),
    ('g3_excess',      '초3수익자부담금(초과금)'),
    ('g3_excess',      '초3수익자'),
    ('g3_excess',      '3학년수익자'),
    ('g3_excess',      '초과금'),
    ('beneficiary',    '수익자'),
    ('beneficiary',    '수익자징수'),
    ('beneficiary',    '수익자징수금액'),
    ('beneficiary',    '일반'),
    ('beneficiary',    '일반수익자'),
    ('free_voucher',   '자유수강권'),
    ('free_voucher',   '자유'),
    ('custom_support', '맞춤형지원'),
    ('custom_support', '맞춤형지원(시간당40,000원)')
)
INSERT INTO fund_type_alias (fund_type_id, alias, is_builtin)
SELECT f.id, a.alias, 1 FROM a JOIN fund_type f ON f.code = a.code;

-- ── 프로그램 그룹 · 별칭 재생성 ──────────────────────────────────────────────

CREATE TABLE program_group (
  id              INTEGER PRIMARY KEY,
  school_year_id  INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
  note            TEXT    NOT NULL DEFAULT '',
  created_at      TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, name)
);

CREATE TABLE program_group_member (
  group_id    INTEGER NOT NULL REFERENCES program_group (id) ON DELETE CASCADE,
  program_id  INTEGER NOT NULL REFERENCES program (id) ON DELETE CASCADE,
  PRIMARY KEY (group_id, program_id)
);

CREATE TABLE program_alias_new (
  id                INTEGER PRIMARY KEY,
  school_year_id    INTEGER NOT NULL REFERENCES school_year (id) ON DELETE CASCADE,
  program_id        INTEGER REFERENCES program (id) ON DELETE CASCADE,
  program_group_id  INTEGER REFERENCES program_group (id) ON DELETE CASCADE,
  alias             TEXT    NOT NULL CHECK (length(trim(alias)) > 0),
  source_kind       TEXT    NOT NULL DEFAULT '',
  created_at        TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  UNIQUE (school_year_id, alias),
  CHECK ((program_id IS NULL) <> (program_group_id IS NULL))
);
INSERT INTO program_alias_new (id, school_year_id, program_id, alias, source_kind, created_at)
  SELECT id, school_year_id, program_id, alias, source_kind, created_at FROM program_alias;
DROP TABLE program_alias;
ALTER TABLE program_alias_new RENAME TO program_alias;

-- ── 회계 거래 재생성 (미배분 · 미확정 허용, 그룹 추가) ─────────────────────────

DROP TRIGGER IF EXISTS trg_ledger_immutable;
DROP TRIGGER IF EXISTS trg_ledger_no_delete;

CREATE TABLE ledger_entry_new (
  id                INTEGER PRIMARY KEY,
  school_year_id    INTEGER NOT NULL REFERENCES school_year (id),
  fiscal_month_id   INTEGER NOT NULL REFERENCES fiscal_month (id),
  program_id        INTEGER REFERENCES program (id),               -- NULL = 미배분
  program_group_id  INTEGER REFERENCES program_group (id),         -- 합산 지출 행
  student_id        INTEGER REFERENCES student (id),
  fund_type_id      INTEGER REFERENCES fund_type (id),             -- NULL = 미확정 재원
  amount_item_id    INTEGER NOT NULL REFERENCES amount_item (id),
  flow              TEXT    NOT NULL CHECK (flow IN ('collection', 'expenditure', 'transfer')),
  entry_kind        TEXT    NOT NULL CHECK (entry_kind IN (
                      'initial_collect', 'extra_collect', 'refund', 'fund_transfer',
                      'instructor_pay', 'vendor_pay', 'adjustment')),
  amount            INTEGER NOT NULL CHECK (typeof(amount) = 'integer'),
  stage             TEXT    NOT NULL DEFAULT 'actual' CHECK (stage IN ('planned', 'actual')),
  state             TEXT    NOT NULL DEFAULT 'active' CHECK (state IN ('active', 'voided', 'superseded')),
  processed_on      TEXT,
  counterparty      TEXT    NOT NULL DEFAULT '',
  transfer_group_id INTEGER,
  upload_file_id    INTEGER REFERENCES upload_file (id),
  source_row_id     INTEGER REFERENCES source_row (id),
  supersedes_id     INTEGER REFERENCES ledger_entry_new (id),
  reason            TEXT    NOT NULL DEFAULT '',
  created_at        TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  CHECK ((flow = 'transfer') = (entry_kind = 'fund_transfer')),
  CHECK (flow <> 'transfer' OR transfer_group_id IS NOT NULL),
  CHECK (program_id IS NULL OR program_group_id IS NULL)
);

INSERT INTO ledger_entry_new
  (id, school_year_id, fiscal_month_id, program_id, program_group_id, student_id, fund_type_id, amount_item_id,
   flow, entry_kind, amount, stage, state, processed_on, counterparty, transfer_group_id,
   upload_file_id, source_row_id, supersedes_id, reason, created_at)
SELECT
   id, school_year_id, fiscal_month_id, program_id, NULL, student_id, fund_type_id, amount_item_id,
   flow, entry_kind, amount, stage, state, processed_on, counterparty, transfer_group_id,
   upload_file_id, source_row_id, supersedes_id, reason, created_at
  FROM ledger_entry;

DROP TABLE ledger_entry;
ALTER TABLE ledger_entry_new RENAME TO ledger_entry;

CREATE INDEX ix_ledger_month   ON ledger_entry (fiscal_month_id, state);
CREATE INDEX ix_ledger_program ON ledger_entry (program_id, fiscal_month_id);
CREATE INDEX ix_ledger_upload  ON ledger_entry (upload_file_id, state);

CREATE TRIGGER trg_ledger_immutable
BEFORE UPDATE ON ledger_entry
WHEN NEW.amount           IS NOT OLD.amount
  OR NEW.school_year_id   IS NOT OLD.school_year_id
  OR NEW.fiscal_month_id  IS NOT OLD.fiscal_month_id
  OR NEW.program_id       IS NOT OLD.program_id
  OR NEW.program_group_id IS NOT OLD.program_group_id
  OR NEW.student_id       IS NOT OLD.student_id
  OR NEW.fund_type_id     IS NOT OLD.fund_type_id
  OR NEW.amount_item_id   IS NOT OLD.amount_item_id
  OR NEW.flow             IS NOT OLD.flow
  OR NEW.entry_kind       IS NOT OLD.entry_kind
  OR NEW.upload_file_id   IS NOT OLD.upload_file_id
  OR NEW.source_row_id    IS NOT OLD.source_row_id
  OR NEW.supersedes_id    IS NOT OLD.supersedes_id
  OR NEW.created_at       IS NOT OLD.created_at
BEGIN
  SELECT RAISE(ABORT, 'LEDGER_IMMUTABLE');
END;

CREATE TRIGGER trg_ledger_no_delete
BEFORE DELETE ON ledger_entry
BEGIN
  SELECT RAISE(ABORT, 'LEDGER_NO_DELETE');
END;

-- ── 업로드 파일 확장 ───────────────────────────────────────────────────────

-- 자료 유형: initial_collect 최초 징수 · extra_refund 추가징수·환불 · instructor_pay 강사료 ·
--            vendor_pay 교재·재료비 · roster_link 프로그램 연결·검증 · snapshot 참고 스냅샷
ALTER TABLE upload_file ADD COLUMN data_kind       TEXT    NOT NULL DEFAULT 'snapshot';
ALTER TABLE upload_file ADD COLUMN fiscal_month_id INTEGER REFERENCES fiscal_month (id);
-- 같은 자료의 판들은 계보(lineage)로 묶인다. 첫 판의 id = lineage_id.
ALTER TABLE upload_file ADD COLUMN lineage_id      INTEGER REFERENCES upload_file (id);
-- 앱 자료 폴더 uploads/ 안의 보관 파일명 (내용 주소: sha256.확장자)
ALTER TABLE upload_file ADD COLUMN stored_name     TEXT    NOT NULL DEFAULT '';
ALTER TABLE upload_file ADD COLUMN original_path   TEXT    NOT NULL DEFAULT '';
ALTER TABLE upload_file ADD COLUMN summary_json    TEXT    NOT NULL DEFAULT '{}';

CREATE INDEX ix_upload_lineage ON upload_file (lineage_id, status);
CREATE INDEX ix_upload_month   ON upload_file (fiscal_month_id, data_kind, status);

-- 정규화 행 — 파일을 해석한 결과. 거래는 여기서 만들어지고, 복원도 여기서 다시 만든다.
CREATE TABLE upload_row (
  id                INTEGER PRIMARY KEY,
  upload_file_id    INTEGER NOT NULL REFERENCES upload_file (id) ON DELETE CASCADE,
  source_row_id     INTEGER REFERENCES source_row (id),
  seq               INTEGER NOT NULL,
  sheet_name        TEXT    NOT NULL,
  row_no            INTEGER NOT NULL,
  program_text      TEXT    NOT NULL DEFAULT '',
  program_id        INTEGER REFERENCES program (id),
  program_group_id  INTEGER REFERENCES program_group (id),
  grade             INTEGER,
  class_name        TEXT    NOT NULL DEFAULT '',
  student_no        TEXT    NOT NULL DEFAULT '',
  student_name      TEXT    NOT NULL DEFAULT '',
  student_id        INTEGER REFERENCES student (id),
  fund_text         TEXT    NOT NULL DEFAULT '',
  fund_type_id      INTEGER REFERENCES fund_type (id),
  amount_item_code  TEXT    NOT NULL DEFAULT '',
  entry_kind        TEXT    NOT NULL DEFAULT '',
  flow              TEXT    NOT NULL DEFAULT '',
  amount            INTEGER NOT NULL DEFAULT 0 CHECK (typeof(amount) = 'integer'),
  counterparty      TEXT    NOT NULL DEFAULT '',
  status            TEXT    NOT NULL CHECK (status IN ('ok', 'check', 'error', 'skip')),
  included          INTEGER NOT NULL DEFAULT 0 CHECK (included IN (0, 1)),   -- 원장에 넣는 행인가
  issues_json       TEXT    NOT NULL DEFAULT '[]'
);
CREATE INDEX ix_upload_row_file ON upload_row (upload_file_id, seq);

-- 적용·복원·취소 이력 (한 파일이 여러 번 적용될 수 있다 — v1 → v2 → v1)
CREATE TABLE upload_apply (
  id                 INTEGER PRIMARY KEY,
  upload_file_id     INTEGER NOT NULL REFERENCES upload_file (id) ON DELETE CASCADE,
  action             TEXT    NOT NULL CHECK (action IN ('apply', 'restore', 'cancel', 'replace')),
  entry_count        INTEGER NOT NULL DEFAULT 0,
  collection_total   INTEGER NOT NULL DEFAULT 0,
  expenditure_total  INTEGER NOT NULL DEFAULT 0,
  note               TEXT    NOT NULL DEFAULT '',
  created_at         TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX ix_upload_apply_file ON upload_apply (upload_file_id, id);
