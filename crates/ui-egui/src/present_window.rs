//! Presenting in a window of its own: the prototype plays in a second OS window while the editor
//! keeps working in the main one, and follows the editor's edits as the preview pane does.
//!
//! The window is an egui deferred viewport the main window keeps open ([`keep_open`], from
//! `windows::show`) and whose frames draw the presenter that `app.ui.present` holds
//! ([`draw`], through the app `windows::install` keeps where the viewport's callback finds it).
//! While it is open the main window shows the editor ([`covers_window`] is false); keys and the
//! pointer go to the window they happen in. Where viewports are embedded or unsupported (the web
//! build, headless tests), with a screen reader running, or from a window that isn't the main one,
//! the presentation stays full window in the window it started in.
//!
//! A child module of `present`.

use egui::{Context, Event, Ui, ViewportBuilder, ViewportCommand, ViewportId};

use super::Presenter;
use crate::FigApp;
use crate::i18n::tr;

/// The presenter window's viewport.
pub fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("figcraft_present_window")
}

/// The presenter takes the whole window of the editor (it plays full window rather than in a window
/// of its own).
pub fn covers_window(app: &FigApp) -> bool {
    app.ui.present.as_ref().is_some_and(|p| !p.ux.window)
}

/// The presenter plays in a window of its own and shows comments: the open comment thread is its.
pub fn owns_comments(app: &FigApp) -> bool {
    app.ui.present.as_ref().is_some_and(|p| p.ux.window && p.view.comments)
}

/// The presenter plays in a window of its own.
pub fn is_open(app: &FigApp) -> bool {
    app.ui.present.as_ref().is_some_and(|p| p.ux.window)
}

/// Why a presentation can't play in a window of its own here (shown as a message), `None` when it
/// can.
fn unsupported(app: &FigApp, ctx: &Context) -> Option<&'static str> {
    if ctx.embed_viewports() {
        Some(tr("This build has one window: the prototype presents here"))
    } else if crate::a11y::active(ctx) {
        // (eframe 0.36 gives only the main window an accessibility tree.)
        Some(tr("With a screen reader running, the prototype presents in this window"))
    } else if app.windows.current != 0 {
        Some(tr("A window of its own opens from the main window: the prototype presents here"))
    } else {
        None
    }
}

/// Play the presentation `app.ui.present` holds in a window of its own (`on`) or in the window of
/// the editor again. A message says why when it can't (the presentation then goes on where it is).
pub fn apply(app: &mut FigApp, ctx: &Context, on: bool) {
    if let Some(mut p) = app.ui.present.take() {
        apply_to(app, ctx, &mut p, on);
        app.ui.present = Some(p);
    }
}

/// [`apply`] to the presenter `p` while a frame of it draws (it is out of `app.ui.present` then).
pub fn apply_to(app: &mut FigApp, ctx: &Context, p: &mut Presenter, on: bool) {
    if on && let Some(why) = unsupported(app, ctx) {
        app.toast(why);
        return;
    }
    if p.ux.window == on {
        return;
    }
    p.ux.window = on;
    p.ux.window_doc = app.session.ds().map(|d| d.uid).unwrap_or(0);
    if !on {
        // The window closes with its last frame; the editor's window shows the presentation.
        ctx.send_viewport_cmd_to(viewport_id(), ViewportCommand::Close);
    }
    // Both windows change what they show.
    ctx.request_repaint_of(ViewportId::ROOT);
    ctx.request_repaint_of(viewport_id());
}

/// Keep the window open while a presentation plays in one (call each frame of the main window).
/// It draws when it needs to; here it is asked to draw when the editor's document changed since.
pub fn keep_open(app: &FigApp, ctx: &Context) {
    let Some(p) = app.ui.present.as_ref().filter(|p| p.ux.window) else { return };
    if ctx.embed_viewports() {
        return;
    }
    let name = app.session.docs.iter().find(|d| d.uid == p.ux.window_doc).map(|d| d.doc.name.clone()).unwrap_or_default();
    let builder = ViewportBuilder::default()
        .with_title(format!("{name} – {}", tr("Prototype")))
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([480.0, 320.0]);
    ctx.show_viewport_deferred(viewport_id(), builder, |ui, _| {
        // (The app lives where the windows module keeps it for the viewports to find.)
        if crate::windows::with_app(|app| draw(app, ui)).is_none() {
            log::warn!("presenter window: the app is busy or not installed");
        }
    });
    let edited = app.session.docs.iter().find(|d| d.uid == p.ux.window_doc).is_some_and(|d| !p.follows(&d.doc));
    if edited {
        ctx.request_repaint_of(viewport_id());
    }
}

/// `ui.set {presentWindow: true|false}`: play the presentation in a window of its own (starting one
/// from the selection or the first screen when none plays) or in the editor's window again.
pub fn set_from_control(app: &mut FigApp, ctx: &Context, p: &serde_json::Value) -> Result<(), String> {
    let Some(on) = p.get("presentWindow").and_then(serde_json::Value::as_bool) else { return Ok(()) };
    if on && app.ui.present.is_none() {
        app.ui.last_error = None;
        app.run("proto.present", serde_json::json!({}));
        if app.ui.present.is_none() {
            return Err(app.ui.last_error.clone().unwrap_or_else(|| "nothing to present".into()));
        }
    }
    // Where there is no window of its own to open the presentation stays in this one (a message says why).
    apply(app, ctx, on);
    Ok(())
}

/// Where a screenshot of the presenter's window is taken: `Err` when the prototype doesn't play in a
/// window of its own, `Ok(None)` while that window hasn't drawn its first frame yet.
pub fn screenshot_viewport(app: &FigApp) -> Result<Option<ViewportId>, String> {
    let p = app.ui.present.as_ref().filter(|p| p.ux.window);
    match p {
        None => Err("the prototype isn't playing in a window of its own (ui.set {presentWindow: true})".into()),
        Some(p) if p.ux.drawn == 0 => Ok(None),
        Some(_) => Ok(Some(viewport_id())),
    }
}

/// Queue input events for the presenter's own window (`ui.input {viewport: "present"}`).
pub fn queue_input(app: &mut FigApp, events: Vec<Event>) -> Result<(), String> {
    /// Events kept for a window that doesn't draw (hidden, or not open yet).
    const KEEP: usize = 4096;
    let p =
        app.ui.present.as_mut().filter(|p| p.ux.window).ok_or("the prototype isn't playing in a window of its own (ui.set {presentWindow: true})")?;
    let room = KEEP.saturating_sub(p.ux.input.len());
    p.ux.input.extend(events.into_iter().take(room));
    Ok(())
}

/// Control channel input events (`ui.input {viewport: "present"}`) for the window about to draw.
pub fn take_input(app: &mut FigApp, viewport: ViewportId) -> Option<Vec<Event>> {
    if viewport != viewport_id() {
        return None;
    }
    app.ui.present.as_mut().filter(|p| p.ux.window).map(|p| std::mem::take(&mut p.ux.input))
}

/// One frame of the presenter window `ui`: the presentation draws with the file it plays active
/// (the editor may have gone to another tab since), and ends with the window.
pub fn draw(app: &mut FigApp, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    // The screenshots asked of this window arrive here.
    crate::control::collect_shots(&ctx);
    let Some(mut pres) = app.ui.present.take() else { return };
    if !pres.ux.window {
        // The presentation went back to the editor's window (or a new one started): this one closes.
        app.ui.present = Some(pres);
        return;
    }
    let (previous, shown) = (app.session.active, app.session.docs.iter().position(|d| d.uid == pres.ux.window_doc));
    pres.ux.drawn = pres.ux.drawn.saturating_add(1);
    let mut keep = false;
    if let Some(i) = shown {
        app.session.active = i;
        // Within the swap, so a panic can't leave another file active.
        crate::windows::guard(app, &ctx, |app| {
            keep = !ctx.input(|i| i.viewport().close_requested()) && super::run(app, ui, &mut pres);
            app.toast_ui(&ctx);
        });
        app.session.active = previous;
    }
    if keep {
        app.ui.present = Some(pres);
    } else {
        // Ended here (Escape, Close, the window's own close button, or its file closed): the window goes.
        ctx.send_viewport_cmd(ViewportCommand::Close);
        ctx.request_repaint_of(ViewportId::ROOT);
    }
}

impl Presenter {
    /// Whether this player has copied `doc` already (it copies again when the editor's document
    /// changes).
    pub fn follows(&self, doc: &std::sync::Arc<figcraft_doc::Document>) -> bool {
        self.source.as_ref().is_some_and(|s| std::sync::Arc::ptr_eq(s, doc))
    }
}
