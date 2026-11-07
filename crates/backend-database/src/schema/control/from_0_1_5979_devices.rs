//! From the control schema where a device is called a device: a
//! conversation keeps the latest end of a turn the user asked for, which
//! alone marks it unread. Which output of a conversation the user asked for
//! is not stored, so each takes its read revision: nothing is unread after
//! the step.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_5979_DEVICES);

const CONTROL_FROM_0_1_5979_DEVICES: &str = "
ALTER TABLE conversations ADD COLUMN asked_end_revision INTEGER NOT NULL DEFAULT 0
  CHECK (asked_end_revision >= 0);
ALTER TABLE conversations ADD COLUMN asked_end_failed INTEGER NOT NULL DEFAULT 0
  CHECK (asked_end_failed IN (0, 1));
UPDATE conversations SET asked_end_revision = read_revision;
";
