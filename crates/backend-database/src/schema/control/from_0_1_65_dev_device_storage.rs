//! From the control schema where a device keeps the machine its runner
//! reports: a device keeps what Jush keeps on it, as its runner last measured,
//! and the server's settings keep how long and how much of its log it keeps.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_DEVICE_STORAGE);

const CONTROL_FROM_0_1_65_DEV_DEVICE_STORAGE: &str = "
ALTER TABLE devices ADD COLUMN storage TEXT;
ALTER TABLE server_settings ADD COLUMN log_keep_days INTEGER NOT NULL DEFAULT 14 CHECK (log_keep_days BETWEEN 1 AND 365);
ALTER TABLE server_settings ADD COLUMN log_max_bytes INTEGER NOT NULL DEFAULT 2147483648 CHECK (log_max_bytes > 0);
";
