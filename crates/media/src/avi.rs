//! AVI (RIFF) files: the Motion-JPEG video stream's pictures and timing.
//!
//! Read from the public container description (Microsoft's AVI RIFF file reference): a `RIFF`
//! chunk of form `AVI ` holding a `hdrl` list (`avih`, one `strl` list per stream with its `strh`
//! stream header and `strf` format) and a `movi` list of data chunks named by stream number
//! (`00dc` is a compressed picture of stream 0). OpenDML files continue in further `RIFF AVIX`
//! chunks. Every size is checked against what is left of the file: a chunk that claims more ends
//! the walk and what came before stays (a download cut short still plays).

use std::sync::Arc;

use crate::bytes::{fourcc, le16, le32, slice, tag};
use crate::{Container, FrameRef, MAX_FRAMES, Media, MediaError, MediaInfo, MediaKind, Source, check_size, corrupt, jpeg};

/// Deepest nesting of lists followed.
const MAX_DEPTH: usize = 8;
/// Most RIFF chunks (a file and its OpenDML continuations) walked.
const MAX_RIFFS: usize = 4096;
/// Most streams kept.
const MAX_STREAMS: usize = 64;

#[derive(Clone, Debug, Default)]
struct Stream {
    kind: [u8; 4],
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    /// `strf` for a video stream: the bitmap header's compression code, size and bit depth.
    compression: Option<[u8; 4]>,
    width: i32,
    height: i32,
    bit_count: u16,
    /// Pictures found for this stream in `movi` lists.
    frames: Vec<FrameRef>,
}

#[derive(Default)]
struct Avi {
    micros_per_frame: u32,
    width: u32,
    height: u32,
    /// In the order of the stream headers (a data chunk's name numbers its stream).
    streams: Vec<Stream>,
}

pub(crate) fn open(bytes: Arc<Vec<u8>>) -> Result<Media, MediaError> {
    let mut avi = Avi::default();
    walk_riffs(&bytes, &mut avi);
    let has_audio = avi.streams.iter().any(|s| &s.kind == b"auds");
    let index =
        avi.streams.iter().position(|s| &s.kind == b"vids").ok_or_else(|| MediaError::Unsupported("This AVI file has no video stream".into()))?;
    let video = avi.streams.swap_remove(index);
    let code = video.compression.filter(|c| *c != [0; 4]).unwrap_or(video.handler);
    if !is_mjpeg(code) {
        return Err(MediaError::UnsupportedCodec { container: Container::Avi, codec: codec_name(code, video.bit_count) });
    }
    let frames = video.frames;
    if frames.is_empty() {
        return Err(corrupt("the AVI file holds no pictures"));
    }
    // The first picture's own header says what size the pictures are; the file's headers are only
    // a fallback.
    let first = frames.iter().find(|f| f.len > 0).ok_or_else(|| corrupt("the AVI file's pictures are all empty"))?;
    let picture = slice(&bytes, usize::try_from(first.offset).unwrap_or(usize::MAX), first.len as usize)
        .ok_or_else(|| corrupt("a picture is outside the file"))?;
    let info = jpeg::scan(picture)?;
    let (w, h) = (info.width, info.height);
    check_size(w, h)?;
    // Frame times: the stream's rate, else the main header's. Each is one division, so a picture
    // that starts at half a second starts at 0.5 exactly.
    let (per, over) = if video.scale > 0 && video.rate > 0 {
        (f64::from(video.scale), f64::from(video.rate))
    } else if avi.micros_per_frame > 0 {
        (f64::from(avi.micros_per_frame), 1e6)
    } else {
        return Err(corrupt("the AVI file doesn't say how fast its pictures play"));
    };
    let starts: Vec<f64> = (0..frames.len()).map(|i| i as f64 * per / over).collect();
    let duration = frames.len() as f64 * per / over;
    if !(duration.is_finite() && duration > 0.0 && duration <= crate::MAX_DURATION) {
        return Err(MediaError::TooLarge("This video is too long".into()));
    }
    let info = MediaInfo {
        container: Container::Avi,
        kind: MediaKind::Video,
        codec: "Motion-JPEG".into(),
        width: w,
        height: h,
        duration,
        frame_count: u32::try_from(frames.len()).unwrap_or(u32::MAX),
        frame_rate: frames.len() as f64 / duration,
        has_audio,
    };
    Ok(Media::new(info, starts, Source::Mjpeg { bytes, frames, size: (w, h) }))
}

/// The file's `RIFF` chunks, one after the other.
fn walk_riffs(b: &[u8], avi: &mut Avi) {
    let mut pos = 0usize;
    for _ in 0..MAX_RIFFS {
        let (Some(id), Some(size)) = (tag(b, pos), le32(b, pos + 4)) else { break };
        if &id != b"RIFF" {
            break;
        }
        let Some(form) = tag(b, pos + 8) else { break };
        let end = pos.saturating_add(8).saturating_add(size as usize).min(b.len());
        if &form == b"AVI " || &form == b"AVIX" {
            walk(b, pos + 12, end, avi, 0);
        }
        // A chunk's data is padded to an even size.
        let next = end.saturating_add(end & 1);
        if next <= pos {
            break;
        }
        pos = next;
    }
}

/// The chunks of `b[start..end]`.
fn walk(b: &[u8], start: usize, end: usize, avi: &mut Avi, depth: usize) {
    let mut p = start;
    while p.saturating_add(8) <= end {
        let (Some(id), Some(size)) = (tag(b, p), le32(b, p + 4)) else { return };
        let data = p + 8;
        let claimed_end = data.saturating_add(size as usize);
        let data_end = claimed_end.min(end);
        match &id {
            b"LIST" => {
                if let Some(list) = tag(b, data).filter(|_| data_end >= data + 4)
                    && depth < MAX_DEPTH
                {
                    match &list {
                        b"hdrl" | b"strl" | b"rec " => walk(b, data + 4, data_end, avi, depth + 1),
                        b"movi" => movi(b, data + 4, data_end, avi, depth + 1),
                        _ => {}
                    }
                }
            }
            b"avih" => {
                // dwMicroSecPerFrame, …, dwWidth, dwHeight (the structure is 56 bytes).
                avi.micros_per_frame = le32(b, data).unwrap_or(0);
                avi.width = le32(b, data + 32).unwrap_or(0);
                avi.height = le32(b, data + 36).unwrap_or(0);
            }
            b"strh" if avi.streams.len() < MAX_STREAMS => {
                let s = Stream {
                    kind: tag(b, data).unwrap_or_default(),
                    handler: tag(b, data + 4).unwrap_or_default(),
                    scale: le32(b, data + 20).unwrap_or(0),
                    rate: le32(b, data + 24).unwrap_or(0),
                    ..Stream::default()
                };
                avi.streams.push(s);
            }
            b"strf" => {
                if let Some(s) = avi.streams.last_mut()
                    && &s.kind == b"vids"
                {
                    // BITMAPINFOHEADER: biSize, biWidth, biHeight, biPlanes, biBitCount, biCompression.
                    s.width = le32(b, data + 4).unwrap_or(0) as i32;
                    s.height = le32(b, data + 8).unwrap_or(0) as i32;
                    s.bit_count = le16(b, data + 14).unwrap_or(0);
                    s.compression = tag(b, data + 16);
                }
            }
            _ => {}
        }
        if claimed_end > end {
            // The chunk runs past what holds it: nothing after it can be trusted.
            return;
        }
        let next = data_end.saturating_add(data_end.wrapping_sub(data) & 1);
        if next <= p {
            return;
        }
        p = next;
    }
}

/// The data chunks of a `movi` list: pictures of the video stream are kept.
fn movi(b: &[u8], start: usize, end: usize, avi: &mut Avi, depth: usize) {
    let mut p = start;
    while p.saturating_add(8) <= end {
        let (Some(id), Some(size)) = (tag(b, p), le32(b, p + 4)) else { return };
        let data = p + 8;
        let claimed_end = data.saturating_add(size as usize);
        let data_end = claimed_end.min(end);
        if &id == b"LIST" {
            // `rec ` lists group the chunks of one moment.
            if tag(b, data) == Some(*b"rec ") && depth < MAX_DEPTH && data_end >= data + 4 {
                movi(b, data + 4, data_end, avi, depth + 1);
            }
        } else if let Some(stream) = stream_of(id)
            && matches!(&id[2..], b"dc" | b"db")
            && let Some(s) = avi.streams.get_mut(stream)
            && &s.kind == b"vids"
            && s.frames.len() < MAX_FRAMES
        {
            // A picture cut short by the end of the file isn't kept.
            if claimed_end <= end {
                s.frames.push(FrameRef { offset: data as u64, len: size });
            }
        }
        if claimed_end > end {
            return;
        }
        let next = data_end.saturating_add(data_end.wrapping_sub(data) & 1);
        if next <= p {
            return;
        }
        p = next;
    }
}

/// The stream number a data chunk's name starts with (`00dc`: 0).
fn stream_of(id: [u8; 4]) -> Option<usize> {
    let digit = |c: u8| c.is_ascii_digit().then(|| usize::from(c - b'0'));
    Some(digit(id[0])? * 10 + digit(id[1])?)
}

/// The compression codes of Motion-JPEG pictures in AVI.
fn is_mjpeg(code: [u8; 4]) -> bool {
    [*b"MJPG", *b"mjpg", *b"JPEG", *b"jpeg", *b"dmb1"].contains(&code)
}

/// A codec for people: its name and the code it carries.
fn codec_name(code: [u8; 4], bit_count: u16) -> String {
    let text = fourcc(code);
    let upper = text.to_ascii_uppercase();
    let name = match upper.as_str() {
        "H264" | "X264" | "AVC1" | "DAVC" | "VSSH" => "H.264",
        "HEVC" | "HVC1" | "H265" | "X265" => "HEVC",
        "AV01" => "AV1",
        "VP80" => "VP8",
        "VP90" => "VP9",
        "XVID" | "DIVX" | "DX50" | "FMP4" | "MP4V" | "3IV2" => "MPEG-4 Visual",
        "MP42" | "MP43" | "MPG4" | "DIV3" | "DIV4" | "DIV5" | "DIV6" => "MPEG-4 (older variants)",
        "WMV1" | "WMV2" | "WMV3" | "WVC1" => "WMV",
        "DVSD" | "DVC " | "DVCP" | "DV25" | "DV50" => "DV",
        "MJPA" => "Motion-JPEG with interlaced fields",
        "CVID" => "Cinepak",
        "IV32" | "IV41" | "IV50" => "Indeo",
        "\0\0\0\0" | "????" => return if bit_count > 0 { format!("uncompressed {bit_count}-bit RGB") } else { "unknown".into() },
        _ => return format!("'{text}'"),
    };
    format!("{name} ({text})")
}
