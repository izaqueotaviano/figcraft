//! The colour picker's video fill: choosing the file, how it fills the layer, the poster picture
//! the editor and exports show, and how it plays in a prototype (autoplay, loop, muted). A child
//! of `colorpicker`, which writes the paint back.

use egui::{Id, Ui};
use figcraft_color::{ImageScaleMode, Paint, PaintKind, VideoPaint};
use figcraft_doc::proto::seconds_text;
use figcraft_media::{MediaInfo, MediaKind};
use serde_json::json;

use super::{PickerTarget, write};
use crate::FigApp;
use crate::a11y::Tip;
use crate::i18n::tr;
use crate::right::{menu_item, menu_popup};
use crate::theme::{Tokens, regular};
use crate::widgets::{self, FieldEdit, Lead};

/// The files the video dialogs list (animated GIF, WebP and PNG play too; other video formats are
/// listed so that opening one says why it can't play).
pub(crate) const EXTENSIONS: &[&str] = &["avi", "mov", "mp4", "m4v", "webm", "mkv", "gif", "webp", "png", "apng"];

/// Ask for a video file and keep it in the document: the video fill for it. `None` when nothing
/// was chosen or the file can't be used (a message says why).
pub(super) fn choose(app: &mut FigApp) -> Option<VideoPaint> {
    let path = app.services.pick_open(tr("Choose video"), EXTENSIONS)?;
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            app.toast(e.to_string());
            return None;
        }
    };
    let added =
        app.session.doc_mut().map_err(|e| e.to_string()).and_then(|d| figcraft_engine::cmd::video::add_video(d, bytes).map_err(|e| e.to_string()));
    match added {
        Ok((hash, info)) => match figcraft_engine::cmd::video::paint_from_params(&hash, &json!({}), info.duration).kind {
            PaintKind::Video(v) => Some(v),
            _ => None,
        },
        Err(m) => {
            app.toast(m);
            None
        }
    }
}

/// What a file is, on one line ("Motion-JPEG · 1280 × 720 · 4 s · 25 fps").
fn describe(i: &MediaInfo) -> String {
    let mut s = format!("{} · {} × {} · {} s", i.codec, i.width, i.height, seconds_text(i.duration));
    if i.kind == MediaKind::Video && i.frame_rate.is_finite() && i.frame_rate > 0.0 {
        s.push_str(&format!(" · {} fps", seconds_text(i.frame_rate)));
    }
    s
}

/// The video fill's controls under the paint type row; edits are written to the paint.
pub(super) fn controls(app: &mut FigApp, ui: &mut Ui, target: &PickerTarget, paint: &mut Paint, v: &VideoPaint) {
    let t = Tokens::get(ui.ctx());
    let mut v = v.clone();
    let mut changed = false;
    let media = app.session.doc().ok().and_then(|d| d.images.get(&v.video)).map(|d| figcraft_media::shared(&v.video, &d.bytes));
    let line = match &media {
        Some(Ok(m)) => describe(m.info()),
        Some(Err(e)) => e.to_string(),
        None => tr("The video isn't in this file").to_string(),
    };
    let duration = match &media {
        Some(Ok(m)) => m.info().duration,
        _ => VideoPaint::MAX_POSTER,
    };
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.add(egui::Label::new(egui::RichText::new(line).font(regular(11.0)).color(t.text_secondary)).truncate());
    });
    if let Some(Ok(m)) = &media
        && m.info().has_audio
    {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            widgets::label(ui, tr("This file has sound, which isn't played yet."), t.text_secondary);
        });
    }
    // How it fills the layer, and another file.
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let resp = widgets::dropdown(ui, if v.scale_mode == ImageScaleMode::Fit { tr("Fit") } else { tr("Fill") }, 100.0, None);
        menu_popup(&resp).show(|ui| {
            for (label, mode) in [(tr("Fill"), ImageScaleMode::Fill), (tr("Fit"), ImageScaleMode::Fit)] {
                if menu_item(ui, label, "") {
                    v.scale_mode = mode;
                    changed = true;
                    ui.close();
                }
            }
        });
        if ui.small_button(tr("Choose another video…")).clicked()
            && let Some(other) = choose(app)
        {
            v.video = other.video;
            v.poster = 0.0;
            changed = true;
        }
    });
    // The picture the editor and exports show.
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        let (lr, _) = ui.allocate_exact_size(egui::vec2(88.0, 18.0), egui::Sense::hover());
        ui.painter().text(lr.left_center(), egui::Align2::LEFT_CENTER, tr("Poster frame"), regular(11.0), t.text);
        if let Some(FieldEdit::Set(x) | FieldEdit::Scrub(x, _)) =
            widgets::number_field(ui, Id::new("cp_video_poster"), Lead::None, Some(v.poster_time()), "s", 96.0, 0.1)
            && x.is_finite()
        {
            v.poster = ((x.clamp(0.0, duration) * 100.0).round() / 100.0).min(duration);
            changed = true;
        }
    });
    let flags = [
        (tr("Autoplay"), v.autoplay, tr("It plays when its screen appears in the prototype; turn this off to start it with an action.")),
        (tr("Loop"), v.looping, tr("It starts over when it ends. A video that loops never ends, so “On media end” never runs.")),
        (tr("Start muted"), v.muted, tr("Sound isn't played yet: this is the state that “Mute / unmute” actions change.")),
    ];
    for (i, (label, on, tip)) in flags.into_iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            if widgets::checkbox_resp(ui, on, label).tip(tip).clicked() {
                match i {
                    0 => v.autoplay = !on,
                    1 => v.looping = !on,
                    _ => v.muted = !on,
                }
                changed = true;
            }
        });
    }
    if changed {
        paint.kind = PaintKind::Video(v);
        write(app, target, paint, false);
    }
}
