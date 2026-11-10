//! A medium's size in pixels, read from its header alone.
//!
//! Its reference and an attachment's record carry the size so a page knows a
//! thumbnail's box before the bytes arrive: an image's as it shows, and a
//! video's from its container's header, MP4 and QuickTime or WebM.

use bytes::Bytes;
use demi_shared_types::PixelSize;

use crate::images;

/// The size of `bytes`, the whole of a medium of `media_type`: an image's as
/// a web browser shows it, and a video's where its container's header gives
/// it. None for any other medium, and for one whose header does not read.
///
/// It takes the whole file, not its opening: a video's header may follow its
/// frames, as in an MP4 whose `moov` box was never moved to the front, and an
/// image's may follow metadata of any length.
pub fn pixel_size(bytes: Bytes, media_type: &str) -> Option<PixelSize> {
    match media_type.split_once('/')?.0 {
        "image" => images::shown_size(&bytes, media_type),
        "video" => video_size(bytes, media_type),
        _ => None,
    }
}

/// Whether a medium of `media_type` has a size [`pixel_size`] reads.
pub fn has_pixel_size(media_type: &str) -> bool {
    matches!(media_type.split_once('/'), Some(("image" | "video", _)))
}

/// The size a video's container header gives its video track, as a player
/// shows it.
///
/// An MP4 or QuickTime file is read by Demi's one MP4 reader, which turns a
/// portrait track's size by its matrix; any other container, as WebM, by a
/// parser that walks its elements to the track header. Both find the header
/// wherever it stands and never read the frames.
fn video_size(bytes: Bytes, media_type: &str) -> Option<PixelSize> {
    if matches!(media_type, "video/mp4" | "video/quicktime" | "video/x-m4v") {
        let facts = demi_shared_types::mp4::read(&bytes).ok()?;
        return (facts.width > 0 && facts.height > 0).then_some(PixelSize { width: facts.width, height: facts.height });
    }
    let source = nom_exif::MediaSource::from_memory(bytes).ok()?;
    let track = nom_exif::MediaParser::new().parse_track(source).ok()?;
    let side = |tag| {
        track
            .get(tag)
            .and_then(nom_exif::EntryValue::as_u32)
            .filter(|side| *side > 0)
    };
    Some(PixelSize {
        width: side(nom_exif::TrackInfoTag::Width)?,
        height: side(nom_exif::TrackInfoTag::Height)?,
    })
}
