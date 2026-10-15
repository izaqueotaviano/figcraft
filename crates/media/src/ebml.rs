//! WebM and Matroska files, read only far enough to name the video codec they hold, so the refusal
//! can say it ("This video codec isn't supported yet: VP9 (V_VP9)"). Elements are an id and a size
//! (both variable-length integers) and a payload; every walk is bounded in depth and in elements.

use crate::bytes::slice;
use crate::{Container, MediaError};

const EBML_HEADER: u32 = 0x1A45_DFA3;
const DOC_TYPE: u32 = 0x4282;
const SEGMENT: u32 = 0x1853_8067;
const TRACKS: u32 = 0x1654_AE6B;
const TRACK_ENTRY: u32 = 0xAE;
const TRACK_TYPE: u32 = 0x83;
const CODEC_ID: u32 = 0x86;
const CLUSTER: u32 = 0x1F43_B675;
/// How much of the file is looked at: the headers come first.
const LOOK: usize = 4 << 20;
/// Most children of one element listed.
const MAX_CHILDREN: usize = 512;

/// An element's id (marker bits kept, as the specification writes ids), size (None: unknown) and
/// where its payload starts.
fn element(b: &[u8], at: usize) -> Option<(u32, Option<u64>, usize)> {
    let first = *b.get(at)?;
    let id_len = first.leading_zeros() as usize + 1;
    if id_len > 4 {
        return None;
    }
    let id = slice(b, at, id_len)?.iter().fold(0u32, |v, x| v << 8 | u32::from(*x));
    let size_at = at.checked_add(id_len)?;
    let first = *b.get(size_at)?;
    let size_len = first.leading_zeros() as usize + 1;
    if size_len > 8 {
        return None;
    }
    let mask = 0xFFu64 >> size_len;
    let mut value = u64::from(first) & mask;
    let mut all_ones = value == mask;
    for x in slice(b, size_at + 1, size_len - 1)? {
        value = value << 8 | u64::from(*x);
        all_ones &= *x == 0xFF;
    }
    Some((id, (!all_ones).then_some(value), size_at + size_len))
}

/// The elements in `b[start..end]` as (id, payload start, payload end); one that claims more than
/// is left, or has no size, runs to `end`.
fn children(b: &[u8], start: usize, end: usize) -> Vec<(u32, usize, usize)> {
    let end = end.min(b.len());
    let mut out = Vec::new();
    let mut p = start;
    while p < end && out.len() < MAX_CHILDREN {
        let Some((id, size, payload)) = element(b, p) else { break };
        if payload > end {
            break;
        }
        let stop = size.and_then(|s| usize::try_from(s).ok()).and_then(|s| payload.checked_add(s)).map_or(end, |e| e.min(end));
        out.push((id, payload, stop));
        if stop <= p {
            break;
        }
        p = stop;
    }
    out
}

fn text(b: &[u8], start: usize, end: usize) -> Option<String> {
    let t = slice(b, start, end.checked_sub(start)?.min(64))?;
    Some(String::from_utf8_lossy(t).trim_end_matches('\0').to_string())
}

fn uint(b: &[u8], start: usize, end: usize) -> Option<u64> {
    let t = slice(b, start, end.checked_sub(start)?)?;
    (t.len() <= 8).then(|| t.iter().fold(0u64, |v, x| v << 8 | u64::from(*x)))
}

/// The document type the header declares (`webm`, `matroska`).
fn doc_type(b: &[u8]) -> Option<String> {
    let (_, s, e) = children(b, 0, LOOK).into_iter().find(|c| c.0 == EBML_HEADER)?;
    let (_, ts, te) = children(b, s, e).into_iter().find(|c| c.0 == DOC_TYPE)?;
    text(b, ts, te)
}

/// The codec id of the first video track.
fn first_video_codec(b: &[u8]) -> Option<String> {
    for (id, s, e) in children(b, 0, LOOK) {
        if id != SEGMENT {
            continue;
        }
        for (cid, cs, ce) in children(b, s, e) {
            if cid == CLUSTER {
                return None;
            }
            if cid != TRACKS {
                continue;
            }
            for (tid, ts, te) in children(b, cs, ce) {
                if tid != TRACK_ENTRY {
                    continue;
                }
                let fields = children(b, ts, te);
                let kind = fields.iter().find(|f| f.0 == TRACK_TYPE).and_then(|f| uint(b, f.1, f.2));
                if kind == Some(1) {
                    return fields.iter().find(|f| f.0 == CODEC_ID).and_then(|f| text(b, f.1, f.2));
                }
            }
            return None;
        }
    }
    None
}

/// The refusal for a WebM / Matroska file, naming its first video codec.
pub(crate) fn refuse(b: &[u8]) -> MediaError {
    let codec = match first_video_codec(b).as_deref() {
        Some("V_VP8") => "VP8 (V_VP8)".to_string(),
        Some("V_VP9") => "VP9 (V_VP9)".to_string(),
        Some("V_AV1") => "AV1 (V_AV1)".to_string(),
        Some("V_MPEG4/ISO/AVC") => "H.264 (V_MPEG4/ISO/AVC)".to_string(),
        Some("V_MPEGH/ISO/HEVC") => "HEVC (V_MPEGH/ISO/HEVC)".to_string(),
        Some(other) if !other.is_empty() => other.chars().filter(|c| !c.is_control()).take(40).collect(),
        _ if doc_type(b).is_some_and(|t| t.starts_with("webm")) => "WebM (VP8, VP9 or AV1)".to_string(),
        _ => "Matroska".to_string(),
    };
    MediaError::UnsupportedCodec { container: Container::Matroska, codec }
}
