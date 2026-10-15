//! The prototype in a window of its own ([`window`]): the second window of an egui multi-viewport
//! app, driven headless. The editor's window is the harness' (`Ui::frame`); the presenter's is a
//! pass of the same egui context under its own viewport id that draws [`window::draw`], which is
//! what eframe makes of the deferred viewport's callback. What a real OS window adds (it opening,
//! taking the focus, closing, being captured) needs a desktop: `docs/prototyping-parity.md` lists
//! it.

use std::sync::Arc;
use std::time::Duration;

use egui::{Color32, ColorImage, Event, Key, Modifiers, Pos2, Rect, ViewportCommand, ViewportEvent, ViewportId, ViewportInfo, pos2, vec2};
use serde_json::{Value, json};

use super::ux_tests::{abc, click, control, ex, frame_out, has_text, key_events, presenter, screen, t, text_at, texts};
use super::window;
use crate::present_tests::{Ui, setup};

/// The presenter window's size (the editor's is 1440×900).
const WINDOW: egui::Vec2 = vec2(1100.0, 760.0);

fn child() -> ViewportId {
    window::viewport_id()
}

/// One frame of the presenter's window: a pass of the context under its own viewport id, as eframe
/// runs a deferred viewport's callback. `close` is the window's close button.
fn child_out(u: &mut Ui, events: Vec<Event>, close: bool) -> egui::FullOutput {
    u.time += 1.0 / 60.0;
    let mut viewports = egui::ViewportIdMap::default();
    viewports.insert(child(), ViewportInfo { events: if close { vec![ViewportEvent::Close] } else { Vec::new() }, ..Default::default() });
    let input = egui::RawInput {
        viewport_id: child(),
        viewports,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)),
        time: Some(u.time),
        events,
        ..Default::default()
    };
    let app = &mut u.app;
    let mut out = u.ctx.run_ui(input, |ui| window::draw(app, ui));
    out.textures_delta.clear();
    out
}

fn child_frames(u: &mut Ui, n: usize) {
    for _ in 0..n {
        child_out(u, Vec::new(), false);
    }
}

/// `key` pressed in the presenter's window, the pointer over the prototype.
fn child_press(u: &mut Ui, key: Key, modifiers: Modifiers) -> egui::FullOutput {
    let mut events = vec![Event::PointerMoved(pos2(550.0, 400.0))];
    events.extend(key_events(key, modifiers));
    child_out(u, events, false)
}

/// A click in the presenter's window.
fn child_click(u: &mut Ui, at: Pos2) {
    let b = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    child_out(u, vec![Event::PointerMoved(at)], false);
    child_out(u, vec![b(true)], false);
    child_out(u, vec![b(false)], false);
    child_out(u, Vec::new(), false);
}

/// The commands the pass `out` sent to window `to`.
fn commands(out: &egui::FullOutput, to: ViewportId) -> Vec<ViewportCommand> {
    out.viewport_output.get(&to).map(|v| v.commands.clone()).unwrap_or_default()
}

fn closes(out: &egui::FullOutput, to: ViewportId) -> bool {
    commands(out, to).contains(&ViewportCommand::Close)
}

/// The message on screen (a toast), if any.
fn toast(u: &Ui) -> Option<String> {
    u.app.ui.toast.as_ref().map(|(m, _)| m.clone())
}

/// Screens A, B and C presented in a window of its own, as on a desktop (where viewports aren't
/// embedded), both windows having drawn.
fn windowed() -> (Ui, [figcraft_doc::NodeId; 3]) {
    let (mut u, ids) = abc();
    u.ctx.set_embed_viewports(false);
    window::apply(&mut u.app, &u.ctx, true);
    assert_eq!(presenter(&u)["window"], json!(true), "{:?}", toast(&u));
    // The editor's window draws on and keeps the other one open; that one then draws.
    u.frames(2, Vec::new);
    child_frames(&mut u, 3);
    (u, ids)
}

#[test]
fn where_viewports_are_embedded_the_presentation_stays_in_the_editors_window_and_says_why() {
    let (mut u, _) = abc();
    assert!(u.ctx.embed_viewports(), "a headless context has no window of its own to give");
    let why = t("This build has one window: the prototype presents here");
    // The Options entry.
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    let menu = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&menu, &t("Open in new window")));
    assert_eq!(presenter(&u)["window"], json!(false));
    assert_eq!(toast(&u), Some(why.clone()));
    // The presenter still covers the editor's window, and nothing asks for another window.
    assert!(window::covers_window(&u.app));
    let out = frame_out(&mut u, vec![]);
    assert!(has_text(&out, "1 / 3"), "{:?}", texts(&out));
    assert!(!out.viewport_output.contains_key(&child()));
    // The control channel starts no window either, and takes it no further than a message.
    u.app.ui.toast = None;
    let r = crate::control::handle(&mut u.app, &u.ctx, "ui.set", &json!({"presentWindow": true}));
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(toast(&u), Some(why));
    assert_eq!(presenter(&u)["window"], json!(false));
}

#[test]
fn the_panes_window_button_moves_the_preview_out_where_there_are_windows() {
    let (mut u, [a, ..]) = abc();
    u.app.ui.present = None;
    u.run("proto.preview", json!({"start": a}));
    // (A window shows its content from its second frame.)
    u.frames(2, Vec::new);
    // With one window the button presents full window, like Present, and the pane waits under it.
    let at = control(&u, true, "window");
    click(&mut u, at);
    assert!(u.app.ui.present.is_some() && window::covers_window(&u.app));
    assert!(u.app.ui.preview.is_some(), "the pane is still there for when the presentation ends");
    assert_eq!(toast(&u), Some(t("This build has one window: the prototype presents here")));
    // With windows the preview becomes the window's: the pane goes, the presentation starts on the
    // screen it showed.
    u.app.ui.present = None;
    u.app.ui.toast = None;
    u.ctx.set_embed_viewports(false);
    u.frames(3, Vec::new);
    let at = control(&u, true, "window");
    click(&mut u, at);
    assert_eq!(presenter(&u)["window"], json!(true), "{:?}", toast(&u));
    assert_eq!(presenter(&u)["current"], json!(a));
    assert!(u.app.ui.preview.is_none(), "the preview moved to the window");
    assert!(!window::covers_window(&u.app));
}

#[test]
fn a_window_of_its_own_takes_the_presentation_and_leaves_the_editors_window_to_the_editor() {
    let (mut u, _) = abc();
    u.ctx.set_embed_viewports(false);
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    let menu = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&menu, &t("Open in new window")));
    assert_eq!(presenter(&u)["window"], json!(true), "{:?}", toast(&u));
    assert_eq!(u.app.inspect()["presenting"], json!(true), "it is still the app's presentation");
    assert!(!window::covers_window(&u.app));
    // The editor's window shows the editor again and keeps the presenter's window open: titled
    // with the file, a window's size, drawn by the callback.
    let out = frame_out(&mut u, vec![]);
    let name = u.app.session.doc().map(|d| d.name.clone()).unwrap_or_default();
    let Some(v) = out.viewport_output.get(&child()) else { panic!("the main window asks for the presenter's window") };
    assert!(v.viewport_ui_cb.is_some(), "a deferred viewport");
    assert_eq!(v.builder.title.as_deref(), Some(format!("{name} – {}", t("Prototype")).as_str()));
    assert_eq!(v.builder.inner_size, Some(vec2(1100.0, 760.0)));
    assert!(!has_text(&out, "1 / 3"), "the presenter isn't in the editor's window: {:?}", texts(&out));
    assert!(has_text(&out, &t("Design")), "the editor is: {:?}", texts(&out));
    // The presenter is in its window, at that window's size.
    child_frames(&mut u, 2);
    let win = child_out(&mut u, vec![], false);
    assert!(has_text(&win, "1 / 3") && has_text(&win, "A"), "{:?}", texts(&win));
    assert!(!has_text(&win, &t("Design")), "{:?}", texts(&win));
    let close = control(&u, false, "close");
    assert!(close.x < WINDOW.x && close.y < 60.0, "the bar is the window's: {close:?}");
}

#[test]
fn keys_go_to_the_window_they_happen_in() {
    let (mut u, a, b, card) = setup();
    u.ctx.set_embed_viewports(false);
    u.run("proto.present", json!({"start": a}));
    window::apply(&mut u.app, &u.ctx, true);
    u.frames(2, Vec::new);
    child_frames(&mut u, 3);
    let bounds = |u: &Ui| u.app.session.doc().ok().and_then(|d| d.world_bounds(card));
    let before = bounds(&u);
    // The arrow in the editor's window nudges the selection and leaves the presentation alone.
    u.key(pos2(600.0, 450.0), Key::ArrowRight);
    assert_ne!(bounds(&u), before, "the editor nudged");
    assert_eq!(presenter(&u)["current"], json!(a));
    // The same key in the presenter's window steps the presentation and leaves the selection alone.
    let before = bounds(&u);
    child_press(&mut u, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(presenter(&u)["current"], json!(b));
    assert_eq!(bounds(&u), before, "the presenter's window doesn't nudge");
    // Escape in the editor's window isn't the presentation's.
    u.key(pos2(600.0, 450.0), Key::Escape);
    assert!(u.app.ui.present.is_some());
}

#[test]
fn escape_the_close_button_and_the_windows_own_close_end_the_presentation_and_the_window() {
    let (mut u, _) = windowed();
    let out = child_press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_none());
    assert!(closes(&out, child()), "the window closes: {:?}", commands(&out, child()));
    // Nothing keeps the window open any longer: the editor's window stops asking for it.
    let out = frame_out(&mut u, vec![]);
    assert!(!out.viewport_output.contains_key(&child()));
    // The bar's Close.
    let (mut u, _) = windowed();
    let at = control(&u, false, "close");
    child_click(&mut u, at);
    assert!(u.app.ui.present.is_none());
    // The close button of the window itself (or the system's).
    let (mut u, _) = windowed();
    let out = child_out(&mut u, vec![], true);
    assert!(u.app.ui.present.is_none());
    assert!(closes(&out, child()));
}

#[test]
fn the_options_entry_brings_the_presentation_back_to_the_editors_window() {
    let (mut u, _) = windowed();
    let at = control(&u, false, "options");
    child_click(&mut u, at);
    let menu = child_out(&mut u, vec![], false);
    let entry = t("Open in new window");
    assert!(texts(&menu).iter().any(|(s, _)| s.starts_with('\u{2713}') && s.ends_with(&entry)), "checked while the window is: {:?}", texts(&menu));
    let at = text_at(&menu, &entry);
    child_click(&mut u, at);
    assert_eq!(presenter(&u)["window"], json!(false));
    assert!(u.app.ui.present.is_some(), "the presentation goes on");
    // The editor's window has it again and asks for no other.
    let out = frame_out(&mut u, vec![]);
    assert!(has_text(&out, "1 / 3"), "{:?}", texts(&out));
    assert!(!out.viewport_output.contains_key(&child()));
    // (And it can leave again.)
    window::apply(&mut u.app, &u.ctx, true);
    assert_eq!(presenter(&u)["window"], json!(true));
}

#[test]
fn the_window_follows_the_editors_edits_and_asks_to_draw_only_when_they_happen() {
    let (mut u, [a, ..]) = windowed();
    // Settled: the editor's window doesn't ask the other to draw.
    child_frames(&mut u, 2);
    let out = frame_out(&mut u, vec![]);
    let delay = |out: &egui::FullOutput| out.viewport_output.get(&child()).map(|v| v.repaint_delay);
    assert!(delay(&out).is_some_and(|d| d > Duration::from_secs(3600)), "{:?}", delay(&out));
    // An edit in the editor (the one that stays in its window): the window is asked to draw ...
    ex(&mut u.app.session, "node.set", json!({"id": a, "props": {"name": "Home"}}));
    let out = frame_out(&mut u, vec![]);
    assert_eq!(delay(&out), Some(Duration::ZERO), "asked to draw");
    // ... and shows it, with the new screen as well.
    ex(&mut u.app.session, "frame.create", json!({"x": 1200, "y": 0, "width": 300, "height": 400, "name": "D"}));
    frame_out(&mut u, vec![]);
    let win = child_out(&mut u, vec![], false);
    assert_eq!(screen(&u), "Home");
    assert!(has_text(&win, "Home") && has_text(&win, "1 / 4"), "{:?}", texts(&win));
    // Once it has drawn it is left alone again.
    child_frames(&mut u, 2);
    let out = frame_out(&mut u, vec![]);
    assert!(delay(&out).is_some_and(|d| d > Duration::from_secs(3600)), "{:?}", delay(&out));
}

#[test]
fn the_window_stays_on_its_file_when_the_editor_goes_to_another_tab() {
    let (mut u, _) = windowed();
    // A second file, active in the editor.
    u.app.run("file.new", json!({}));
    assert_eq!(u.app.session.docs.len(), 2);
    assert_eq!(u.app.session.active, 1);
    u.frames(2, Vec::new);
    // The window still plays the first file, and the editor's active file is the editor's again.
    let win = child_out(&mut u, vec![], false);
    assert!(has_text(&win, "1 / 3") && has_text(&win, "A"), "{:?}", texts(&win));
    assert_eq!(u.app.session.active, 1, "the editor's tab is back");
    // Editing the other file doesn't reach the presentation or ask the window to draw.
    child_frames(&mut u, 2);
    ex(&mut u.app.session, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 400, "name": "Other"}));
    let out = frame_out(&mut u, vec![]);
    let asked = out.viewport_output.get(&child()).map(|v| v.repaint_delay);
    assert!(asked.is_some_and(|d| d > Duration::from_secs(3600)), "{asked:?}");
    let win = child_out(&mut u, vec![], false);
    assert!(has_text(&win, "1 / 3"), "{:?}", texts(&win));
    // Closing the file it plays ends the presentation, and the window with it.
    u.app.session.execute("file.close", &json!({"index": 0, "force": true})).ok();
    u.app.session.active = 0;
    let out = child_out(&mut u, vec![], false);
    assert!(u.app.ui.present.is_none());
    assert!(closes(&out, child()), "{:?}", commands(&out, child()));
}

#[test]
fn the_control_channel_opens_drives_and_captures_the_window() {
    let (mut u, [a, b, _]) = abc();
    u.ctx.set_embed_viewports(false);
    u.app.ui.present = None;
    ex(&mut u.app.session, "select.set", json!({"ids": [a]}));
    let handle = |u: &mut Ui, method: &str, p: Value| crate::control::handle(&mut u.app, &u.ctx, method, &p);
    // Input for the window needs the window.
    let events = json!({"viewport": "present", "events": [{"key": "ArrowRight"}]});
    assert!(handle(&mut u, "ui.input", events.clone()).is_err());
    // `ui.set` opens it (starting a presentation when none plays) and answers with the state.
    let state = handle(&mut u, "ui.set", json!({"presentWindow": true})).expect("ui.set");
    assert_eq!(state["presenter"]["window"], json!(true), "{:?}", toast(&u));
    assert_eq!(presenter(&u)["current"], json!(a));
    // A capture asked for before the window has drawn waits for it ...
    let shot = |u: &mut Ui, params: Value| {
        let (tx, rx) = std::sync::mpsc::channel();
        let request = crate::control::ControlRequest { id: json!(1), method: "ui.screenshot".into(), params, reply: tx, cancelled: Default::default() };
        u.app.control_queue.push(request);
        rx
    };
    let path = std::env::temp_dir().join(format!("figcraft-present-window-{}.png", std::process::id())).to_string_lossy().into_owned();
    let rx = shot(&mut u, json!({"viewport": "present", "path": path}));
    let out = frame_out(&mut u, vec![]);
    assert_eq!(u.app.control_queue.len(), 1, "waiting for the window's first frame");
    assert!(!commands(&out, child()).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))));
    // ... then the window is asked for it, and the picture it delivers is saved and answered.
    child_frames(&mut u, 2);
    let out = frame_out(&mut u, vec![]);
    assert!(u.app.control_queue.is_empty());
    assert!(commands(&out, child()).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))), "{:?}", commands(&out, child()));
    assert!(!commands(&out, ViewportId::ROOT).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))), "not the editor's window");
    let image = Arc::new(ColorImage::filled([8, 6], Color32::RED));
    child_out(&mut u, vec![Event::Screenshot { viewport_id: child(), user_data: Default::default(), image }], false);
    let reply = rx.try_recv().expect("answered with the window's picture").result.expect("saved");
    assert_eq!((reply["width"].clone(), reply["height"].clone()), (json!(8), json!(6)));
    assert!(std::path::Path::new(&path).exists());
    std::fs::remove_file(&path).ok();
    // A capture without `viewport` is still the editor's window.
    let rx = shot(&mut u, json!({"path": path}));
    let out = frame_out(&mut u, vec![]);
    assert!(commands(&out, ViewportId::ROOT).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))));
    assert!(!commands(&out, child()).iter().any(|c| matches!(c, ViewportCommand::Screenshot(_))));
    assert!(rx.try_recv().is_err(), "(the picture comes later)");
    // Input events for the window reach it and no other window.
    child_frames(&mut u, 2);
    assert_eq!(handle(&mut u, "ui.input", events).expect("ui.input"), json!({"queued": 1}));
    assert!(crate::windows::take_input(&mut u.app, ViewportId::ROOT).is_empty(), "not the editor's");
    let events = crate::windows::take_input(&mut u.app, child());
    assert_eq!(events.len(), 2, "the key goes down and up");
    child_out(&mut u, events, false);
    assert_eq!(presenter(&u)["current"], json!(b));
    assert!(crate::windows::take_input(&mut u.app, child()).is_empty(), "each event is taken once");
    // Back in the editor's window; a capture of the presenter's window then says there is none.
    let state = handle(&mut u, "ui.set", json!({"presentWindow": false})).expect("ui.set");
    assert_eq!(state["presenter"]["window"], json!(false));
    let rx = shot(&mut u, json!({"viewport": "present", "path": path}));
    frame_out(&mut u, vec![]);
    assert!(rx.try_recv().is_ok_and(|r| r.result.is_err()), "no window to capture");
    assert!(handle(&mut u, "ui.input", json!({"viewport": "present", "events": [{"key": "ArrowRight"}]})).is_err());
}

#[test]
fn a_window_that_shows_comments_owns_the_open_thread() {
    let (mut u, _) = windowed();
    assert!(!window::owns_comments(&u.app), "the comments are off");
    child_press(&mut u, Key::C, Modifiers::SHIFT);
    assert_eq!(presenter(&u)["comments"], json!(true));
    assert!(window::owns_comments(&u.app));
    // Back in the editor's window the presenter's comments are the editor's business again.
    window::apply(&mut u.app, &u.ctx, false);
    assert!(!window::owns_comments(&u.app));
}

#[test]
fn a_screen_reader_or_another_window_keeps_the_presentation_where_it_is() {
    // eframe gives only the main window an accessibility tree: with a screen reader the
    // presentation stays in it.
    let (mut u, _) = abc();
    u.ctx.set_embed_viewports(false);
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    window::apply(&mut u.app, &u.ctx, true);
    assert_eq!(presenter(&u)["window"], json!(false));
    assert_eq!(toast(&u), Some(t("With a screen reader running, the prototype presents in this window")));
    // A window of its own opens from the editor's main window, not from another one.
    let (mut u, _) = abc();
    u.ctx.set_embed_viewports(false);
    u.app.windows.current = 1;
    window::apply(&mut u.app, &u.ctx, true);
    assert_eq!(presenter(&u)["window"], json!(false));
    assert_eq!(toast(&u), Some(t("A window of its own opens from the main window: the prototype presents here")));
}

#[test]
fn a_new_presentation_starts_in_the_editors_window_and_the_old_window_closes() {
    let (mut u, [_, b, _]) = windowed();
    // Present from the editor again while the window plays: the new one covers the editor's window.
    u.run("proto.present", json!({"start": b}));
    assert_eq!(presenter(&u)["window"], json!(false));
    assert!(window::covers_window(&u.app));
    child_out(&mut u, vec![], false);
    assert!(u.app.ui.present.is_some(), "the old window doesn't end the new presentation");
    let out = frame_out(&mut u, vec![]);
    assert!(!out.viewport_output.contains_key(&child()), "so the window isn't asked for any longer");
    assert_eq!(screen(&u), "B");
}
