//! From the control schema whose skill sources could fail for their size: a
//! fetch has no size limit, so a source whose last fetch failed for the
//! repository's or its skills' files' size keeps that failure in the words
//! it was given, as any other failure, and fetches again on Update.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_SKILL_SIZE_FAILURES);

const CONTROL_FROM_0_1_65_DEV_SKILL_SIZE_FAILURES: &str = "
UPDATE plugin_values SET
  document = json_set(document, '$.failure.problem', json_object('code', 'failed')),
  revision = revision + 1
WHERE plugin = 'skills'
  AND json_extract(document, '$.failure.problem.code') IN ('repository_too_large', 'files_too_large');
";
