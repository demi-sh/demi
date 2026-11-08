//! From the control schema where a device keeps the machine its runner
//! reports: a conversation's tabs and the layout of their windows are its
//! work, no longer its panel. The table takes the word; its rows stay as
//! they are.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_5979_CONVERSATION_WORK);

const CONTROL_FROM_0_1_5979_CONVERSATION_WORK: &str = "
ALTER TABLE conversation_panels RENAME TO conversation_work;
";
