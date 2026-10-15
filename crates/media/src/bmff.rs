//! MP4 and QuickTime MOV files (the ISO base media file format): the video track's pictures and
//! timing, from the sample tables.
//!
//! Read from the public specifications (ISO/IEC 14496-12 and Apple's QuickTime file format
//! documentation): a file is a run of boxes (`size`, `type`, payload; a size of 1 means a 64-bit
//! size follows, 0 means "to the end of the file"); the `moov` box holds a `trak` per track, each
//! with its handler (`vide`, `soun`…), its timescale (`mdhd`) and the sample tables (`stsd`
//! description, `stts` durations, `stsc` samples per chunk, `stsz` sample sizes, `stco` / `co64`
//! chunk offsets) that say where each sample is in the file and when it starts. Every size and
//! count read from the file is checked against what is left of it, and nesting is bounded.
//! Fragmented files (`moof` boxes) aren't read.

use std::sync::Arc;

use crate::bytes::{be32, be64, fourcc, slice, tag};
use crate::{Container, FrameRef, MAX_FRAMES, Media, MediaError, MediaInfo, MediaKind, Source, corrupt, jpeg};

/// Most tracks looked at.
const MAX_TRACKS: usize = 256;
/// Most boxes of one parent walked (a hostile file can't make a walk long).
const MAX_BOXES: usize = 1 << 20;

/// A box in the file: its type and where its payload (after the header) and the box end.
#[derive(Clone, Copy, Debug)]
struct BoxRef {
    kind: [u8; 4],
    payload: usize,
    end: usize,
}

/// The boxes in `b[pos..end]`; one that claims more than is left ends at `end`.
struct Boxes<'a> {
    b: &'a [u8],
    pos: usize,
    end: usize,
    count: usize,
}

fn boxes(b: &[u8], pos: usize, end: usize) -> Boxes<'_> {
    Boxes { b, pos, end: end.min(b.len()), count: 0 }
}

impl Iterator for Boxes<'_> {
    type Item = BoxRef;

    fn next(&mut self) -> Option<BoxRef> {
        if self.count >= MAX_BOXES || self.pos.checked_add(8)? > self.end {
            return None;
        }
        self.count += 1;
        let small = be32(self.b, self.pos)?;
        let kind = tag(self.b, self.pos + 4)?;
        let (header, size) = match small {
            0 => (8usize, self.end - self.pos),
            1 => (16usize, usize::try_from(be64(self.b, self.pos + 8)?).ok()?),
            n => (8usize, n as usize),
        };
        if size < header {
            return None;
        }
        let end = self.pos.checked_add(size).map_or(self.end, |e| e.min(self.end));
        let item = BoxRef { kind, payload: self.pos.checked_add(header)?, end };
        self.pos = end;
        Some(item)
    }
}

fn child(b: &[u8], parent: BoxRef, kind: &[u8; 4]) -> Option<BoxRef> {
    boxes(b, parent.payload, parent.end).find(|c| &c.kind == kind)
}

/// Which brand a file declares: `qt  ` is QuickTime, a file without `ftyp` is taken for QuickTime
/// too (old movies start with `moov`, `mdat` or `wide`).
pub(crate) fn brand(b: &[u8]) -> Container {
    match (tag(b, 4), tag(b, 8)) {
        (Some(t), Some(brand)) if &t == b"ftyp" => {
            if &brand == b"qt  " {
                Container::Mov
            } else {
                Container::Mp4
            }
        }
        _ => Container::Mov,
    }
}

/// What the video track's description (`stsd`) says.
struct Description {
    format: [u8; 4],
    /// For `mp4v`: the object type of its decoder configuration (0x6C is JPEG).
    object_type: Option<u8>,
}

/// A track's tables.
#[derive(Default)]
struct Track {
    handler: [u8; 4],
    timescale: u32,
    description: Option<Description>,
    /// `stts`: (samples, ticks each).
    durations: Vec<(u32, u32)>,
    /// `stsc`: (first chunk, samples per chunk), chunks counted from 1.
    runs: Vec<(u32, u32)>,
    sizes: Vec<u32>,
    chunks: Vec<u64>,
    sample_count: usize,
}

pub(crate) fn open(bytes: Arc<Vec<u8>>, container: Container) -> Result<Media, MediaError> {
    let b: &[u8] = &bytes;
    let top: Vec<BoxRef> = boxes(b, 0, b.len()).collect();
    if top.iter().any(|x| &x.kind == b"moof") {
        return Err(MediaError::Unsupported("This video is a fragmented MP4, which isn't supported yet".into()));
    }
    let moov = top
        .iter()
        .find(|x| &x.kind == b"moov")
        .copied()
        .ok_or_else(|| corrupt("the file has no movie header (moov): it may be cut short, or be a streaming file whose header is at the end"))?;
    if child(b, moov, b"mvex").is_some() {
        return Err(MediaError::Unsupported("This video is a fragmented MP4, which isn't supported yet".into()));
    }
    let tracks: Vec<Track> = boxes(b, moov.payload, moov.end).filter(|x| &x.kind == b"trak").take(MAX_TRACKS).filter_map(|t| track(b, t)).collect();
    let has_audio = tracks.iter().any(|t| &t.handler == b"soun");
    let mut videos = tracks.into_iter().filter(|t| &t.handler == b"vide").peekable();
    if videos.peek().is_none() {
        return Err(MediaError::Unsupported("This file has no video track".into()));
    }
    let mut refused: Option<String> = None;
    for t in videos {
        match codec_of(&t) {
            Ok(()) => return finish(bytes, container, t, has_audio),
            Err(name) => {
                refused.get_or_insert(name);
            }
        }
    }
    Err(MediaError::UnsupportedCodec { container, codec: refused.unwrap_or_else(|| "unknown".into()) })
}

/// Whether the track's pictures are Motion-JPEG; otherwise the codec's name for people.
fn codec_of(t: &Track) -> Result<(), String> {
    let Some(d) = &t.description else { return Err("unknown".into()) };
    let text = fourcc(d.format);
    let name = match &d.format {
        b"jpeg" => return Ok(()),
        b"mp4v" if d.object_type == Some(0x6C) => return Ok(()),
        b"mp4v" => "MPEG-4 Visual",
        b"mjpa" | b"mjpb" => "Motion-JPEG with interlaced fields",
        b"avc1" | b"avc3" => "H.264",
        b"hvc1" | b"hev1" => "HEVC",
        b"av01" => "AV1",
        b"vp08" => "VP8",
        b"vp09" => "VP9",
        b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" | b"ap4x" => "Apple ProRes",
        b"s263" | b"h263" => "H.263",
        b"dvc " | b"dvcp" | b"dvpp" | b"dv5n" | b"dv5p" => "DV",
        b"SVQ1" | b"SVQ3" => "Sorenson Video",
        b"cvid" => "Cinepak",
        b"rpza" => "Apple Video",
        b"png " => "PNG",
        _ => return Err(format!("'{text}'")),
    };
    Err(format!("{name} ({text})"))
}

/// The picture list of an accepted video track.
fn finish(bytes: Arc<Vec<u8>>, container: Container, t: Track, has_audio: bool) -> Result<Media, MediaError> {
    if t.timescale == 0 {
        return Err(corrupt("the video track's timescale is zero"));
    }
    let n = t.sample_count.min(t.sizes.len());
    if n == 0 {
        return Err(corrupt("the video track has no pictures"));
    }
    if n > MAX_FRAMES {
        return Err(MediaError::TooLarge(format!("This video has more than {MAX_FRAMES} pictures")));
    }
    // Where each picture is: chunks hold runs of samples one after the other.
    let mut frames: Vec<FrameRef> = Vec::with_capacity(n);
    let mut run = 0usize;
    'chunks: for (i, offset) in t.chunks.iter().enumerate() {
        let number = u32::try_from(i + 1).unwrap_or(u32::MAX);
        while t.runs.get(run + 1).is_some_and(|r| r.0 <= number) {
            run += 1;
        }
        let Some(&(first, per_chunk)) = t.runs.get(run) else { break };
        if first > number {
            continue;
        }
        let mut at = *offset;
        for _ in 0..per_chunk {
            let Some(&size) = t.sizes.get(frames.len()) else { break 'chunks };
            frames.push(FrameRef { offset: at, len: size });
            at = at.saturating_add(u64::from(size));
            if frames.len() >= n {
                break 'chunks;
            }
        }
    }
    if frames.is_empty() {
        return Err(corrupt("the video track's tables don't place any picture"));
    }
    // When each picture starts: the sum of the durations before it.
    let scale = f64::from(t.timescale);
    let mut starts: Vec<f64> = Vec::with_capacity(frames.len());
    let mut ticks = 0u64;
    let mut last_delta = 0u32;
    'times: for &(count, delta) in &t.durations {
        for _ in 0..count {
            if starts.len() >= frames.len() {
                break 'times;
            }
            starts.push(ticks as f64 / scale);
            ticks = ticks.saturating_add(u64::from(delta));
            last_delta = delta;
        }
    }
    if starts.is_empty() {
        return Err(corrupt("the video track has no timing"));
    }
    // Pictures the durations don't reach go on at the last pace.
    while starts.len() < frames.len() {
        starts.push(ticks as f64 / scale);
        ticks = ticks.saturating_add(u64::from(last_delta));
    }
    frames.truncate(starts.len());
    let duration = ticks as f64 / scale;
    if !(duration.is_finite() && duration > 0.0 && duration <= crate::MAX_DURATION) {
        return Err(if duration > crate::MAX_DURATION {
            MediaError::TooLarge("This video is too long".into())
        } else {
            corrupt("the video has no length")
        });
    }
    // The first picture's own header says what size the pictures are.
    let first = frames.iter().find(|f| f.len > 0).ok_or_else(|| corrupt("the video's pictures are all empty"))?;
    let picture = slice(&bytes, usize::try_from(first.offset).unwrap_or(usize::MAX), first.len as usize)
        .ok_or_else(|| corrupt("a picture is outside the file"))?;
    let j = jpeg::scan(picture)?;
    let info = MediaInfo {
        container,
        kind: MediaKind::Video,
        codec: "Motion-JPEG".into(),
        width: j.width,
        height: j.height,
        duration,
        frame_count: u32::try_from(frames.len()).unwrap_or(u32::MAX),
        frame_rate: frames.len() as f64 / duration,
        has_audio,
    };
    Ok(Media::new(info, starts, Source::Mjpeg { bytes, frames, size: (j.width, j.height) }))
}

/// A `trak` box's handler, timescale and tables.
fn track(b: &[u8], trak: BoxRef) -> Option<Track> {
    let mdia = child(b, trak, b"mdia")?;
    let mut t = Track::default();
    if let Some(h) = child(b, mdia, b"hdlr") {
        t.handler = tag(b, h.payload + 8)?;
    }
    if let Some(m) = child(b, mdia, b"mdhd") {
        // Version 0 has 32-bit times, version 1 64-bit; the timescale follows the two times.
        let version = *b.get(m.payload)?;
        t.timescale = be32(b, m.payload + if version == 1 { 20 } else { 12 })?;
    }
    if &t.handler != b"vide" {
        return Some(t);
    }
    let stbl = child(b, child(b, mdia, b"minf")?, b"stbl")?;
    for x in boxes(b, stbl.payload, stbl.end) {
        match &x.kind {
            b"stsd" => t.description = description(b, x),
            b"stts" => {
                t.durations = table(b, x, 8, |b, at| Some((be32(b, at)?, be32(b, at + 4)?)))?;
            }
            b"stsc" => {
                t.runs = table(b, x, 12, |b, at| Some((be32(b, at)?, be32(b, at + 4)?)))?;
            }
            b"stsz" => {
                let each = be32(b, x.payload + 4)?;
                t.sample_count = usize::try_from(be32(b, x.payload + 8)?).ok()?;
                if each == 0 {
                    t.sizes = table_from(b, x, 12, 4, t.sample_count, be32)?;
                } else {
                    // One size for every sample.
                    t.sizes = vec![each; t.sample_count.min(MAX_FRAMES + 1)];
                }
            }
            b"stco" => t.chunks = table(b, x, 4, |b, at| Some(u64::from(be32(b, at)?)))?,
            b"co64" => t.chunks = table(b, x, 8, be64)?,
            _ => {}
        }
    }
    Some(t)
}

/// The rows of a table box: version and flags, a count, then `count` rows of `row` bytes. A count
/// larger than the box holds reads the rows that are there.
fn table<T>(b: &[u8], x: BoxRef, row: usize, read: impl Fn(&[u8], usize) -> Option<T>) -> Option<Vec<T>> {
    let count = usize::try_from(be32(b, x.payload + 4)?).ok()?;
    table_from(b, x, 8, row, count, read)
}

fn table_from<T>(b: &[u8], x: BoxRef, start: usize, row: usize, count: usize, read: impl Fn(&[u8], usize) -> Option<T>) -> Option<Vec<T>> {
    let first = x.payload.checked_add(start)?;
    let room = x.end.saturating_sub(first) / row.max(1);
    let n = count.min(room).min(MAX_FRAMES + 1);
    (0..n).map(|i| read(b, first.checked_add(i.checked_mul(row)?)?)).collect()
}

/// The first sample description of a video track.
fn description(b: &[u8], stsd: BoxRef) -> Option<Description> {
    // Version and flags, the entry count, then the entries: size, format, and for pictures a
    // fixed 78-byte header (the size among it, which the first picture says better) followed by
    // boxes.
    let entry = stsd.payload.checked_add(8)?;
    let size = be32(b, entry)? as usize;
    let format = tag(b, entry + 4)?;
    let end = entry.checked_add(size).map_or(stsd.end, |e| e.min(stsd.end));
    let mut object_type = None;
    if &format == b"mp4v"
        && let Some(esds) = boxes(b, entry + 86, end).find(|x| &x.kind == b"esds")
    {
        object_type = decoder_object_type(b, esds.payload + 4, esds.end);
    }
    Some(Description { format, object_type })
}

/// The object type indication in an MPEG-4 elementary stream descriptor (`esds` payload after its
/// version and flags): the first byte of the decoder configuration descriptor inside the ES one.
fn decoder_object_type(b: &[u8], at: usize, end: usize) -> Option<u8> {
    let (tag3, rest) = (*b.get(at)?, at + 1);
    if tag3 != 0x03 {
        return None;
    }
    let (_, used) = descriptor_length(b, rest)?;
    // ES_ID (2) and flags (1), then what the flags say follows.
    let mut p = rest + used + 2;
    let flags = *b.get(p)?;
    p += 1;
    if flags & 0x80 != 0 {
        p += 2;
    }
    if flags & 0x40 != 0 {
        p += 1 + usize::from(*b.get(p)?);
    }
    if flags & 0x20 != 0 {
        p += 2;
    }
    if p >= end || *b.get(p)? != 0x04 {
        return None;
    }
    let (_, used) = descriptor_length(b, p + 1)?;
    b.get(p + 1 + used).copied()
}

/// A descriptor's length: up to four bytes of 7 bits, the top bit saying another follows.
fn descriptor_length(b: &[u8], at: usize) -> Option<(usize, usize)> {
    let mut len = 0usize;
    for i in 0..4 {
        let c = *b.get(at + i)?;
        len = len << 7 | usize::from(c & 0x7F);
        if c & 0x80 == 0 {
            return Some((len, i + 1));
        }
    }
    None
}
