-- ============================================================================
-- 스키마 v3 (P2) — 프로그램별 징수액 배분 · 대조 확인 메모
--
--   · allocation: 미배분 징수 거래(프로그램 NULL)의 금액이 어느 프로그램에 귀속되는지.
--     새 징수 거래가 아니다 — 원장 금액은 그대로이고 귀속만 표시한다.
--     합은 원 거래를 넘지 못하고 부호가 같아야 한다(트리거). 고칠 수 없고 취소만 된다.
--   · recon_note: 담당자의 확인 상태·사유. 덧붙이기만 하며 마지막 줄이 현재 상태다.
--     저장 시점의 검증 지문(fingerprint)을 함께 둬서 자료가 바뀌면 '재확인 필요'를 안다.
-- ============================================================================

CREATE TABLE allocation (
  id                INTEGER PRIMARY KEY,
  entry_id          INTEGER NOT NULL REFERENCES ledger_entry (id),
  program_id        INTEGER NOT NULL REFERENCES program (id),
  amount            INTEGER NOT NULL CHECK (typeof(amount) = 'integer' AND amount <> 0),
  method            TEXT    NOT NULL CHECK (method IN ('auto_single', 'auto_split', 'manual')),
  source_upload_id  INTEGER REFERENCES upload_file (id),
  source_row_id     INTEGER REFERENCES upload_row (id),
  status            TEXT    NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'voided')),
  confirmed         INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1)),
  reason            TEXT    NOT NULL DEFAULT '',
  created_at        TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
  voided_at         TEXT,
  void_reason       TEXT    NOT NULL DEFAULT ''
);
CREATE INDEX ix_allocation_entry   ON allocation (entry_id, status);
CREATE INDEX ix_allocation_program ON allocation (program_id, status);

-- 배분 합(절댓값)이 원 거래를 넘지 못하고, 부호가 같아야 한다.
CREATE TRIGGER trg_allocation_cap
BEFORE INSERT ON allocation
WHEN NEW.status = 'active' AND (
     (SELECT abs(coalesce(sum(a.amount), 0)) FROM allocation a WHERE a.entry_id = NEW.entry_id AND a.status = 'active')
       + abs(NEW.amount)
       > (SELECT abs(e.amount) FROM ledger_entry e WHERE e.id = NEW.entry_id)
  OR (NEW.amount > 0) <> ((SELECT e.amount FROM ledger_entry e WHERE e.id = NEW.entry_id) > 0)
)
BEGIN
  SELECT RAISE(ABORT, 'ALLOCATION_OVER');
END;

-- 배분은 고치지 않는다. 취소(status·voided_at·void_reason)만 바뀐다.
CREATE TRIGGER trg_allocation_immutable
BEFORE UPDATE ON allocation
WHEN NEW.entry_id IS NOT OLD.entry_id
  OR NEW.program_id IS NOT OLD.program_id
  OR NEW.amount IS NOT OLD.amount
  OR NEW.method IS NOT OLD.method
  OR NEW.source_upload_id IS NOT OLD.source_upload_id
  OR NEW.source_row_id IS NOT OLD.source_row_id
  OR NEW.created_at IS NOT OLD.created_at
BEGIN
  SELECT RAISE(ABORT, 'ALLOCATION_IMMUTABLE');
END;

CREATE TRIGGER trg_allocation_no_delete
BEFORE DELETE ON allocation
BEGIN
  SELECT RAISE(ABORT, 'ALLOCATION_NO_DELETE');
END;

-- 대조 확인 메모 (덧붙이기만)
CREATE TABLE recon_note (
  id               INTEGER PRIMARY KEY,
  school_year_id   INTEGER NOT NULL REFERENCES school_year (id),
  fiscal_month_id  INTEGER NOT NULL REFERENCES fiscal_month (id),
  unit_kind        TEXT    NOT NULL CHECK (unit_kind IN ('program', 'group')),
  unit_id          INTEGER NOT NULL,
  item_set         TEXT    NOT NULL,          -- instructor · textbook_material · admin · other
  status           TEXT    NOT NULL CHECK (status IN ('todo', 'investigating', 'explained')),
  reason           TEXT    NOT NULL DEFAULT '',
  fingerprint      TEXT    NOT NULL,          -- 저장 시점의 검증 지문
  created_at       TEXT    NOT NULL DEFAULT (datetime('now', 'localtime'))
);
CREATE INDEX ix_recon_note_unit ON recon_note (fiscal_month_id, unit_kind, unit_id, item_set, id);
