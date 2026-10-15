//! The presenter's screens panel: the page's top-level frames in a list at the left of the
//! prototype, each with a thumbnail, grouped under the flows, to jump to one.
//!
//! A child module of `present`.

use std::collections::HashMap;

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use figcraft_doc::proto::{Easing, Flow, Transition};
use figcraft_doc::{Document, NodeId, NodeKind};

use super::Presenter;
use crate::i18n::tr;
use crate::icons::{self, Icon};
use crate::theme::{ACCENT, Tokens, regular, semibold};

/// The panel's width.
pub const WIDTH: f32 = 232.0;
/// A screen's row, and a group's title row (points).
const ROW_H: f32 = 56.0;
const TITLE_H: f32 = 28.0;
/// The box a thumbnail fits into (points).
const THUMB: f32 = 40.0;
/// The most thumbnails kept: past this the ones out of view are dropped.
const MAX_THUMBS: usize = 300;

/// Screens of the page that go together: those from a flow's starting screen up to the next flow's
/// start in the order the arrow keys step through them, or those before the first flow's start.
pub struct Group {
    /// The flow that starts the group (`None`: screens before the first flow's start, or all of
    /// them when the page has no flows).
    pub flow: Option<Flow>,
    pub screens: Vec<NodeId>,
}

/// The page's screens in groups. The order is the sequence the arrow keys step through
/// ([`super::ux::sequence`]: the top-level frames in reading order, not the layers panel's order),
/// so the list, the "3 / 12" in the bar and the keys all agree. Flows don't partition a page by
/// themselves (a flow is a starting point, and the screens it reaches overlap with other flows'),
/// so a group is the run of screens from a flow's start to the next flow's start: the way a page is
/// usually laid out, a flow's screens in a row after its first. A flow whose start isn't one of the
/// page's top-level frames, or repeats another flow's start, has no group.
pub fn groups(doc: &Document, current: NodeId) -> Vec<Group> {
    let seq = super::ux::sequence(doc, current);
    let flows: Vec<Flow> = match doc.page_of(current).and_then(|pg| doc.node(pg)).map(|n| &n.kind) {
        Some(NodeKind::Page { flows, .. }) => flows.clone(),
        _ => Vec::new(),
    };
    let mut starts: Vec<(usize, Flow)> = Vec::new();
    for f in flows {
        if let Some(i) = seq.iter().position(|s| *s == f.start)
            && !starts.iter().any(|(j, _)| *j == i)
        {
            starts.push((i, f));
        }
    }
    starts.sort_by_key(|(i, _)| *i);
    let slice = |a: usize, b: usize| seq.get(a..b).map(<[NodeId]>::to_vec).unwrap_or_default();
    let mut out = Vec::new();
    let first = starts.first().map_or(seq.len(), |(i, _)| *i);
    if first > 0 {
        out.push(Group { flow: None, screens: slice(0, first) });
    }
    for (k, (i, f)) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(seq.len(), |(j, _)| *j);
        out.push(Group { flow: Some(f.clone()), screens: slice(*i, end) });
    }
    out
}

/// Show `frame` at once, as the arrow keys do (the history keeps where this came from, so Back
/// returns there): nothing when it is the screen already showing, except that an overlay open over
/// it closes (what navigating to a screen does).
pub fn jump(p: &mut Presenter, frame: NodeId, now: f64) {
    if p.doc.as_ref().is_none_or(|d| !d.contains(frame)) || (frame == p.current && p.overlays.is_empty()) {
        return;
    }
    p.go(frame, Transition::Instant, 0.0, Easing::Linear, now, true, false);
}

// ---------------------------------------------------------------------------------------------
// Thumbnails.

/// A screen's thumbnail, drawn from the editor's document.
struct Thumb {
    /// `None`: the screen couldn't be drawn (it isn't tried again until the document changes).
    tex: Option<egui::TextureHandle>,
    /// The size it shows at (points).
    size: Vec2,
    /// The document changed since: the picture stays until the next one is drawn.
    stale: bool,
}

/// The thumbnails of the screens panel. They draw lazily: only the rows in view, one per frame
/// (the UI thread does it, so a screen that takes long makes the next wait: the pause after each
/// is twenty times what it took), and an edit in the editor draws the ones in view again while the
/// old pictures stay.
#[derive(Default)]
pub struct Thumbs {
    /// The editor's document they were drawn from, as the address of its `Arc`.
    source: usize,
    map: HashMap<NodeId, Thumb>,
    /// The UI clock before which the next one waits.
    next_at: f64,
    /// Drawn in the last frame, and rows in view still waiting for one.
    pub drawn: usize,
    pub pending: usize,
    /// The slowest (ms) and the total drawn so far (for `ui.inspect` and the frame-cost test).
    pub slowest_ms: f64,
    pub total: usize,
}

impl Thumbs {
    /// How many thumbnails are drawn and kept.
    pub fn count(&self) -> usize {
        self.map.values().filter(|t| t.tex.is_some()).count()
    }
}

/// Draw `id` of `doc` small: at most [`THUMB`] points on its longer side (never magnified).
fn draw_thumb(ctx: &egui::Context, doc: &Document, id: NodeId) -> Thumb {
    let none = Thumb { tex: None, size: Vec2::ZERO, stale: false };
    let Some(bounds) = figcraft_render::export_bounds(doc, id) else { return none };
    let (w, h) = (bounds.width().max(1.0), bounds.height().max(1.0));
    let per_unit = (f64::from(THUMB) / w.max(h)).min(1.0);
    let ppp = f64::from(ctx.pixels_per_point());
    let Some(pix) = figcraft_render::render_node_at(doc, id, per_unit * ppp, 0.0) else { return none };
    let img = crate::render_worker::screen_image(&pix, doc.color_profile);
    let tex = ctx.load_texture(format!("present-thumb-{}", id.0), img, egui::TextureOptions::LINEAR);
    Thumb { tex: Some(tex), size: vec2((w * per_unit) as f32, (h * per_unit) as f32), stale: false }
}

// ---------------------------------------------------------------------------------------------
// The panel.

/// What a row of the list is.
enum Item {
    /// A group's title: the flow (click: play it from its start), or "other screens".
    Title { flow: Option<Flow>, name: String },
    /// A screen and its place in the list.
    Screen { id: NodeId, number: usize },
}

impl Item {
    fn height(&self) -> f32 {
        match self {
            Item::Title { .. } => TITLE_H,
            Item::Screen { .. } => ROW_H,
        }
    }
}

/// `text` on one line, cut with an ellipsis at `width`.
fn one_line(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::single_section(text.to_string(), TextFormat::simple(font, color));
    job.wrap = TextWrapping::truncate_at_width(width.max(8.0));
    ui.painter().layout_job(job)
}

/// The panel's contents: its title, then each group with its screens (only the rows in view are
/// laid out); the current screen is highlighted and scrolled to when it changes.
pub fn show(p: &mut Presenter, ui: &mut Ui, now: f64) {
    let t = Tokens::get(ui.ctx());
    let Some(doc) = p.doc.clone() else { return };
    let groups = groups(&doc, p.current);
    let total: usize = groups.iter().map(|g| g.screens.len()).sum();
    let any_flow = groups.iter().any(|g| g.flow.is_some());
    let mut items: Vec<Item> = Vec::new();
    let mut number = 0;
    for g in &groups {
        match &g.flow {
            Some(f) => items.push(Item::Title { flow: Some(f.clone()), name: f.name.clone() }),
            None if any_flow => items.push(Item::Title { flow: None, name: tr("Other screens").to_string() }),
            None => {}
        }
        for &id in &g.screens {
            number += 1;
            items.push(Item::Screen { id, number });
        }
    }
    // A title row: "Screens" and how many.
    let (head, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    ui.painter().text(pos2(head.min.x + 16.0, head.center().y), egui::Align2::LEFT_CENTER, tr("Screens"), semibold(11.5), t.text);
    ui.painter().text(pos2(head.max.x - 16.0, head.center().y), egui::Align2::RIGHT_CENTER, total.to_string(), regular(11.0), t.text_secondary);
    ui.painter().line_segment([head.left_bottom(), head.right_bottom()], Stroke::new(1.0, t.border));
    // The thumbnails start over (stale) when the editor's document changed.
    let source = p.source.as_ref().map_or(0, |a| std::sync::Arc::as_ptr(a) as usize);
    if p.ux.thumbs.source != source {
        p.ux.thumbs.source = source;
        p.ux.thumbs.map.values_mut().for_each(|t| t.stale = true);
        let ids: std::collections::HashSet<NodeId> =
            items.iter().filter_map(|i| if let Item::Screen { id, .. } = i { Some(*id) } else { None }).collect();
        p.ux.thumbs.map.retain(|id, _| ids.contains(id));
    }
    p.ux.thumbs.drawn = 0;
    p.ux.thumbs.pending = 0;
    let (current, start) = (p.current, p.start);
    let scroll_to = p.ux.panel_seen != Some(current);
    let mut jump_to: Option<NodeId> = None;
    let mut restart_at: Option<NodeId> = None;
    let mut scrolled = false;
    let mut visible: Vec<NodeId> = Vec::new();
    let mut waiting: Vec<NodeId> = Vec::new();
    let heights: Vec<f32> = items.iter().map(Item::height).collect();
    let content_h = 8.0 + heights.iter().sum::<f32>() + 8.0;
    let ctx = ui.ctx().clone();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_viewport(ui, |ui, viewport| {
        let (left, top, w) = (ui.max_rect().left(), ui.max_rect().top(), ui.available_width());
        ui.set_height(content_h.max(viewport.height()));
        let mut y = 8.0;
        for (item, h) in items.iter().zip(&heights) {
            let r = Rect::from_min_size(pos2(left, top + y), vec2(w, *h));
            y += h;
            let on_screen = y > viewport.min.y && y - h < viewport.max.y;
            match item {
                Item::Screen { id, .. } if *id == current && scroll_to => {
                    ui.scroll_to_rect(r.expand2(vec2(0.0, 12.0)), Some(Align::Center));
                    scrolled = true;
                }
                _ => {}
            }
            if !on_screen {
                continue;
            }
            match item {
                Item::Title { flow, name } => {
                    let resp = ui.interact(r, ui.id().with(("pscreens_title", flow.as_ref().map(|f| f.start))), Sense::click());
                    if resp.hovered() && flow.is_some() {
                        ui.painter().rect_filled(r.shrink2(vec2(6.0, 0.0)), CornerRadius::same(5), t.hover);
                    }
                    let playing = flow.as_ref().is_some_and(|f| f.start == start);
                    let x = if playing { r.min.x + 28.0 } else { r.min.x + 16.0 };
                    if playing {
                        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 20.0, r.center().y), vec2(10.0, 10.0)), Icon::Play, ACCENT);
                    }
                    let galley = one_line(ui, name, semibold(11.0), t.text_secondary, r.max.x - x - 12.0);
                    ui.painter().galley(pos2(x, r.center().y - galley.size().y / 2.0), galley, t.text_secondary);
                    if let Some(f) = flow {
                        let tip = f.description.trim();
                        if !tip.is_empty() {
                            let _ = resp.clone().on_hover_text(tip);
                        }
                        if resp.clicked() {
                            restart_at = Some(f.start);
                        }
                        p.ux.note(format!("flow:{}", f.start.0), r);
                    }
                }
                Item::Screen { id, number } => {
                    let resp = ui.interact(r, ui.id().with(("pscreens_row", *id)), Sense::click());
                    let on = *id == current;
                    let pill = r.shrink2(vec2(6.0, 2.0));
                    if on {
                        ui.painter().rect_filled(pill, CornerRadius::same(6), t.selected_row);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(pill, CornerRadius::same(6), t.hover);
                    }
                    let ink = if on { t.accent_text } else { t.text };
                    ui.painter().text(
                        pos2(r.min.x + 30.0, r.center().y),
                        egui::Align2::RIGHT_CENTER,
                        number.to_string(),
                        regular(11.0),
                        t.text_secondary,
                    );
                    // The thumbnail in its box (a placeholder until it is drawn).
                    let boxr = Rect::from_center_size(pos2(r.min.x + 38.0 + THUMB / 2.0, r.center().y), vec2(THUMB, THUMB));
                    visible.push(*id);
                    match p.ux.thumbs.map.get(id) {
                        Some(Thumb { tex: Some(tex), size, stale }) => {
                            let img = Rect::from_center_size(boxr.center(), *size);
                            ui.painter().image(tex.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                            ui.painter().rect_stroke(img, CornerRadius::same(2), Stroke::new(1.0, t.border_strong), StrokeKind::Outside);
                            if *stale {
                                waiting.push(*id);
                            }
                        }
                        Some(Thumb { tex: None, stale, .. }) => {
                            ui.painter().rect_filled(boxr, CornerRadius::same(3), t.field);
                            if *stale {
                                waiting.push(*id);
                            }
                        }
                        None => {
                            ui.painter().rect_filled(boxr, CornerRadius::same(3), t.field);
                            waiting.push(*id);
                        }
                    }
                    let name = doc.node(*id).map(|n| n.name.clone()).unwrap_or_default();
                    let nx = boxr.max.x + 10.0;
                    let galley = one_line(ui, &name, regular(11.5), ink, r.max.x - nx - 14.0);
                    ui.painter().galley(pos2(nx, r.center().y - galley.size().y / 2.0), galley, ink);
                    let _ = resp.clone().on_hover_text(&name);
                    if resp.clicked() {
                        jump_to = Some(*id);
                    }
                    p.ux.note(format!("screen:{}", id.0), r);
                }
            }
        }
    });
    if scrolled {
        p.ux.panel_seen = Some(current);
    }
    // One thumbnail a frame, when the pause after the last one is over.
    p.ux.thumbs.pending = waiting.len();
    if let Some(&id) = waiting.first()
        && now >= p.ux.thumbs.next_at
        && let Some(editor) = p.original.as_ref()
    {
        #[cfg(not(target_arch = "wasm32"))]
        let t0 = std::time::Instant::now();
        let thumb = draw_thumb(&ctx, editor, id);
        #[cfg(not(target_arch = "wasm32"))]
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        #[cfg(target_arch = "wasm32")]
        let ms = 0.0;
        let th = &mut p.ux.thumbs;
        th.map.insert(id, thumb);
        th.next_at = now + ms / 1000.0 * 20.0;
        th.drawn = 1;
        th.total += 1;
        th.slowest_ms = th.slowest_ms.max(ms);
        th.pending -= 1;
        if th.map.len() > MAX_THUMBS {
            th.map.retain(|k, _| visible.contains(k));
        }
    }
    if p.ux.thumbs.pending > 0 {
        crate::repaint_in(&ctx, (p.ux.thumbs.next_at - now).max(0.0));
    }
    if let Some(f) = restart_at {
        p.restart(f, now);
    } else if let Some(id) = jump_to {
        jump(p, id, now);
    }
}
