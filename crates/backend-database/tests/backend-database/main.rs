//! The databases' migrations: the crate's test binary. Every step of each
//! database's history, taken from an empty database of its schema, must
//! leave the database a new one of the current schema would be, so that a
//! server that upgraded and one installed afresh hold the same tables.

use std::path::{Path, PathBuf};

use demi_backend_database::DatabaseKind;
use demi_backend_database::schema::ControlSteps;
use demi_backend_database::testing::{current, history, migrate, migrate_control, migrate_control_to, version_of};
use demi_web_api_protocol::ids::ProviderId;
use rusqlite::Connection;

/// The last control schema that held every user's records, which the tests
/// of the steps before it read.
const SHARED: &str = include_str!("../../src/schema/control/from_0_1_65_one_core_per_user.sql");

/// The schema of the core's database the split of `SHARED` writes.
const CORE_AT_SPLIT: &str = include_str!("../../src/schema/control/core_at_one_core_per_user.sql");

#[test]
fn every_step_of_the_control_history_leads_to_the_current_schema() {
    every_step_leads_to_the_current_schema(DatabaseKind::Control);
}

#[test]
fn every_step_of_the_conversation_history_leads_to_the_current_schema() {
    every_step_leads_to_the_current_schema(DatabaseKind::Conversation);
}

#[test]
fn every_step_of_the_core_history_leads_to_the_current_schema() {
    every_step_leads_to_the_current_schema(DatabaseKind::Core);
}

#[test]
fn the_core_history_holds_the_schema_the_split_writes() {
    let written = version_of(CORE_AT_SPLIT);
    let known = version_of(current(DatabaseKind::Core)) == written
        || history(DatabaseKind::Core).into_iter().any(|sql| version_of(sql) == written);
    assert!(known, "a core the split wrote, of version {written}, has no step to the current core schema");
}

#[test]
fn the_split_gives_each_user_a_core_and_moves_their_files_there() {
    /// The data directory, and no vault: the split seals nothing again.
    struct Directory(PathBuf);

    impl ControlSteps for Directory {
        fn data_directory(&self) -> &Path {
            &self.0
        }

        fn provider_config(&self, id: &ProviderId, _: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            Err(format!("entry {id} is read by no step here"))
        }

        fn schedule_secret(&self, id: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            Err(format!("schedule {id} is read by no step here"))
        }

        fn provider_endpoint_on_device(
            &self,
            id: &ProviderId,
            _: &[u8],
            _: &dyn Fn(&str) -> bool,
        ) -> Result<Option<(String, Vec<u8>)>, String> {
            Err(format!("entry {id} is read by no step here"))
        }

        fn provider_config_format_3(&self, id: &ProviderId, _: &[u8]) -> Result<Option<Vec<u8>>, String> {
            Err(format!("entry {id} is read by no step here"))
        }
    }

    let data = tempfile::tempdir().unwrap();
    let databases = tempfile::tempdir().unwrap();
    let before = databases.path().join("before.sqlite");
    let connection = Connection::open(&before).unwrap();
    connection.execute_batch(SHARED).unwrap();
    connection.pragma_update(None, "user_version", version_of(SHARED)).unwrap();
    connection
        .execute_batch(
            "INSERT INTO users (id, email, nickname, password_hash, role, created_at) VALUES
               ('alex', 'alex@example.com', 'Alex', 'x', 'master', 1),
               ('sam', 'sam@example.com', 'Sam', 'x', 'user', 1);
             INSERT INTO workspaces (id, user_id, name, icon, color, sort_order, created_at) VALUES
               ('home-a', 'alex', NULL, 'house', 'blue', 0, 1),
               ('home-s', 'sam', NULL, 'house', 'blue', 0, 1);
             INSERT INTO devices (id, user_id, kind, name, platform, token_hash, claimed_at) VALUES
               ('laptop', 'alex', 'user', 'Alex''s MacBook', 'macos', 'hash-a', 1),
               ('desktop', 'sam', 'user', 'Sam''s PC', 'windows', 'hash-s', 1);
             INSERT INTO projects (id, user_id, device_id, path, sort_order, created_at, workspace_id) VALUES
               ('site', 'alex', 'laptop', '/Users/alex/site', 0, 1, 'home-a');
             INSERT INTO conversations (id, user_id, title, title_origin, pinned, sort_order, read_revision,
               target_kind, context_version, created_at, updated_at, workspace_id) VALUES
               ('draft', 'alex', 'Draft the release notes', 'user', 0, 1, 0, 'cloud', 1, 1, 1, 'home-a'),
               ('Plan', 'alex', 'Plan the launch', 'user', 0, 2, 0, 'cloud', 1, 1, 1, 'home-a'),
               ('budget', 'sam', 'Check the budget', 'user', 0, 1, 0, 'cloud', 1, 1, 1, 'home-s');
             INSERT INTO conversation_deletions (id, user_id, deleted_at) VALUES ('gone', 'alex', 2);
             INSERT INTO providers (id, owner_user_id, provider_type, credential_kind, label, config, created_at) VALUES
               ('codex', 'alex', 'codex', 'subscription', 'Codex', NULL, 1),
               ('claude', 'sam', 'claude', 'subscription', 'Claude', NULL, 1);
             INSERT INTO provider_share_keys (token_hash, id, provider_id, created_by, created_at, used_by, used_at) VALUES
               ('key-hash', 'key', 'codex', 'alex', 1, 'sam', 2);
             INSERT INTO schedules (id, conversation_id, name, trigger, time_zone, if_busy, prompt, paused, since,
               created_at, updated_at) VALUES
               ('daily', 'draft', 'Daily summary', '{}', 'UTC', 'queue', 'Sum up the day', 0, 1, 1, 1),
               ('weekly', 'budget', 'Weekly check', '{}', 'UTC', 'queue', 'Check the week', 0, 1, 1, 1);
             INSERT INTO saved_messages (id, conversation_id, project_id, saved_at, document) VALUES
               ('note', 'draft', NULL, 1, '{}'),
               ('pinned', NULL, 'site', 1, '{}');
             INSERT INTO usage_ledger (id, user_id, conversation_id, provider_id, model_id, ended_at) VALUES
               ('call-a', 'alex', 'gone', 'codex', 'gpt', 1),
               ('call-s', 'sam', 'budget', 'claude', 'opus', 1);
             INSERT INTO usage_costs (conversation_id, input_tokens, cache_write_tokens, cache_read_tokens,
               output_tokens, calls) VALUES ('gone', 1, 0, 0, 1, 1), ('budget', 1, 0, 0, 1, 1);",
        )
        .unwrap();
    drop(connection);
    // Each database holds its own name, so a moved file shows where it came
    // from; the stray one belongs to no record.
    for file in [
        "conversations/draft.sqlite",
        "conversations/draft.sqlite-wal",
        "conversations/plan.sqlite",
        "conversations/gone.sqlite",
        "conversations/budget.sqlite",
        "conversations/stray.sqlite",
        "search/alex.sqlite",
        "search/sam.sqlite",
    ] {
        let path = data.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, file).unwrap();
    }
    let wanted: Vec<String> = [
        "control device_owners desktop sam hash-s",
        "control device_owners laptop alex hash-a",
        "control provider_share_keys key alex codex",
        "control schedule_owners daily alex",
        "control schedule_owners weekly sam",
        "file conversations/stray.sqlite = conversations/stray.sqlite",
        "file users/alex/conversations/draft.sqlite = conversations/draft.sqlite",
        "file users/alex/conversations/draft.sqlite-wal = conversations/draft.sqlite-wal",
        "file users/alex/conversations/gone.sqlite = conversations/gone.sqlite",
        "file users/alex/conversations/plan.sqlite = conversations/plan.sqlite",
        "file users/alex/core.sqlite",
        "file users/alex/search.sqlite = search/alex.sqlite",
        "file users/sam/conversations/budget.sqlite = conversations/budget.sqlite",
        "file users/sam/core.sqlite",
        "file users/sam/search.sqlite = search/sam.sqlite",
        "users/alex conversation_deletions gone",
        "users/alex conversations Plan",
        "users/alex conversations draft",
        "users/alex devices laptop",
        "users/alex projects site",
        "users/alex providers codex",
        "users/alex saved_messages note",
        "users/alex saved_messages pinned",
        "users/alex schedules daily",
        "users/alex usage_costs gone",
        "users/alex usage_ledger call-a",
        "users/alex workspaces home-a",
        "users/sam conversations budget",
        "users/sam devices desktop",
        "users/sam providers claude",
        "users/sam schedules weekly",
        "users/sam usage_costs budget",
        "users/sam usage_ledger call-s",
        "users/sam workspaces home-s",
    ]
    .map(str::to_owned)
    .to_vec();
    let steps = Directory(data.path().to_owned());
    // The second run is a start after one that stopped before the control
    // database committed: the files moved, and a core was half written.
    for run in ["first", "again"] {
        let control = databases.path().join(format!("{run}.sqlite"));
        std::fs::copy(&before, &control).unwrap();
        let mut connection = Connection::open(&control).unwrap();
        migrate_control(&mut connection, &steps).unwrap();
        assert_eq!(split(&connection, data.path()), wanted, "{run} run");
        std::fs::write(data.path().join("users/alex/core.sqlite.writing"), "half").unwrap();
    }
}

#[test]
fn the_grants_and_requests_of_the_other_conversations_go() {
    let shipped = include_str!("../../src/schema/control/from_0_1_5960.sql");
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(shipped).unwrap();
    connection.pragma_update(None, "user_version", version_of(shipped)).unwrap();
    // The rows need no conversation of their own.
    connection.pragma_update(None, "foreign_keys", false).unwrap();
    connection
        .execute_batch(
            "INSERT INTO permission_grants VALUES ('c1', 'conversation.use', 1), ('c1', 'credentials.use', 1);
             INSERT INTO permission_requests (id, conversation_id, kind, categories, command, node_id, created_at, decision, decided_at) VALUES
               ('waits', 'c1', 'permission', '[\"conversation.use\"]', 'demi conversation list', 'n', 1, NULL, NULL),
               ('reached', 'c1', 'permission', '[\"conversation.use\"]', 'demi conversation send', 'n', 1, 'reached', 2),
               ('other', 'c1', 'permission', '[\"credentials.use\"]', 'demi credentials list', 'n', 1, NULL, NULL);",
        )
        .unwrap();
    migrate_control_to(&mut connection, SHARED, None).unwrap();
    let grants = query(&connection, "SELECT category FROM permission_grants", |row| row.get::<_, String>(0));
    let requests = query(&connection, "SELECT id FROM permission_requests", |row| row.get::<_, String>(0));
    assert_eq!((grants, requests), (vec!["credentials.use".to_owned()], vec!["other".to_owned()]));
}

#[test]
fn an_archived_conversation_takes_its_last_activity_as_its_archive_time() {
    let shipped = include_str!("../../src/schema/control/from_0_1_5979_port_forward_routes.sql");
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(shipped).unwrap();
    connection.pragma_update(None, "user_version", version_of(shipped)).unwrap();
    // The rows need no owner of their own.
    connection.pragma_update(None, "foreign_keys", false).unwrap();
    connection
        .execute_batch(
            "INSERT INTO conversations (id, user_id, workspace_id, title, title_origin, archived, pinned, sort_order, read_revision,
               target_kind, context_version, created_at, updated_at) VALUES
               ('active', 'u1', 'w1', 'Draft the release notes', 'user', 0, 0, 1, 0, 'cloud', 1, 1, 10),
               ('archived', 'u1', 'w1', 'Plan the launch', 'user', 1, 0, 2, 0, 'cloud', 1, 1, 20);",
        )
        .unwrap();
    migrate_control_to(&mut connection, SHARED, None).unwrap();
    let times = query(&connection, "SELECT id, archived_at FROM conversations ORDER BY id", |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
    });
    assert_eq!(times, vec![("active".to_owned(), None), ("archived".to_owned(), Some(20))]);
}

#[test]
fn a_conversations_work_loses_its_widget_tabs_and_keeps_the_others() {
    let shipped = include_str!("../../src/schema/control/from_0_1_5979.sql");
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(shipped).unwrap();
    connection.pragma_update(None, "user_version", version_of(shipped)).unwrap();
    // The rows need no conversation of their own.
    connection.pragma_update(None, "foreign_keys", false).unwrap();
    connection
        .execute_batch(
            r#"INSERT INTO conversation_panels VALUES
                 ('c1', 4, '{"tabs":[{"id":"t1","kind":"file","data":{"path":"a.md"}},{"id":"t2","kind":"widget","data":{"id":"w1","title":"Orders"}}],"retired":["t0"]}', 1),
                 ('c2', 2, '{"tabs":[{"id":"t1","kind":"file","data":{}}],"retired":[]}', 1);
               INSERT INTO shares (token, user_id, conversation_id, kind, block_id, title, taken_at) VALUES
                 ('s1', 'u1', 'c1', 'answer', 'b1', 'Orders', 1);"#,
        )
        .unwrap();
    migrate_control_to(&mut connection, SHARED, None).unwrap();
    let works = query(&connection, "SELECT conversation_id, revision, document FROM conversation_work ORDER BY 1", |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))
    });
    assert_eq!(
        works,
        vec![
            ("c1".to_owned(), 5, r#"{"tabs":[{"id":"t1","kind":"file","data":{"path":"a.md"}}],"retired":["t0","t2"]}"#.to_owned()),
            ("c2".to_owned(), 2, r#"{"tabs":[{"id":"t1","kind":"file","data":{}}],"retired":[]}"#.to_owned()),
        ]
    );
    let shares = query(&connection, "SELECT token, kind, block_id FROM shares", |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    });
    assert_eq!(shares, vec![("s1".to_owned(), "answer".to_owned(), "b1".to_owned())]);
}

#[test]
fn a_widget_keeps_its_title_in_place_of_its_file_name() {
    let shipped = include_str!("../../src/schema/control/from_0_1_5979.sql");
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(shipped).unwrap();
    connection.pragma_update(None, "user_version", version_of(shipped)).unwrap();
    // The rows need no user of their own.
    connection.pragma_update(None, "foreign_keys", false).unwrap();
    connection
        .execute_batch(
            r#"INSERT INTO plugin_values VALUES
                 ('u1', 'widget', 'w1', '{"number":1,"name":"Orders per week.html","frame":{}}', 2, '[]'),
                 ('u1', 'widget', 'w2', '{"number":2,"name":"widget-w2.html","frame":{}}', 1, '[]'),
                 ('u1', 'widget', 'w3', '{"number":3,"name":"widget-w2 sales.html","frame":{}}', 1, '[]'),
                 ('u1', 'widget', 'w4', '{"number":4,"name":"x","frame":{}}', 1, '[]'),
                 ('u1', 'widget', 'count', '2', 3, '[]'),
                 ('u1', 'web', 'w1', '{"name":"Kept.html"}', 1, '[]');"#,
        )
        .unwrap();
    migrate_control_to(&mut connection, SHARED, None).unwrap();
    let values = query(&connection, "SELECT plugin, key, document, revision FROM plugin_values ORDER BY 1, 2", |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?))
    });
    let value = |plugin: &str, key: &str, document: &str, revision: i64| {
        (plugin.to_owned(), key.to_owned(), document.to_owned(), revision)
    };
    assert_eq!(
        values,
        vec![
            value("web", "w1", r#"{"name":"Kept.html"}"#, 1),
            value("widget", "count", "2", 3),
            value("widget", "w1", r#"{"number":1,"frame":{},"title":"Orders per week"}"#, 3),
            value("widget", "w2", r#"{"number":2,"frame":{},"title":null}"#, 2),
            value("widget", "w3", r#"{"number":3,"frame":{},"title":"widget-w2 sales"}"#, 2),
            value("widget", "w4", r#"{"number":4,"frame":{},"title":null}"#, 2),
        ]
    );
}

#[test]
fn each_configuration_of_format_2_is_sealed_again_in_format_3() {
    /// A vault whose sealed values are their format's name: it seals one of
    /// format 2 again as format 3, leaves one of format 3, and cannot read
    /// any other.
    struct Formats;

    impl ControlSteps for Formats {
        fn data_directory(&self) -> &std::path::Path {
            std::path::Path::new("test")
        }

        fn provider_config(&self, id: &ProviderId, _: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            Err(format!("entry {id} is no configuration of format 1"))
        }

        fn schedule_secret(&self, id: &str, _: &[u8]) -> Result<Vec<u8>, String> {
            Err(format!("schedule {id} has no secret here"))
        }

        fn provider_endpoint_on_device(
            &self,
            id: &ProviderId,
            _: &[u8],
            _: &dyn Fn(&str) -> bool,
        ) -> Result<Option<(String, Vec<u8>)>, String> {
            Err(format!("entry {id} is read by no earlier step here"))
        }

        fn provider_config_format_3(&self, id: &ProviderId, sealed: &[u8]) -> Result<Option<Vec<u8>>, String> {
            match sealed {
                b"format 2" => Ok(Some(b"format 3".to_vec())),
                b"format 3" => Ok(None),
                _ => Err(format!("entry {id} does not open")),
            }
        }
    }

    let shipped = include_str!("../../src/schema/control/from_0_1_5979_reply_block_shares.sql");
    let mut connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(shipped).unwrap();
    connection.pragma_update(None, "user_version", version_of(shipped)).unwrap();
    connection
        .execute_batch(
            "INSERT INTO users (id, email, nickname, password_hash, role, created_at) VALUES ('alex', 'alex@example.com', 'Alex', 'x', 'master', 1);
             INSERT INTO providers (id, owner_user_id, provider_type, credential_kind, label, config, created_at) VALUES
               ('compatible', 'alex', 'openai', 'api_key', 'Compatible API', CAST('format 2' AS BLOB), 1),
               ('resealed', 'alex', 'openai', 'api_key', 'xAI', CAST('format 3' AS BLOB), 1),
               ('subscription', 'alex', 'codex', 'subscription', 'Codex', NULL, 1);",
        )
        .unwrap();
    migrate_control_to(&mut connection, SHARED, Some(&Formats)).unwrap();
    let configs = query(&connection, "SELECT id, CAST(config AS TEXT) FROM providers ORDER BY id", |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
    });
    let formats = |id: &str, config: Option<&str>| (id.to_owned(), config.map(str::to_owned));
    assert_eq!(
        configs,
        vec![formats("compatible", Some("format 3")), formats("resealed", Some("format 3")), formats("subscription", None)]
    );
}

/// What the split left, one fact a line, sorted: the control database's
/// routing rows, each file below `data` with what it holds, and the records
/// of each user's core.
fn split(control: &Connection, data: &Path) -> Vec<String> {
    let mut facts = Vec::new();
    for (table, columns) in [
        ("device_owners", "device_id, user_id, token_hash"),
        ("schedule_owners", "schedule_id, user_id"),
        ("provider_share_keys", "id, owner_id, provider_id"),
    ] {
        let rows = query(control, &format!("SELECT {columns} FROM {table}"), |row| {
            (0..columns.split(", ").count()).map(|index| row.get::<_, String>(index)).collect::<rusqlite::Result<Vec<_>>>()
        });
        facts.extend(rows.into_iter().map(|row| format!("control {table} {}", row.join(" "))));
    }
    let left = query(
        control,
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('devices', 'conversations', 'providers')",
        |row| row.get::<_, String>(0),
    );
    facts.extend(left.into_iter().map(|table| format!("control still holds {table}")));
    let mut files = Vec::new();
    below(data, data, &mut files);
    for file in files {
        let name = file.strip_prefix(data).unwrap().display().to_string();
        if name.ends_with("core.sqlite") {
            let core = Connection::open(&file).unwrap();
            let user = name.trim_end_matches("/core.sqlite");
            for (table, column) in [
                ("workspaces", "id"),
                ("devices", "id"),
                ("projects", "id"),
                ("conversations", "id"),
                ("conversation_deletions", "id"),
                ("providers", "id"),
                ("schedules", "id"),
                ("saved_messages", "id"),
                ("usage_ledger", "id"),
                ("usage_costs", "conversation_id"),
            ] {
                let ids = query(&core, &format!("SELECT {column} FROM {table}"), |row| row.get::<_, String>(0));
                facts.extend(ids.into_iter().map(|id| format!("{user} {table} {id}")));
            }
            facts.push(format!("file {name}"));
        } else {
            facts.push(format!("file {name} = {}", std::fs::read_to_string(&file).unwrap()));
        }
    }
    facts.sort();
    facts
}

/// Every file below `directory`, `root` included.
fn below(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            below(root, &path, files);
        } else {
            files.push(path);
        }
    }
}

fn every_step_leads_to_the_current_schema(kind: DatabaseKind) {
    let fresh = Connection::open_in_memory().unwrap();
    fresh.execute_batch(current(kind)).unwrap();
    let wanted = shape(&fresh);
    let mut failures = Vec::new();
    for (index, sql) in history(kind).into_iter().enumerate() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.pragma_update(None, "foreign_keys", true).unwrap();
        connection.execute_batch(sql).unwrap();
        connection.pragma_update(None, "user_version", version_of(sql)).unwrap();
        if let Err(error) = migrate(kind, &mut connection) {
            failures.push(format!("step {index} ({}): {error}", version_of(sql)));
            continue;
        }
        let got = shape(&connection);
        let missing: Vec<&String> = wanted.iter().filter(|line| !got.contains(line)).collect();
        let extra: Vec<&String> = got.iter().filter(|line| !wanted.contains(line)).collect();
        if !missing.is_empty() || !extra.is_empty() {
            failures.push(format!(
                "step {index} ({}) differs from a new database\n  lacks: {missing:#?}\n  has besides: {extra:#?}",
                version_of(sql)
            ));
        }
    }
    assert!(failures.is_empty(), "{kind:?}:\n{}", failures.join("\n"));
}

/// What SQLite reports of the database's schema, one fact a line: each
/// table's columns, keys and `STRICT`, each index with its columns'
/// collations, each trigger and view. A `CHECK`, and the collation of a
/// column no index covers, live only in the table's text, which differs
/// between a table a migration altered and one made new, so they are left
/// out.
fn shape(connection: &Connection) -> Vec<String> {
    let mut lines = Vec::new();
    let objects: Vec<(String, String, String, Option<String>)> = query(
        connection,
        "SELECT type, name, tbl_name, sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    );
    for (kind, name, table, sql) in objects {
        match kind.as_str() {
            "table" => {
                let strict: bool = connection
                    .query_row("SELECT strict FROM pragma_table_list WHERE name = ?1", [&name], |row| row.get(0))
                    .unwrap();
                lines.push(format!("table {name} strict={strict}"));
                lines.extend(query(
                    connection,
                    &format!("SELECT name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo('{name}')"),
                    |row| {
                        Ok(format!(
                            "table {name} column {} {} not_null={} default={:?} key={} hidden={}",
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, bool>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                        ))
                    },
                ));
                // A key of several columns is one line, its columns in order.
                lines.extend(query(
                    connection,
                    &format!(
                        "SELECT \"table\", group_concat(\"from\", ',' ORDER BY seq), group_concat(ifnull(\"to\", '?'), ',' ORDER BY seq),
                           on_update, on_delete, \"match\"
                         FROM pragma_foreign_key_list('{name}') GROUP BY id"
                    ),
                    |row| {
                        Ok(format!(
                            "table {name} key ({}) references {} ({}) on_update={} on_delete={} match={}",
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    },
                ));
                // Each index with its columns' order and collations, which
                // a column's own collation gives an index on it. One that
                // SQLite makes for a table's own constraint has no text and
                // a name of its choosing, so it is named by its origin.
                for (index, unique, origin) in query(
                    connection,
                    &format!("SELECT name, \"unique\", origin FROM pragma_index_list('{name}')"),
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, row.get::<_, String>(2)?)),
                ) {
                    let columns = query(
                        connection,
                        &format!("SELECT name, \"desc\", coll FROM pragma_index_xinfo('{index}') WHERE key ORDER BY seqno"),
                        |row| Ok(format!("{:?} desc={} {}", row.get::<_, Option<String>>(0)?, row.get::<_, bool>(1)?, row.get::<_, String>(2)?)),
                    );
                    let named = if origin == "c" { index } else { format!("({origin})") };
                    lines.push(format!("table {name} index {named} unique={unique} on {columns:?}"));
                }
            }
            _ => lines.push(format!("{kind} {name} on {table}: {}", normalized(sql.as_deref().unwrap_or("")))),
        }
    }
    lines.sort();
    lines
}

/// `sql` with its whitespace and its identifiers' quotes made uniform, as
/// a table's rename leaves the statements that name it; a string literal
/// stays as it is.
fn normalized(sql: &str) -> String {
    let (mut out, mut literal, mut space) = (String::new(), false, false);
    for character in sql.chars() {
        if literal {
            out.push(character);
            literal = character != '\'';
            continue;
        }
        match character {
            '\'' => {
                if space && !out.is_empty() {
                    out.push(' ');
                }
                space = false;
                literal = true;
                out.push(character);
            }
            '"' => {}
            character if character.is_whitespace() => space = true,
            character => {
                if space && !out.is_empty() {
                    out.push(' ');
                }
                space = false;
                out.push(character);
            }
        }
    }
    out
}

fn query<T>(connection: &Connection, sql: &str, row: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>) -> Vec<T> {
    connection.prepare(sql).unwrap().query_map([], row).unwrap().collect::<rusqlite::Result<_>>().unwrap()
}

