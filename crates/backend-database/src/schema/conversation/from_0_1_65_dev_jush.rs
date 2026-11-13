//! From the conversation schema of before the product took the name Jush: a
//! child's working directory moves as the control schema's step moves a
//! conversation's (`super::super::control::from_0_1_65_dev_jush`). Its
//! device lies in the control database, so a directory under `/home/demi`
//! is taken to be a Cloud's.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONVERSATION_FROM_0_1_65_DEV_JUSH);

const CONVERSATION_FROM_0_1_65_DEV_JUSH: &str = "
UPDATE nodes SET placement_directory = '/home/jush' || substr(placement_directory, 11)
WHERE placement_directory = '/home/demi' OR placement_directory LIKE '/home/demi/%';
UPDATE nodes SET placement_directory =
  replace(replace(replace(replace(replace(placement_directory,
    '/.demi/runner/', '/.jush/runner/'), '/.demi/instances/', '/.jush/instances/'),
    '\\.demi\\runner\\', '\\.jush\\runner\\'), '\\.demi\\instances\\', '\\.jush\\instances\\'),
    '/home/jush/.demi/', '/home/jush/.jush/')
WHERE placement_directory LIKE '%.demi%';
";
