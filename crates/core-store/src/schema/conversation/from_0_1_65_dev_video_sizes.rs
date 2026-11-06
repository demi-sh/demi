//! From the schema whose attachments took a video's size from its file's
//! first 64 KiB: an MP4 or QuickTime file whose `moov` box follows its
//! frames kept no size, so its tile had no box before its first frame. The
//! step reads each such video's blob and keeps the size its video track's
//! header gives, as a player shows it, for the attachment and for the
//! shares' copies of it; one whose blob or header does not read keeps none,
//! since a size only shapes its tile.

use std::str::FromStr;

use demi_shared_types::BlobRef;
use rusqlite::Transaction;

use super::Migration;
use super::super::ReadBlob;

pub(super) const MIGRATION: Migration = Migration::Code(conversation_from_video_sizes);

fn conversation_from_video_sizes(transaction: &Transaction<'_>, blobs: &dyn ReadBlob) -> rusqlite::Result<()> {
    let videos: Vec<(i64, String)> = transaction
        .prepare(
            "SELECT number, blob FROM attachments
             WHERE width IS NULL AND media_type IN ('video/mp4', 'video/quicktime', 'video/x-m4v')",
        )?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut update = transaction.prepare("UPDATE attachments SET width = ?1, height = ?2 WHERE number = ?3")?;
    for (number, blob) in videos {
        let Ok(blob) = BlobRef::from_str(&blob) else {
            continue;
        };
        let Ok(Some(bytes)) = blobs.read(&blob) else {
            continue;
        };
        if let Some((width, height)) = video_track_size(&bytes) {
            update.execute(rusqlite::params![width, height, number])?;
        }
    }
    transaction.execute_batch(
        "UPDATE share_attachments
         SET (width, height) = (
           SELECT width, height FROM attachments
           WHERE attachments.blob = share_attachments.blob AND attachments.width IS NOT NULL
           LIMIT 1
         )
         WHERE width IS NULL AND EXISTS (
           SELECT 1 FROM attachments
           WHERE attachments.blob = share_attachments.blob AND attachments.width IS NOT NULL
         );",
    )?;
    Ok(())
}

/// The width and height of the first video track of an MP4 or QuickTime
/// file as a player shows it (`moov` / `trak` / `tkhd`, a track whose
/// `mdia` / `hdlr` names the handler `vide`): each the integer part of its
/// 16.16 fixed-point value, swapped when the header's matrix, whose
/// diagonal is then zero, turns the picture a quarter; none when it is
/// missing or zero.
fn video_track_size(file: &[u8]) -> Option<(u32, u32)> {
    let moov = child(file, b"moov")?;
    boxes(moov).filter(|(kind, _)| kind == b"trak").find_map(|(_, trak)| {
        let mdia = child(trak, b"mdia")?;
        if child(mdia, b"hdlr")?.get(8..12)? != b"vide" {
            return None;
        }
        // The header ends with its matrix of nine 32-bit values, then the
        // width and the height, in both of its versions.
        let tkhd = child(trak, b"tkhd")?;
        let tail = tkhd.get(tkhd.len().checked_sub(44)?..)?;
        let word = |at: usize| u32::from_be_bytes([tail[at], tail[at + 1], tail[at + 2], tail[at + 3]]);
        let (width, height) = (word(36) >> 16, word(40) >> 16);
        let turned = word(0) == 0 && word(16) == 0;
        let shown = if turned { (height, width) } else { (width, height) };
        (width > 0 && height > 0).then_some(shown)
    })
}

/// The payload of the first box of `kind` among those `bytes` holds.
fn child<'a>(bytes: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    boxes(bytes).find(|(found, _)| found == kind).map(|(_, payload)| payload)
}

/// Each box `bytes` holds, its kind and its payload, in order, up to the
/// first one whose size does not fit.
fn boxes(mut bytes: &[u8]) -> impl Iterator<Item = ([u8; 4], &[u8])> {
    std::iter::from_fn(move || {
        let size = u64::from(u32::from_be_bytes(bytes.get(0..4)?.try_into().ok()?));
        let kind: [u8; 4] = bytes.get(4..8)?.try_into().ok()?;
        let (header, size) = match size {
            // The box reaches the end of what holds it.
            0 => (8, bytes.len() as u64),
            1 => (16, u64::from_be_bytes(bytes.get(8..16)?.try_into().ok()?)),
            size => (8, size),
        };
        let size = usize::try_from(size).ok().filter(|size| *size >= header && *size <= bytes.len())?;
        let payload = &bytes[header..size];
        bytes = &bytes[size..];
        Some((kind, payload))
    })
}
