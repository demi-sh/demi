//! From the control schema where a conversation keeps the latest end of a
//! turn the user asked for: a device keeps the machine its runner reports,
//! which the next hello writes.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_5979_ASKED_ENDS);

const CONTROL_FROM_0_1_5979_ASKED_ENDS: &str = "
ALTER TABLE devices ADD COLUMN hardware TEXT;
";
