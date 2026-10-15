//! Video fills: placing a video (or an animated image) in the document and choosing how it plays.
//!
//! What opens is decided by `figcraft-media`: Motion-JPEG in AVI, MP4 and MOV, and animated GIF,
//! WebP and PNG. Any other video codec is refused with its name, and the file is not kept in the
//! document. Sound is not played: `muted` is state that prototype actions can change.

use std::sync::Arc;

use figcraft_color::{ImageScaleMode, PaintKind, VideoPaint};
use figcraft_doc::{Document, ImageData, NodeId};
use figcraft_media::{MediaInfo, MediaKind};
use serde_json::{Value, json};

use super::CommandSpec;
use crate::params::*;
use crate::{EngineError, Result, Session, bad, spec};

/// The most video fills `video.info` lists.
const MAX_LISTED: usize = 1000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        spec!(
            "video.set",
            "Set video",
            "{ids?, index? (the fill; default every video fill), autoplay? (it plays when its screen shows), loop?, muted? (state only: nothing plays sound), poster? (seconds into the video that the editor and exports show), mode? (fill|fit)} → {changed}",
            true,
            set
        ),
        spec!(
            "video.info",
            "Video info",
            "{ids?} → {videos: [{id, index, video (the file's hash), autoplay, loop, muted, poster, mode, bytes, container, codec, kind (video|animation), width, height, duration, frames, frameRate, hasAudio} | {id, index, video, error}]}",
            false,
            info
        ),
    ]
}

/// Open `bytes` as a video or animated image and keep them in the document's assets under their
/// content hash (images are kept there too). Nothing is kept when they don't open: a codec that
/// isn't decoded is refused by name ("This video codec isn't supported yet: H.264 (avc1)").
pub fn add_video(doc: &mut Document, bytes: Vec<u8>) -> Result<(String, MediaInfo)> {
    if bytes.len() > figcraft_media::MAX_FILE_BYTES {
        return Err(EngineError::Other(format!(
            "This video is too large: {} MB (the most is {} MB)",
            bytes.len().div_ceil(1 << 20),
            figcraft_media::MAX_FILE_BYTES >> 20
        )));
    }
    let bytes = Arc::new(bytes);
    let hash = figcraft_format::image_hash(&bytes);
    // Opened through the shared cache: the first picture the renderer asks for finds it open.
    let media = figcraft_media::shared(&hash, &bytes).map_err(|e| EngineError::Other(e.to_string()))?;
    let info = media.info().clone();
    doc.images.entry(hash.clone()).or_insert_with(|| ImageData { bytes, width: info.width, height: info.height });
    Ok((hash, info))
}

/// What a file is, as JSON.
pub fn info_json(i: &MediaInfo) -> Value {
    let kind = match i.kind {
        MediaKind::Video => "video",
        MediaKind::Animation => "animation",
    };
    json!({
        "container": i.container.label(),
        "codec": i.codec,
        "kind": kind,
        "width": i.width,
        "height": i.height,
        "duration": i.duration,
        "frames": i.frame_count,
        "frameRate": i.frame_rate,
        "hasAudio": i.has_audio,
    })
}

/// The layout mode a `mode` param names for a video: Fit, or Fill for anything else.
pub fn scale_mode(mode: Option<&str>) -> ImageScaleMode {
    if mode.is_some_and(|m| m.eq_ignore_ascii_case("fit")) { ImageScaleMode::Fit } else { ImageScaleMode::Fill }
}

/// A video paint from placement params: `autoplay`, `loop`, `muted`, `poster` (seconds, within
/// `duration`) and `mode`.
pub fn paint_from_params(hash: &str, p: &Value, duration: f64) -> figcraft_color::Paint {
    let mut paint = figcraft_color::Paint::video(hash, scale_mode(string(p, "mode").as_deref()));
    if let PaintKind::Video(v) = &mut paint.kind {
        v.autoplay = boolean(p, "autoplay").unwrap_or(true);
        v.looping = boolean(p, "loop").unwrap_or(false);
        v.muted = boolean(p, "muted").unwrap_or(false);
        v.poster = num(p, "poster").map_or(0.0, |t| t.clamp(0.0, duration.clamp(0.0, VideoPaint::MAX_POSTER)));
    }
    paint
}

/// A boolean param that must be one when given.
fn flag(cmd: &str, p: &Value, key: &str) -> Result<Option<bool>> {
    match get(p, key) {
        None => Ok(None),
        Some(v) => bool_of(v).map(Some).ok_or_else(|| bad(cmd, format!("`{key}` must be true or false"))),
    }
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "video.set";
    let ids = req_targets(cmd, s, p)?;
    let index = match get(p, "index") {
        None => None,
        Some(_) => Some(req_index(cmd, p, "index")?),
    };
    let (autoplay, looping, muted) = (flag(cmd, p, "autoplay")?, flag(cmd, p, "loop")?, flag(cmd, p, "muted")?);
    let poster = match get(p, "poster") {
        None => None,
        Some(v) => Some(f64_of(v).filter(|t| *t >= 0.0).ok_or_else(|| bad(cmd, "`poster` must be a number of seconds, 0 or more"))?),
    };
    let mode = match get(p, "mode") {
        None => None,
        Some(v) => match v.as_str().map(str::to_ascii_lowercase).as_deref() {
            Some("fill") => Some(ImageScaleMode::Fill),
            Some("fit") => Some(ImageScaleMode::Fit),
            _ => return Err(bad(cmd, "`mode`: fill|fit")),
        },
    };
    if autoplay.is_none() && looping.is_none() && muted.is_none() && poster.is_none() && mode.is_none() {
        return Err(bad(cmd, "pass `autoplay`, `loop`, `muted`, `poster` or `mode`"));
    }
    // The poster stays within the video: its length, when the file is there and opens.
    let mut work: Vec<(NodeId, usize, f64)> = Vec::new();
    {
        let doc = s.doc()?;
        for id in ids {
            let n = doc.get(id)?;
            for (i, paint) in n.fills.iter().enumerate() {
                let PaintKind::Video(v) = &paint.kind else { continue };
                if index.is_some_and(|k| k != i) {
                    continue;
                }
                work.push((id, i, duration_of(doc, v)));
            }
        }
    }
    if work.is_empty() {
        return Err(bad(cmd, if index.is_some() { "no video fill at that index" } else { "the layers have no video fills" }));
    }
    let doc = s.doc_mut()?;
    let mut changed = 0;
    let mut touched: Vec<NodeId> = Vec::new();
    for (id, i, duration) in work {
        let n = doc.get_mut(id)?;
        let Some(PaintKind::Video(v)) = n.fills.get_mut(i).map(|p| &mut p.kind) else { continue };
        if let Some(a) = autoplay {
            v.autoplay = a;
        }
        if let Some(l) = looping {
            v.looping = l;
        }
        if let Some(m) = muted {
            v.muted = m;
        }
        if let Some(t) = poster {
            v.poster = t.min(duration);
        }
        if let Some(m) = mode {
            v.scale_mode = m;
        }
        changed += 1;
        if !touched.contains(&id) {
            touched.push(id);
        }
    }
    // Like any paint edit, the layer leaves its paint style.
    for id in touched {
        doc.get_mut(id)?.styles.fill = None;
    }
    Ok(json!({"changed": changed}))
}

/// How long the video `v` plays (seconds), or the longest a poster time is when the file isn't
/// in the document or doesn't open.
fn duration_of(doc: &Document, v: &VideoPaint) -> f64 {
    doc.images.get(&v.video).and_then(|d| figcraft_media::shared(&v.video, &d.bytes).ok()).map_or(VideoPaint::MAX_POSTER, |m| m.info().duration)
}

fn info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = req_targets("video.info", s, p)?;
    let doc = s.doc()?;
    let mut out = Vec::new();
    'nodes: for id in ids {
        let n = doc.get(id)?;
        for (i, paint) in n.fills.iter().enumerate() {
            let PaintKind::Video(v) = &paint.kind else { continue };
            if out.len() >= MAX_LISTED {
                break 'nodes;
            }
            let mut e = json!({
                "id": id,
                "index": i,
                "video": v.video,
                "autoplay": v.autoplay,
                "loop": v.looping,
                "muted": v.muted,
                "poster": v.poster_time(),
                "mode": if v.scale_mode == ImageScaleMode::Fit { "fit" } else { "fill" },
            });
            match doc.images.get(&v.video) {
                None => e["error"] = json!("the video isn't in the file"),
                Some(d) => {
                    e["bytes"] = json!(d.bytes.len());
                    match figcraft_media::shared(&v.video, &d.bytes) {
                        Ok(m) => {
                            if let (Some(o), Value::Object(extra)) = (e.as_object_mut(), info_json(m.info())) {
                                o.extend(extra);
                            }
                        }
                        Err(err) => e["error"] = json!(err.to_string()),
                    }
                }
            }
            out.push(e);
        }
    }
    Ok(json!({"videos": out}))
}
