//! What the player does beyond moving between screens: opening links, and playing video.
//!
//! **Links.** Open-link actions open in a new tab or in the page itself ([`open_links`]).
//!
//! **Video.** A layer with a video fill plays while its screen shows (the presented frame or an
//! open overlay). Each such layer has a [`Player`] (the pure clock and state machine of
//! `figcraft-media`) driven by the player's own time: [`tick`] advances them once per frame, runs
//! the interactions of the "On media hit" times they crossed and of "On media end", and asks for
//! the next frame only while one is playing and its picture or a hit time is about to change.
//! The pictures are drawn in place: the screen's texture is rendered once with every video at the
//! picture it shows, and when a video moves on only the part of the texture it covers is rendered
//! again and patched in ([`patch`]) — so everything above, below and around a video stays
//! composited exactly as in a full render, at the price of the video's area only.
//!
//! When videos start over: each time a screen appears (navigating to it, going back, restarting
//! the prototype) its videos begin again, as their autoplay says; an overlay's videos begin when it
//! opens and are dropped when it closes, while the screen under it keeps playing. Editing the file
//! while presenting keeps a video playing unless its file or its autoplay, loop or muted setting
//! changed. Media actions act on the layer they name (else the layer the interaction is on) if it
//! is playing on the screen or an open overlay; on any other layer they do nothing. Sound is not
//! played: muting only changes the state the actions toggle.

use std::sync::Arc;

use figcraft_color::{PaintKind, VideoPaint};
use figcraft_doc::proto::{Action, Trigger};
use figcraft_doc::{Document, NodeId};
use figcraft_media::{Media, Player, PlayerConfig};
use serde_json::{Value, json};

use super::{FrameTex, Presenter};
use crate::FigApp;
use crate::i18n::trf;

// ---------------------------------------------------------------------------------------------
// Links.

/// After a player's frame: open the links its interactions asked for. Only web, mail and phone
/// links open (a file's link runs on the viewer's machine: `javascript:`, `file:` and the like are
/// refused, with a note saying so); each opens in a new tab, or in the page itself when its action
/// says so (only the web version has a page to replace; the desktop app opens the system browser
/// either way).
pub(super) fn open_links(app: &mut FigApp, p: &mut Presenter) {
    for (url, new_tab) in p.urls.drain(..) {
        let Some(link) = figcraft_doc::proto::web_link(&url) else {
            let shown: String = url.chars().take(80).collect();
            app.toast(trf("Didn't open the link “{url}”: only web, mail and phone links open", &[("url", &shown)]));
            continue;
        };
        if new_tab {
            app.services.open_url(&link);
        } else {
            app.services.open_url_here(&link);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Video.

/// The most videos the player plays at once (a screen with more shows the rest as posters).
const MAX_VIDEOS: usize = 32;
/// The most layers looked through for videos on one screen.
const MAX_LAYERS: usize = 200_000;
/// A video's patch covering more than this share of its texture is a full render instead.
const PATCH_LIMIT: f64 = 0.5;

/// One video layer being played.
struct Video {
    node: NodeId,
    /// The fill as the video started: a change of file, autoplay, loop or muted starts it over.
    paint: VideoPaint,
    media: Arc<Media>,
    player: Player,
    /// The interactions on "On media hit" (index, time) and on "On media end" (index) of the layer.
    hits: Vec<(usize, f64)>,
    ends: Vec<usize>,
}

impl Video {
    /// The time whose picture shows: the playing position, or (paused at the start, nothing
    /// played yet) the poster picture.
    fn shown_at(&self) -> f64 {
        let p = &self.player;
        if p.position() <= 0.0 && !p.playing() && !p.ended() { self.paint.poster_time().min(p.duration()) } else { p.position() }
    }

    /// Seconds until its picture changes or a hit time is crossed (None: not playing).
    fn next_wake(&self) -> Option<f64> {
        let p = &self.player;
        if !p.playing() {
            return None;
        }
        let pos = p.position();
        let next = self.media.frame_index_at(pos).saturating_add(1);
        let mut due = if next < self.media.info().frame_count { self.media.frame_start(next) - pos } else { p.duration() - pos };
        for (_, t) in &self.hits {
            if t.is_finite() && *t > pos {
                due = due.min(*t - pos);
            }
        }
        // (Never longer than a step: the player moves by at most that much per frame.)
        Some(due.clamp(0.0, figcraft_media::MAX_STEP))
    }
}

/// The videos the player plays and the counts tests read.
#[derive(Default)]
pub(super) struct State {
    videos: Vec<Video>,
    /// What `videos` was collected for: the layout revision, the screen and the open overlays.
    collected: Option<(u64, NodeId, Vec<NodeId>)>,
    /// Counts the times the videos started over (a screen shown): events of an older one are dropped.
    epoch: u64,
    /// Textures rendered whole while a video showed in them, and patched for a video moving on.
    renders: u64,
    patches: u64,
}

/// The layer `id`'s video fill the player plays: its first visible one.
fn video_of(doc: &Document, id: NodeId) -> Option<VideoPaint> {
    doc.node(id)?.fills.iter().find_map(|p| match &p.kind {
        PaintKind::Video(v) if p.visible => Some(v.clone()),
        _ => None,
    })
}

/// The visible layers under `root` with a video fill, in layer order.
fn find(doc: &Document, root: NodeId, out: &mut Vec<NodeId>) {
    let mut stack = vec![root];
    let mut seen = 0usize;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > MAX_LAYERS || out.len() >= MAX_VIDEOS {
            return;
        }
        let Some(n) = doc.node(id) else { continue };
        if !n.visible {
            continue;
        }
        if video_of(doc, id).is_some() {
            out.push(id);
        }
        stack.extend(n.children.iter().rev());
    }
}

/// The "On media hit" and "On media end" interactions of `id`.
fn interactions(doc: &Document, id: NodeId) -> (Vec<(usize, f64)>, Vec<usize>) {
    let (mut hits, mut ends) = (Vec::new(), Vec::new());
    for (k, it) in doc.node(id).map(|n| n.interactions.as_slice()).unwrap_or_default().iter().enumerate() {
        match it.trigger {
            Trigger::MediaHit { time } => hits.push((k, time)),
            Trigger::MediaEnd => ends.push(k),
            _ => {}
        }
    }
    (hits, ends)
}

/// Start the video on layer `id` (None: its file isn't in the file or doesn't open).
fn start(doc: &Document, id: NodeId, paint: &VideoPaint) -> Option<Video> {
    let data = doc.images.get(&paint.video)?;
    let media = figcraft_media::shared(&paint.video, &data.bytes).ok()?;
    let player = Player::new(media.info().duration, PlayerConfig { autoplay: paint.autoplay, looping: paint.looping, muted: paint.muted });
    Some(Video { node: id, paint: paint.clone(), media, player, hits: Vec::new(), ends: Vec::new() })
}

/// Whether a video started with `a` plays on unchanged with `b`.
fn same(a: &VideoPaint, b: &VideoPaint) -> bool {
    a.video == b.video && a.autoplay == b.autoplay && a.looping == b.looping && a.muted == b.muted
}

/// Bring the videos in line with what shows: those on the screen and in the open overlays. Videos
/// that stay keep playing; new ones start; those gone are dropped.
fn sync(p: &mut Presenter) {
    let Some(doc) = p.doc.as_ref() else { return };
    let overlays: Vec<NodeId> = p.overlays.iter().map(|o| o.id).collect();
    let key = (p.rev, p.current, overlays);
    if p.media.collected.as_ref() == Some(&key) {
        return;
    }
    let mut wanted = Vec::new();
    find(doc, p.current, &mut wanted);
    for o in &key.2 {
        find(doc, *o, &mut wanted);
    }
    wanted.truncate(MAX_VIDEOS);
    let mut old = std::mem::take(&mut p.media.videos);
    let mut videos = Vec::with_capacity(wanted.len());
    for id in wanted {
        let Some(paint) = video_of(doc, id) else { continue };
        let kept = old.iter().position(|v| v.node == id && same(&v.paint, &paint));
        let Some(mut v) = kept.map(|i| old.swap_remove(i)).or_else(|| start(doc, id, &paint)) else { continue };
        v.paint = paint;
        (v.hits, v.ends) = interactions(doc, id);
        videos.push(v);
    }
    p.media.videos = videos;
    p.media.collected = Some(key);
}

impl Presenter {
    /// The screen changed (shown, shown again, the prototype restarted): its videos start over.
    pub(super) fn media_reset(&mut self) {
        self.media.videos.clear();
        self.media.collected = None;
        self.media.epoch += 1;
    }

    /// A media action (play / pause, mute, skip): done on the video layer it targets, else on the
    /// layer the interaction is on. True when that layer is playing a video.
    pub(super) fn media_action(&mut self, a: &Action, node: NodeId) -> bool {
        sync(self);
        let target = a.media_target().unwrap_or(node);
        let Some(v) = self.media.videos.iter_mut().find(|v| v.node == target) else { return false };
        match a {
            Action::MediaPlayPause { mode, .. } => {
                if mode.apply(v.player.playing()) {
                    v.player.play();
                } else {
                    v.player.pause();
                }
            }
            Action::MediaMuteUnmute { mode, .. } => {
                let muted = mode.apply(v.player.muted());
                v.player.set_muted(muted);
            }
            Action::MediaSkip { seconds, .. } => v.player.skip(*seconds),
            Action::MediaSkipTo { time, .. } => v.player.seek(*time),
            _ => return false,
        }
        true
    }
}

/// Once per frame: move the videos to the player's time, run the interactions of the hit times
/// they crossed and of the ends they reached, and ask for the next frame while one is playing.
pub(super) fn tick(p: &mut Presenter, ctx: &egui::Context, now: f64) {
    sync(p);
    let epoch = p.media.epoch;
    let mut due: Vec<(NodeId, usize)> = Vec::new();
    for v in &mut p.media.videos {
        let times: Vec<f64> = v.hits.iter().map(|h| h.1).collect();
        let ev = v.player.advance(now, &times);
        due.extend(ev.hits.iter().filter_map(|i| v.hits.get(*i)).map(|(k, _)| (v.node, *k)));
        if ev.ended {
            due.extend(v.ends.iter().map(|k| (v.node, *k)));
        }
    }
    for (node, k) in due {
        // A screen change drops the rest: those videos are gone.
        if p.media.epoch != epoch {
            break;
        }
        p.fire(node, k, None, now);
    }
    // The next frame, only while something plays (egui wakes a little early: ask for that much
    // later, as the animated GIFs do).
    if let Some(secs) = p.media.videos.iter().filter_map(Video::next_wake).reduce(f64::min) {
        crate::repaint_in(ctx, secs + f64::from(ctx.input(|i| i.predicted_dt)));
    }
}

/// A video playing under a texture's root, the picture it shows and which that is.
pub(super) struct Live {
    node: NodeId,
    at: f64,
    frame: u32,
}

/// The videos playing under `root` (and not under the `hide` layers), with the picture each shows.
pub(super) fn live(p: &Presenter, doc: &Document, root: NodeId, hide: &[NodeId]) -> Vec<Live> {
    p.media
        .videos
        .iter()
        .filter(|v| doc.is_ancestor(root, v.node) && !hide.iter().any(|h| doc.is_ancestor(*h, v.node)))
        .map(|v| {
            let at = v.shown_at();
            Live { node: v.node, at, frame: v.media.frame_index_at(at) }
        })
        .collect()
}

/// Which pictures a texture holds: (video, picture) for each.
pub(super) fn painted(live: &[Live]) -> Vec<(NodeId, u32)> {
    live.iter().map(|l| (l.node, l.frame)).collect()
}

/// `doc` with each video in `live` set to the time it shows (the renderer draws a video fill at
/// its poster time).
pub(super) fn posed(doc: &Document, live: &[Live]) -> Document {
    let mut d = doc.clone();
    for l in live {
        let Some(n) = d.node_mut(l.node) else { continue };
        for paint in &mut n.fills {
            if let PaintKind::Video(v) = &mut paint.kind
                && paint.visible
            {
                v.poster = l.at;
                break;
            }
        }
    }
    d
}

/// Count a texture rendered whole with videos in it.
pub(super) fn rendered(p: &mut Presenter, live: &[Live]) {
    if !live.is_empty() {
        p.media.renders += 1;
    }
}

/// The scale `figcraft_render` renders at when asked for `s`.
pub(super) fn effective(s: f64) -> f64 {
    if s.is_finite() && s > 0.0 { s.min(64.0) } else { 1.0 }
}

/// Bring the texture `t` (rendered from `doc`, rooted at `root`, `time` seconds in) up to date for
/// the videos in `live` that show another picture than it holds: each one's area of the texture is
/// rendered again and patched in. False when that isn't worth it or can't be done (the caller
/// renders the texture whole).
pub(super) fn patch(p: &mut Presenter, doc: &Document, root: NodeId, t: &mut FrameTex, live: &[Live], time: f64) -> bool {
    let s = t.px;
    let [w, h] = t.size;
    if !(s.is_finite() && s > 0.0 && w > 0 && h > 0) {
        return false;
    }
    let moved: Vec<&Live> = live.iter().filter(|l| !t.videos.contains(&(l.node, l.frame))).collect();
    let mut patches: Vec<([usize; 2], egui::ColorImage)> = Vec::new();
    let mut area = 0.0;
    for l in moved {
        // Where the video paints, with its effects, in texture pixels (a pixel more all round).
        let Some(b) = figcraft_render::raster_bounds(doc, l.node) else { return false };
        let (x0, y0) = (((b.x0 - t.bounds.x0) * s).floor() - 1.0, ((b.y0 - t.bounds.y0) * s).floor() - 1.0);
        let (x1, y1) = (((b.x1 - t.bounds.x0) * s).ceil() + 1.0, ((b.y1 - t.bounds.y0) * s).ceil() + 1.0);
        let (x0, y0, x1, y1) = (x0.max(0.0), y0.max(0.0), x1.min(w as f64), y1.min(h as f64));
        if !(x1 > x0 && y1 > y0) {
            // Entirely outside what the texture holds: nothing to show.
            continue;
        }
        area += (x1 - x0) * (y1 - y0);
        if area > PATCH_LIMIT * (w * h) as f64 {
            return false;
        }
        let region = figcraft_geom::Rect::new(t.bounds.x0 + x0 / s, t.bounds.y0 + y0 / s, t.bounds.x0 + x1 / s, t.bounds.y0 + y1 / s);
        let Some(pix) = figcraft_render::render_area_at(doc, &[root], region, s, time) else { return false };
        if f64::from(pix.width()) != x1 - x0 || f64::from(pix.height()) != y1 - y0 {
            return false;
        }
        patches.push(([x0 as usize, y0 as usize], crate::render_worker::screen_image(&pix, doc.color_profile)));
    }
    for (at, img) in patches {
        t.tex.set_partial(at, img, egui::TextureOptions::LINEAR);
        p.media.patches += 1;
    }
    t.videos = painted(live);
    true
}

/// The videos for `ui.inspect`.
pub(super) fn inspect(p: &Presenter) -> Value {
    let videos: Vec<Value> = p
        .media
        .videos
        .iter()
        .map(|v| {
            let pl = &v.player;
            json!({
                "id": v.node,
                "playing": pl.playing(),
                "ended": pl.ended(),
                "muted": pl.muted(),
                "looping": pl.looping(),
                "position": pl.position(),
                "duration": pl.duration(),
                "frame": v.media.frame_index_at(v.shown_at()),
                "frames": v.media.info().frame_count,
            })
        })
        .collect();
    json!({"videos": videos, "renders": p.media.renders, "patches": p.media.patches})
}

#[cfg(test)]
#[path = "present_link_tests.rs"]
mod link_tests;

#[cfg(test)]
#[path = "present_media_tests.rs"]
mod tests;
