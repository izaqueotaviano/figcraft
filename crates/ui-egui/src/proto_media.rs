//! The media parts of the interaction editor: the time of an "On media hit" trigger, and the
//! settings of the four media actions (which video layer they control, play or pause, how far or
//! where to skip). Every edit runs the same `proto.*` commands as the rest of the editor. (A
//! child of `proto_panel`, which has the helpers it shares.)

use egui::{Id, Ui};
use figcraft_color::PaintKind;
use figcraft_doc::proto::{Action, MAX_MEDIA_SECONDS, MediaToggle};
use figcraft_doc::{Document, NodeId};
use serde_json::json;

use super::patch;
use crate::FigApp;
use crate::i18n::tr;
use crate::icons::Icon;
use crate::proto_editor::{Slot, set_action};
use crate::right::{menu_item, menu_popup, row};
use crate::theme::Tokens;
use crate::widgets::{self, FieldEdit, Lead};

/// The most video layers a menu lists.
const MAX_LISTED: usize = 200;

/// Whether layer `id` has a video fill.
pub(crate) fn has_video(doc: &Document, id: NodeId) -> bool {
    doc.node(id).is_some_and(|n| n.fills.iter().any(|p| matches!(p.kind, PaintKind::Video(_))))
}

/// The layers with a video fill in the screen `node` is on, in layer order.
pub(crate) fn video_layers(doc: &Document, node: NodeId) -> Vec<NodeId> {
    let Some(root) = doc.top_level_frame(node) else { return Vec::new() };
    doc.subtree(root).into_iter().filter(|i| has_video(doc, *i)).take(MAX_LISTED).collect()
}

/// The layer a new media action on `node` controls: none (the layer itself) when it has a video
/// fill, else the first video layer of its screen.
pub(crate) fn default_target(doc: &Document, node: NodeId) -> Option<NodeId> {
    if has_video(doc, node) { None } else { video_layers(doc, node).into_iter().next() }
}

/// `action` pointed at `to` (`None`: the layer it is on).
fn retargeted(action: &Action, to: Option<NodeId>) -> Action {
    let mut a = action.clone();
    a.set_media_target(to);
    a
}

/// The "Time" field of an "On media hit" trigger, and a note when the layer has no video.
pub(crate) fn hit_rows(app: &mut FigApp, ui: &mut Ui, doc: &Document, node: NodeId, k: usize, time: f64) {
    let t = Tokens::get(ui.ctx());
    row(ui, 32.0, |ui| {
        widgets::label(ui, tr("Time"), t.text_secondary);
        let id = Id::new(("proto_media_hit", node, k));
        if let Some(FieldEdit::Set(v) | FieldEdit::Scrub(v, _)) = widgets::number_field(ui, id, Lead::None, Some(time), "s", 96.0, 0.1)
            && v.is_finite()
        {
            let secs = (v.clamp(0.0, MAX_MEDIA_SECONDS) * 100.0).round() / 100.0;
            patch(app, node, k, json!({"trigger": {"type": "mediaHit", "time": secs}}));
        }
    });
    if !has_video(doc, node) {
        note(ui, tr("This layer has no video fill, so this never runs."));
    }
}

/// A line of small secondary text.
fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    row(ui, 24.0, |ui| {
        ui.add(egui::Label::new(egui::RichText::new(text).font(crate::theme::regular(10.0)).color(t.text_secondary)).wrap());
    });
}

/// The settings of a media action at `slot`: its video layer, then what it does.
pub(crate) fn action_rows(app: &mut FigApp, ui: &mut Ui, doc: &Document, slot: &Slot, action: &Action) {
    let target = action.media_target();
    widgets::sublabels(ui, tr("Video layer"), None);
    row(ui, 32.0, |ui| {
        let text = match target {
            None => tr("This layer").to_string(),
            Some(d) => doc.node(d).map_or_else(|| tr("Missing layer").to_string(), |n| n.name.clone()),
        };
        let resp = widgets::dropdown(ui, &text, ui.available_width(), Some(Icon::Play));
        menu_popup(&resp).show(|ui| {
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                let mark = |on: bool, s: &str| if on { format!("✓ {s}") } else { s.to_string() };
                if menu_item(ui, &mark(target.is_none(), tr("This layer")), "") {
                    set_action(app, slot, retargeted(action, None));
                    ui.close();
                }
                for d in video_layers(doc, slot.node) {
                    let name = doc.node(d).map(|n| n.name.clone()).unwrap_or_default();
                    if menu_item(ui, &mark(Some(d) == target, &name), "") {
                        set_action(app, slot, retargeted(action, Some(d)));
                        ui.close();
                    }
                }
            });
        });
    });
    if !has_video(doc, target.unwrap_or(slot.node)) {
        note(ui, tr("That layer has no video fill, so this does nothing."));
    }
    match action {
        Action::MediaPlayPause { target, mode } => {
            let (target, cur) = (*target, *mode);
            mode_row(app, ui, slot, tr("Playback"), [tr("Play or pause"), tr("Play"), tr("Pause")], cur, move |m| Action::MediaPlayPause {
                target,
                mode: m,
            });
        }
        Action::MediaMuteUnmute { target, mode } => {
            let (target, cur) = (*target, *mode);
            mode_row(app, ui, slot, tr("Sound"), [tr("Mute or unmute"), tr("Mute"), tr("Unmute")], cur, move |m| Action::MediaMuteUnmute {
                target,
                mode: m,
            });
            note(ui, tr("Sound isn't played yet: this only changes the video's muted state."));
        }
        Action::MediaSkip { target, seconds } => {
            let (target, seconds) = (*target, *seconds);
            widgets::sublabels(ui, tr("Skip by (seconds)"), None);
            row(ui, 32.0, |ui| {
                if let Some(FieldEdit::Set(v) | FieldEdit::Scrub(v, _)) =
                    widgets::number_field(ui, slot.id("proto_media_skip"), Lead::None, Some(seconds), "s", 96.0, 1.0)
                    && v.is_finite()
                {
                    let v = (v.clamp(-MAX_MEDIA_SECONDS, MAX_MEDIA_SECONDS) * 100.0).round() / 100.0;
                    set_action(app, slot, Action::MediaSkip { target, seconds: v });
                }
            });
            note(ui, tr("A negative number skips backward."));
        }
        Action::MediaSkipTo { target, time } => {
            let (target, time) = (*target, *time);
            widgets::sublabels(ui, tr("Skip to (seconds)"), None);
            row(ui, 32.0, |ui| {
                if let Some(FieldEdit::Set(v) | FieldEdit::Scrub(v, _)) =
                    widgets::number_field(ui, slot.id("proto_media_to"), Lead::None, Some(time), "s", 96.0, 0.1)
                    && v.is_finite()
                {
                    let v = (v.clamp(0.0, MAX_MEDIA_SECONDS) * 100.0).round() / 100.0;
                    set_action(app, slot, Action::MediaSkipTo { target, time: v });
                }
            });
        }
        _ => {}
    }
}

/// A dropdown of the three things a toggling media action can do (`labels` in the order of
/// [`MediaToggle::ALL`]).
fn mode_row(app: &mut FigApp, ui: &mut Ui, slot: &Slot, title: &str, labels: [&str; 3], cur: MediaToggle, make: impl Fn(MediaToggle) -> Action) {
    widgets::sublabels(ui, title, None);
    row(ui, 32.0, |ui| {
        let shown = MediaToggle::ALL.iter().position(|m| *m == cur).and_then(|i| labels.get(i)).copied().unwrap_or("");
        let resp = widgets::dropdown(ui, shown, ui.available_width(), None);
        menu_popup(&resp).show(|ui| {
            for (m, l) in MediaToggle::ALL.iter().zip(labels) {
                let text = if *m == cur { format!("✓ {l}") } else { l.to_string() };
                if menu_item(ui, &text, "") {
                    set_action(app, slot, make(*m));
                    ui.close();
                }
            }
        });
    });
}
