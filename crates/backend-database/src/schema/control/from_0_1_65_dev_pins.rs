//! From the control schema before pinned documents: a conversation's main
//! agent pins documents, which come back after each compaction.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_PINS);

const CONTROL_FROM_0_1_65_DEV_PINS: &str = "
CREATE TABLE conversation_pins (
  conversation_id TEXT NOT NULL COLLATE NOCASE REFERENCES conversations (id) ON DELETE CASCADE,
  id              INTEGER NOT NULL CHECK (id >= 1),
  device_id       TEXT NOT NULL,
  path            TEXT NOT NULL,
  pinned_at       INTEGER NOT NULL,
  text            TEXT NOT NULL,
  read_at         INTEGER NOT NULL,
  unpinned_at     INTEGER,
  unpinned_by     TEXT CHECK (unpinned_by IN ('agent', 'user')),
  told_at         INTEGER CHECK (told_at IS NULL OR unpinned_by = 'user'),
  CHECK ((unpinned_at IS NULL) = (unpinned_by IS NULL)),
  PRIMARY KEY (conversation_id, id)
) STRICT;
CREATE UNIQUE INDEX conversation_pins_pinned ON conversation_pins (conversation_id, device_id, path)
  WHERE unpinned_at IS NULL;
";
