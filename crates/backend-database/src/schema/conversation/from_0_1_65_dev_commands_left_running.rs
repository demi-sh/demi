//! From the schema before a row said which command its shell call left
//! running: the step adds `blocks.left_running` and its index, and fills it
//! for each completed shell call whose view says its command still ran as
//! the call returned, `service` when the call's input started it as one.
//! It adds `command_outputs.started_at`, null for the rows there are.
//!
//! It reads each such record as the release that wrote it did: the body
//! compressed with the conversation's dictionary it names, or none, and the
//! call's input either inline or as the content a marker names.

use std::collections::HashMap;

use rusqlite::{OptionalExtension, Transaction, params};
use serde_json::Value;

use super::super::{Migration, ReadBlob, corrupt_text};

pub(super) const MIGRATION: Migration = Migration::Code(conversation_from_commands_left_running);

/// The control character a record's marker begins with.
const MARK: char = '\u{1}';

fn conversation_from_commands_left_running(transaction: &Transaction<'_>, _: &dyn ReadBlob) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "ALTER TABLE blocks ADD COLUMN left_running TEXT CHECK (left_running IN ('command', 'service'));
         CREATE INDEX blocks_left_running ON blocks (node_id, idx, command, left_running, created_at) WHERE left_running IS NOT NULL;
         ALTER TABLE command_outputs ADD COLUMN started_at INTEGER;",
    )?;
    let calls: Vec<(i64, Option<i64>, Vec<u8>)> = transaction
        .prepare(
            "SELECT record, dictionary, body FROM blocks
             WHERE command IS NOT NULL AND type = 'tool_call' AND status IS NOT 'executing'",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut dictionaries: HashMap<Option<i64>, Vec<u8>> = HashMap::new();
    let mut update = transaction.prepare("UPDATE blocks SET left_running = ?1 WHERE record = ?2")?;
    for (record, dictionary, body) in calls {
        if !dictionaries.contains_key(&dictionary) {
            let bytes = match dictionary {
                Some(id) => transaction
                    .query_row("SELECT bytes FROM dictionaries WHERE id = ?1", [id], |row| row.get(0))
                    .optional()?
                    .ok_or_else(|| corrupt_text(&format!("the dictionary {id} is missing")))?,
                None => Vec::new(),
            };
            dictionaries.insert(dictionary, bytes);
        }
        let plain = decompress(&body, &dictionaries[&dictionary])?;
        let call: Value = serde_json::from_slice(&plain)
            .map_err(|error| corrupt_text(&format!("a record's body is not JSON: {error}")))?;
        let view = &call["view"];
        if view["kind"] != "shell" || view["status"] != "running" {
            continue;
        }
        let input = match call["input"].as_str() {
            Some(text) => joined(transaction, text)?,
            None => String::new(),
        };
        // An input that does not parse started no service: its call
        // failed before it started one.
        let service = serde_json::from_str::<Value>(&input)
            .ok()
            .and_then(|input| input["service"].as_bool())
            .unwrap_or_default();
        update.execute(params![if service { "service" } else { "command" }, record])?;
    }
    Ok(())
}

/// `stored` decompressed with `dictionary`, empty for none.
fn decompress(stored: &[u8], dictionary: &[u8]) -> rusqlite::Result<Vec<u8>> {
    let size = zstd::zstd_safe::get_frame_content_size(stored)
        .ok()
        .flatten()
        .and_then(|size| usize::try_from(size).ok())
        .ok_or_else(|| corrupt_text("a stored value does not say its size"))?;
    zstd::bulk::Decompressor::with_dictionary(dictionary)
        .and_then(|mut decompressor| decompressor.decompress(stored, size))
        .map_err(|error| corrupt_text(&format!("a stored value does not decompress: {error}")))
}

/// The string `text` of a record is: itself, or what its marker names, a
/// content or a piece of one followed by a tail.
fn joined(transaction: &Transaction<'_>, text: &str) -> rusqlite::Result<String> {
    let Some(rest) = text.strip_prefix(MARK) else {
        return Ok(text.to_owned());
    };
    let mut chars = rest.chars();
    let kind = chars.next().ok_or_else(|| corrupt_text("a marker of no kind"))?;
    if kind == MARK {
        return Ok(rest.to_owned());
    }
    let rest = chars.as_str();
    let hash = rest
        .get(..64)
        .and_then(|hex| hex::decode(hex).ok())
        .ok_or_else(|| corrupt_text("a marker names no hash"))?;
    let stored: Vec<u8> = transaction
        .query_row("SELECT bytes FROM contents WHERE hash = ?1", [&hash], |row| row.get(0))
        .optional()?
        .ok_or_else(|| corrupt_text("a content a record names is missing"))?;
    let whole = String::from_utf8(decompress(&stored, &[])?).map_err(|_| corrupt_text("a content is not text"))?;
    match kind {
        'c' => Ok(whole),
        's' => {
            let mut parts = rest.get(65..).unwrap_or_default().splitn(3, ':');
            let range = parts
                .next()
                .and_then(|from| from.parse::<usize>().ok())
                .zip(parts.next().and_then(|to| to.parse::<usize>().ok()));
            let piece = range
                .and_then(|(from, to)| whole.get(from..to))
                .ok_or_else(|| corrupt_text("a piece lies outside its content"))?;
            Ok(format!("{piece}{}", parts.next().unwrap_or_default()))
        }
        // A call's input is JSON text, never base64 or a JSON value.
        _ => Err(corrupt_text(&format!("a call's input is a marker of the kind {kind:?}"))),
    }
}
