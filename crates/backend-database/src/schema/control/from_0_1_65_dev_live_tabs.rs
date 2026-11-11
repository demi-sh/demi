//! From the control schema whose browser tabs could outlast their page: a
//! work's `browser` tab is the tab its device lists, so a tab marked lost
//! with its browser (`closed`) and a tab made before its browser tab (no
//! `tab`) go, their ids retired as a removed tab's are, and the rest lose
//! the `closed` and `failure` fields.

use super::super::Migration;

pub(super) const MIGRATION: Migration = Migration::Sql(CONTROL_FROM_0_1_65_DEV_LIVE_TABS);

const CONTROL_FROM_0_1_65_DEV_LIVE_TABS: &str = "
UPDATE conversation_work SET
  document = json_set(
    document,
    '$.tabs', (
      SELECT json_group_array(json(kept))
      FROM (
        SELECT CASE json_extract(tab.value, '$.kind')
                 WHEN 'browser' THEN json_remove(tab.value, '$.data.closed', '$.data.failure')
                 ELSE tab.value
               END AS kept
        FROM json_each(document, '$.tabs') AS tab
        WHERE NOT (json_extract(tab.value, '$.kind') = 'browser'
                   AND (json_extract(tab.value, '$.data.closed') IS 1
                        OR json_type(tab.value, '$.data.tab') IS NULL))
        ORDER BY tab.key
      )
    ),
    '$.retired', (
      SELECT json_group_array(id)
      FROM (
        SELECT retired.value AS id FROM json_each(document, '$.retired') AS retired
        UNION ALL
        SELECT json_extract(tab.value, '$.id') AS id
        FROM json_each(document, '$.tabs') AS tab
        WHERE json_extract(tab.value, '$.kind') = 'browser'
          AND (json_extract(tab.value, '$.data.closed') IS 1
               OR json_type(tab.value, '$.data.tab') IS NULL)
      )
    )
  ),
  revision = revision + 1
WHERE EXISTS (
  SELECT 1 FROM json_each(document, '$.tabs') AS tab
  WHERE json_extract(tab.value, '$.kind') = 'browser'
    AND (json_type(tab.value, '$.data.closed') IS NOT NULL
         OR json_type(tab.value, '$.data.failure') IS NOT NULL
         OR json_type(tab.value, '$.data.tab') IS NULL)
);
";
