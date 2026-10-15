//! Video and animation sources for prototypes, with no UI.
//!
//! [`Media::open`] reads a file's bytes and says what it is ([`MediaInfo`]: size, duration, frame
//! count, sound) and gives its pictures by time ([`Media::frame_at`]); [`Player`] is the clock
//! and state machine that plays one (play, pause, mute, skip, loop, "hit this time", "ended"),
//! driven by whatever time its caller has, so it is deterministic and needs no window.
//!
//! What opens, honestly:
//!
//! - **Motion-JPEG** (every picture a JPEG) in **AVI** (`MJPG`) and in **MP4 / MOV** (`jpeg`;
//!   MPEG-4 Visual objects of type JPEG). The containers are read here, from their public
//!   specifications, with every size checked; the pictures are decoded by the JPEG decoder of the
//!   `image` crate.
//! - **Animated GIF, WebP and PNG** (APNG), through the `image` crate's decoders.
//! - Every other video codec (H.264, HEVC, AV1, VP8 and VP9, ProRes, MPEG-4 Visual…) is
//!   recognised in its container and refused with a message that names it: there is no pure-Rust
//!   decoder for them, and nothing here links C. [`MediaError::UnsupportedCodec`] says
//!   "This video codec isn't supported yet: H.264 (avc1)".
//! - Sound is not played: files that have some open, and [`MediaInfo::has_audio`] says so.
//!
//! Nothing here panics on a damaged or hostile file: sizes, counts, dimensions, durations and
//! allocations are capped ([`MAX_PIXELS`], [`MAX_FRAMES`], …), and truncation gives what came
//! before it.
//!
//! With the `testing` feature the crate also offers [`testing`]: a synthetic source (a few bytes
//! that open as a video whose pictures show their own index) and writers of small AVI, MOV, MP4,
//! GIF, WebP and APNG files, so every behaviour is tested without real files.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

mod anim;
mod avi;
mod bmff;
mod bytes;
mod cache;
mod clock;
mod ebml;
mod jpeg;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(test)]
mod anim_tests;
#[cfg(test)]
mod avi_tests;
#[cfg(test)]
mod bmff_tests;
#[cfg(test)]
mod clock_tests;
#[cfg(test)]
mod fuzz_tests;
#[cfg(test)]
mod jpeg_tests;
#[cfg(test)]
mod media_tests;

use std::sync::{Arc, Mutex, PoisonError};

pub use cache::{forget_all, shared};
pub use clock::{Events, MAX_STEP, Player, PlayerConfig};

/// The largest video file the editor places (bytes): bigger ones are refused with a message.
pub const MAX_FILE_BYTES: usize = 100 << 20;
/// The longest side of a picture (pixels).
pub const MAX_SIDE: u32 = 16_384;
/// The most pixels in a picture (8K UHD is 33 million).
pub const MAX_PIXELS: u64 = 36_000_000;
/// The most pictures a video lists.
pub const MAX_FRAMES: usize = 500_000;
/// The longest video (seconds).
pub const MAX_DURATION: f64 = 86_400.0;
/// The most pictures of an animated image that play.
pub const MAX_ANIMATION_FRAMES: usize = 500;
/// The most decoded bytes an animated image's pictures take together (pictures past it don't play).
pub const MAX_ANIMATION_BYTES: u64 = 128 << 20;
/// How many decoded pictures of a video stay at hand (bytes): scrubbing and replays reuse them.
const FRAME_CACHE_BYTES: usize = 96 << 20;

/// The file layouts [`Media`] recognises.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Container {
    Avi,
    Mp4,
    Mov,
    Gif,
    WebP,
    /// An animated PNG.
    Apng,
    /// WebM or Matroska: recognised so it can be refused by name (its codecs aren't decoded).
    Matroska,
    /// The `testing` feature's synthetic source (never recognised without it).
    Synthetic,
}

impl Container {
    /// The layout's name, for messages.
    pub fn label(self) -> &'static str {
        match self {
            Container::Avi => "AVI",
            Container::Mp4 => "MP4",
            Container::Mov => "MOV",
            Container::Gif => "GIF",
            Container::WebP => "WebP",
            Container::Apng => "APNG",
            Container::Matroska => "WebM / Matroska",
            Container::Synthetic => "synthetic",
        }
    }
}

/// A video file or an animated image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// A container with a video track (AVI, MP4, MOV).
    Video,
    /// An animated image format (GIF, WebP, APNG).
    Animation,
}

/// What a file is.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaInfo {
    pub container: Container,
    pub kind: MediaKind,
    /// The picture codec, named for people ("Motion-JPEG", "GIF").
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Seconds: more than 0, at most [`MAX_DURATION`].
    pub duration: f64,
    pub frame_count: u32,
    /// Average pictures per second.
    pub frame_rate: f64,
    /// The file has sound (it isn't played).
    pub has_audio: bool,
}

/// A picture: straight (not premultiplied) RGBA, 8 bits per channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// `width × height × 4` bytes, rows from the top.
    pub rgba: Vec<u8>,
}

impl Frame {
    /// The pixel at `(x, y)` (None outside the picture).
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let at = (usize::try_from(y).ok()?.checked_mul(usize::try_from(self.width).ok()?)?.checked_add(usize::try_from(x).ok()?)?).checked_mul(4)?;
        self.rgba.get(at..at.checked_add(4)?).and_then(|p| p.try_into().ok())
    }

    /// The bytes the picture takes.
    pub fn bytes(&self) -> usize {
        self.rgba.len()
    }
}

/// Why a file doesn't open or a picture can't be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    /// Not a video or animated image this crate recognises.
    NotMedia,
    /// A container read fine, with a video codec that isn't decoded. `codec` names it
    /// ("H.264 (avc1)").
    UnsupportedCodec { container: Container, codec: String },
    /// Something else about the file isn't handled (fragmented MP4, no video track, …); the text
    /// says what, as a sentence.
    Unsupported(String),
    /// The file's structure doesn't add up.
    Corrupt(String),
    /// Beyond the limits (size, pictures, dimensions, duration).
    TooLarge(String),
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MediaError::NotMedia => f.write_str("This isn't a video or animated image FigCraft can read"),
            MediaError::UnsupportedCodec { codec, .. } => write!(f, "This video codec isn't supported yet: {codec}"),
            MediaError::Unsupported(m) | MediaError::TooLarge(m) => f.write_str(m),
            MediaError::Corrupt(m) => write!(f, "This video is damaged: {m}"),
        }
    }
}

impl std::error::Error for MediaError {}

pub(crate) fn corrupt(m: impl Into<String>) -> MediaError {
    MediaError::Corrupt(m.into())
}

/// The picture size must be positive and within [`MAX_SIDE`] and [`MAX_PIXELS`].
pub(crate) fn check_size(w: u32, h: u32) -> Result<(), MediaError> {
    if w == 0 || h == 0 {
        return Err(corrupt("the picture has no size"));
    }
    if w > MAX_SIDE || h > MAX_SIDE || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(MediaError::TooLarge(format!(
            "Pictures of {w} × {h} pixels are too large (the most is {} million pixels, {MAX_SIDE} on a side)",
            MAX_PIXELS / 1_000_000
        )));
    }
    Ok(())
}

/// Which layout `b` starts like, without reading further than a few headers; `None` for anything
/// else, still images (also animated-image formats that hold one picture) included. A GIF may
/// turn out to have a single picture: [`Media::open`] says so.
pub fn sniff(b: &[u8]) -> Option<Container> {
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some(Container::Gif);
    }
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return anim::png_is_animated(b).then_some(Container::Apng);
    }
    if b.get(0..4) == Some(b"RIFF") {
        return match b.get(8..12) {
            Some(b"AVI ") | Some(b"AVIX") => Some(Container::Avi),
            Some(b"WEBP") => anim::webp_is_animated(b).then_some(Container::WebP),
            _ => None,
        };
    }
    if let Some(kind) = b.get(4..8)
        && [&b"ftyp"[..], b"moov", b"mdat", b"free", b"wide", b"skip", b"pnot"].contains(&kind)
    {
        return Some(bmff::brand(b));
    }
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Container::Matroska);
    }
    #[cfg(any(test, feature = "testing"))]
    if testing::is_synthetic(b) {
        return Some(Container::Synthetic);
    }
    None
}

/// Where a video's pictures come from.
enum Source {
    /// Motion-JPEG: each picture decoded when asked for, from the file's own bytes.
    Mjpeg { bytes: Arc<Vec<u8>>, frames: Vec<FrameRef>, size: (u32, u32) },
    /// An animated image: every picture decoded when it opened.
    Frames(Vec<Arc<Frame>>),
    #[cfg(any(test, feature = "testing"))]
    Synthetic(testing::SyntheticSpec),
}

/// Where one Motion-JPEG picture is in the file.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameRef {
    pub offset: u64,
    pub len: u32,
}

/// An opened video or animated image: its [`MediaInfo`] and its pictures by time. Shareable between
/// threads (pictures are decoded on demand and a few are kept).
pub struct Media {
    info: MediaInfo,
    /// When each picture starts (seconds, increasing; the first is 0).
    starts: Vec<f64>,
    source: Source,
    cache: Mutex<FrameCache>,
}

const _: fn() = || {
    fn is_send_sync<T: Send + Sync>() {}
    is_send_sync::<Media>();
};

impl std::fmt::Debug for Media {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Media").field("info", &self.info).finish_non_exhaustive()
    }
}

impl Media {
    /// Open `bytes` as a video or animated image. Cheap for Motion-JPEG (the containers' tables are
    /// read, no picture decoded but the first, to check it); animated images decode all their
    /// pictures (within [`MAX_ANIMATION_FRAMES`] and [`MAX_ANIMATION_BYTES`]).
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Media, MediaError> {
        match sniff(&bytes).ok_or(MediaError::NotMedia)? {
            Container::Avi => avi::open(bytes),
            c @ (Container::Mp4 | Container::Mov) => bmff::open(bytes, c),
            c @ (Container::Gif | Container::WebP | Container::Apng) => anim::open(&bytes, c),
            Container::Matroska => Err(ebml::refuse(&bytes)),
            #[cfg(any(test, feature = "testing"))]
            Container::Synthetic => testing::open(&bytes),
            #[cfg(not(any(test, feature = "testing")))]
            Container::Synthetic => Err(MediaError::NotMedia),
        }
    }

    pub(crate) fn new(info: MediaInfo, starts: Vec<f64>, source: Source) -> Media {
        Media { info, starts, source, cache: Mutex::new(FrameCache::default()) }
    }

    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    /// The index of the picture showing `t` seconds in: the first before time 0 or for a time that
    /// isn't a number, the last at or after the end.
    pub fn frame_index_at(&self, t: f64) -> u32 {
        if t.is_nan() || t <= 0.0 {
            return 0;
        }
        let n = self.starts.partition_point(|s| *s <= t);
        u32::try_from(n.saturating_sub(1)).unwrap_or(u32::MAX)
    }

    /// When picture `index` starts (seconds; the last picture's start for an index past the end).
    pub fn frame_start(&self, index: u32) -> f64 {
        let i = usize::try_from(index).unwrap_or(usize::MAX).min(self.starts.len().saturating_sub(1));
        self.starts.get(i).copied().unwrap_or(0.0)
    }

    /// Picture `index` (the last for an index past the end). Decoded now, or taken from the few
    /// kept. A picture that can't be decoded is an error; [`frame_at`](Self::frame_at) falls back
    /// to an earlier one.
    pub fn frame(&self, index: u32) -> Result<Arc<Frame>, MediaError> {
        let i = usize::try_from(index).unwrap_or(usize::MAX).min(self.starts.len().saturating_sub(1));
        // An animated image holds its pictures already.
        if let Source::Frames(frames) = &self.source {
            return frames.get(i).cloned().ok_or_else(|| corrupt("no such picture"));
        }
        let i = u32::try_from(i).unwrap_or(0);
        if let Some(f) = self.cache.lock().unwrap_or_else(PoisonError::into_inner).get(i) {
            return Ok(f);
        }
        let frame = Arc::new(self.decode(i)?);
        self.cache.lock().unwrap_or_else(PoisonError::into_inner).put(i, frame.clone());
        Ok(frame)
    }

    /// The picture showing `t` seconds in. A damaged picture is replaced by the nearest earlier one
    /// that decodes (Motion-JPEG pictures stand alone), looking back a few only.
    pub fn frame_at(&self, t: f64) -> Result<Arc<Frame>, MediaError> {
        let at = self.frame_index_at(t);
        let mut first_error = None;
        for back in 0..=MAX_LOOK_BACK {
            let Some(i) = at.checked_sub(back) else { break };
            match self.frame(i) {
                Ok(f) => return Ok(f),
                Err(e) => {
                    first_error.get_or_insert(e);
                }
            }
        }
        Err(first_error.unwrap_or_else(|| corrupt("no picture")))
    }

    fn decode(&self, index: u32) -> Result<Frame, MediaError> {
        let i = usize::try_from(index).map_err(|_| corrupt("no such picture"))?;
        match &self.source {
            Source::Frames(frames) => frames.get(i).map(|f| (**f).clone()).ok_or_else(|| corrupt("no such picture")),
            Source::Mjpeg { bytes, frames, size } => {
                // A picture of no bytes is a dropped one: the one before shows again.
                let mut at = i;
                let r = loop {
                    let r = frames.get(at).ok_or_else(|| corrupt("no such picture"))?;
                    if r.len > 0 {
                        break r;
                    }
                    at = at.checked_sub(1).ok_or_else(|| corrupt("the first picture is empty"))?;
                };
                let start = usize::try_from(r.offset).map_err(|_| corrupt("a picture is outside the file"))?;
                let end = start.checked_add(r.len as usize).ok_or_else(|| corrupt("a picture is outside the file"))?;
                let data = bytes.get(start..end).ok_or_else(|| corrupt("a picture is outside the file"))?;
                jpeg::decode(data, Some(*size))
            }
            #[cfg(any(test, feature = "testing"))]
            Source::Synthetic(spec) => Ok(testing::frame(spec, index)),
        }
    }

    /// Roughly the memory the opened file holds besides its bytes (animated images keep every
    /// picture; videos keep a few).
    pub fn resident_bytes(&self) -> usize {
        let tables = self.starts.len().saturating_mul(24);
        match &self.source {
            Source::Frames(f) => f.iter().map(|f| f.bytes()).sum::<usize>().saturating_add(tables),
            _ => FRAME_CACHE_BYTES.saturating_add(tables),
        }
    }
}

/// How many pictures [`Media::frame_at`] looks back through for one that decodes.
const MAX_LOOK_BACK: u32 = 8;

/// A few decoded pictures, the least recently used going first when they take too many bytes.
#[derive(Default)]
struct FrameCache {
    bytes: usize,
    tick: u64,
    entries: Vec<(u32, u64, Arc<Frame>)>,
}

impl FrameCache {
    fn get(&mut self, index: u32) -> Option<Arc<Frame>> {
        self.tick += 1;
        let tick = self.tick;
        let e = self.entries.iter_mut().find(|e| e.0 == index)?;
        e.1 = tick;
        Some(e.2.clone())
    }

    fn put(&mut self, index: u32, frame: Arc<Frame>) {
        self.tick += 1;
        if self.entries.iter().any(|e| e.0 == index) {
            return;
        }
        self.bytes = self.bytes.saturating_add(frame.bytes());
        self.entries.push((index, self.tick, frame));
        // The newest always stays, even when it alone is over budget.
        while self.bytes > FRAME_CACHE_BYTES && self.entries.len() > 1 {
            let Some(oldest) = self.entries.iter().enumerate().min_by_key(|(_, e)| e.1).map(|(i, _)| i) else { break };
            let (_, _, gone) = self.entries.swap_remove(oldest);
            self.bytes = self.bytes.saturating_sub(gone.bytes());
        }
    }
}
