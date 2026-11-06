//! The core database's history: each schema a core's database may hold
//! before the current one, oldest first, and the step from it to the next.
//! The schema a server's control database is split into
//! (`crates/backend-database/src/schema/control/core_at_one_core_per_user.sql`)
//! is the current one until the first change, and then the first of these.

type Shipped = super::Shipped<()>;

pub(super) const HISTORY: &[Shipped] = &[];
