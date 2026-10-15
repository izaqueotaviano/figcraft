//! The presenter's own controls: the shortcut table that its keys, the Options menu's hints and the
//! "?" list all read, and stepping through the page's screens. Driven headless through egui (the
//! harness is `present_tests`'); the window is 1440×900 and the presenter's bar takes 40.

use egui::{Event, Key, Modifiers, Pos2, Rect, Shape, pos2, vec2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::ux::{self, Act, SHORTCUTS, With};
use crate::present_tests::{Ui, id};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn frame(s: &mut Session, name: &str, x: f64) -> NodeId {
    id(&ex(s, "frame.create", json!({"x": x, "y": 0, "width": 300, "height": 400, "name": name})))
}

/// Screens A, B and C side by side (no device); A presented.
fn abc() -> (Ui, [NodeId; 3]) {
    let mut s = Session::new();
    let ids = [("A", 0.0), ("B", 400.0), ("C", 800.0)].map(|(n, x)| frame(&mut s, n, x));
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": ids[0]}));
    (u, ids)
}

fn presenter(u: &Ui) -> Value {
    u.app.inspect()["presenter"].clone()
}

fn screen(u: &Ui) -> String {
    presenter(u)["name"].as_str().unwrap_or_default().to_string()
}

/// One frame with `events`; the output (shapes included; textures dropped).
pub(crate) fn frame_out(u: &mut Ui, events: Vec<Event>) -> egui::FullOutput {
    u.time += 1.0 / 60.0;
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, u.size)), time: Some(u.time), events, ..Default::default() };
    let app = &mut u.app;
    let mut out = u.ctx.run_ui(input, |ui| app.ui(ui));
    out.textures_delta.clear();
    out
}

/// The shapes of `out`, nested ones flattened.
pub(crate) fn shapes(out: &egui::FullOutput) -> Vec<Shape> {
    fn walk(s: &Shape, into: &mut Vec<Shape>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, into)),
            s => into.push(s.clone()),
        }
    }
    let mut v = Vec::new();
    out.shapes.iter().for_each(|c| walk(&c.shape, &mut v));
    v
}

/// The text shapes of `out`: text and the middle of the text.
pub(crate) fn texts(out: &egui::FullOutput) -> Vec<(String, Pos2)> {
    shapes(out)
        .iter()
        .filter_map(|s| match s {
            Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.size() / 2.0)),
            _ => None,
        })
        .collect()
}

/// The middle of the control `name` the presenter (or the pane: `pane`) drew last.
pub(crate) fn control(u: &Ui, pane: bool, name: &str) -> Pos2 {
    let p = if pane { u.preview() } else { presenter(u) };
    let r = &p["controls"][name];
    let f = |i: usize| r[i].as_f64().unwrap_or_else(|| panic!("no control {name:?}: {}", p["controls"])) as f32;
    pos2((f(0) + f(2)) / 2.0, (f(1) + f(3)) / 2.0)
}

/// `s` translated (the tests read the screen in whatever language the UI is set to).
pub(crate) fn t(s: &str) -> String {
    crate::i18n::tr(s).to_string()
}

/// A text as menu rows write it: a check mark or its room before the label doesn't count.
fn bare(t: &str) -> &str {
    t.trim_start_matches('\u{2713}').trim()
}

pub(crate) fn has_text(out: &egui::FullOutput, s: &str) -> bool {
    texts(out).iter().any(|(t, _)| bare(t) == s)
}

/// The middle of the text `s` on screen (menu rows have a check mark's room before them).
pub(crate) fn text_at(out: &egui::FullOutput, s: &str) -> Pos2 {
    texts(out).into_iter().find(|(t, _)| bare(t) == s).unwrap_or_else(|| panic!("no text {s:?} on screen")).1
}

pub(crate) fn key_events(key: Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false].map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }).to_vec()
}

/// Press `key` with the pointer over the player.
pub(crate) fn press(u: &mut Ui, key: Key, modifiers: Modifiers) -> egui::FullOutput {
    let mut events = vec![Event::PointerMoved(pos2(720.0, 470.0))];
    events.extend(key_events(key, modifiers));
    frame_out(u, events)
}

/// Click at `at`.
pub(crate) fn click(u: &mut Ui, at: Pos2) {
    let b = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame_out(u, vec![Event::PointerMoved(at)]);
    frame_out(u, vec![b(true)]);
    frame_out(u, vec![b(false)]);
    frame_out(u, vec![]);
}

/// Click the full-window presenter's control `name` ([`control`]).
pub(crate) fn click_control(u: &mut Ui, name: &str) {
    let at = control(u, false, name);
    click(u, at);
}

/// The modifiers a chord is pressed with.
fn held(with: With) -> Modifiers {
    match with {
        With::None | With::Any => Modifiers::NONE,
        With::Shift => Modifiers::SHIFT,
        With::Cmd => {
            if crate::shortcuts::platform() == figcraft_engine::keymap::Platform::Mac {
                Modifiers::MAC_CMD | Modifiers::COMMAND
            } else {
                Modifiers::CTRL | Modifiers::COMMAND
            }
        }
    }
}

#[test]
fn the_table_holds_every_act_once_and_its_specs_say_what_its_chords_are() {
    // Every act has exactly one row, in the order the list shows them.
    assert_eq!(SHORTCUTS.iter().map(|s| s.act).collect::<Vec<_>>(), Act::ALL, "one row per act, in the list's order");
    // A chord's spec (what the list and the menus print) is its key and modifiers.
    for s in SHORTCUTS {
        assert!(!s.chords.is_empty(), "{:?} has no key", s.act);
        for c in s.chords {
            let mut parts: Vec<&str> = c.spec.split('+').collect();
            let name = parts.pop().unwrap_or_default();
            let name = if c.spec.ends_with("++") { "+" } else { name };
            assert_eq!(Key::from_name(name), Some(c.key), "{:?}: spec {:?} names another key", s.act, c.spec);
            let want = parts.iter().fold(With::None, |w, m| match (w, *m) {
                (With::None, "Shift") => With::Shift,
                (With::None, "Cmd") => With::Cmd,
                (w, m) => panic!("{:?}: spec {:?} has a modifier {m:?} after {w:?}", s.act, c.spec),
            });
            let ok = want == c.with || (c.with == With::Any && want == With::None);
            assert!(ok, "{:?}: spec {:?} says {want:?}, the chord needs {:?}", s.act, c.spec, c.with);
        }
    }
    // No key press does two things.
    let mods = [Modifiers::NONE, Modifiers::SHIFT, held(With::Cmd), held(With::Cmd) | Modifiers::SHIFT, Modifiers::ALT];
    for s in SHORTCUTS {
        for c in s.chords {
            for m in mods {
                let rows: Vec<Act> =
                    SHORTCUTS.iter().filter(|r| r.chords.iter().any(|x| x.key == c.key && x.with.matches(m))).map(|r| r.act).collect();
                assert!(rows.len() <= 1, "{:?} with {m:?} does {rows:?}", c.key);
            }
        }
    }
    // The pane has only what makes sense in a small window over the editor.
    let pane: Vec<Act> = SHORTCUTS.iter().filter(|s| s.pane).map(|s| s.act).collect();
    assert_eq!(pane, [Act::Next, Act::Prev, Act::Restart]);
    assert_eq!(ux::act_for(Key::F, Modifiers::NONE, true), None, "F isn't the pane's");
    assert_eq!(ux::act_for(Key::F, Modifiers::NONE, false), Some(Act::Fullscreen));
    // Shift+/ is the question mark of a US keyboard; other layouts type the question mark itself.
    assert_eq!(ux::act_for(Key::Slash, Modifiers::SHIFT, false), Some(Act::Help));
    assert_eq!(ux::act_for(Key::Questionmark, Modifiers::SHIFT, false), Some(Act::Help));
    assert_eq!(ux::act_for(Key::Slash, Modifiers::NONE, false), None);
    // The arrows work with any modifier; Space and N only alone.
    assert_eq!(ux::act_for(Key::ArrowRight, Modifiers::SHIFT, true), Some(Act::Next));
    assert_eq!(ux::act_for(Key::N, Modifiers::SHIFT, true), None);
    assert_eq!(ux::act_for(Key::C, Modifiers::SHIFT, false), Some(Act::Comments));
    assert_eq!(ux::act_for(Key::C, held(With::Cmd), false), None, "Cmd+C is the editor's");
}

/// Press every chord of every row and see its act happen: the list can't promise a key the
/// handlers don't answer. (The `match` has no catch-all: a new act needs a case here.)
#[test]
fn every_listed_key_does_what_the_list_says() {
    for row in SHORTCUTS {
        for c in row.chords {
            // A fresh presentation of A with B and C after it, and a comment to show.
            let (mut u, [a, b, _]) = abc();
            let m = held(c.with);
            let key = |u: &mut Ui| press(u, c.key, m);
            let what = format!("{:?} ({})", row.act, c.spec);
            match row.act {
                Act::Next => {
                    key(&mut u);
                    assert_eq!(screen(&u), "B", "{what}");
                }
                Act::Prev => {
                    u.run("proto.present", json!({"start": b}));
                    key(&mut u);
                    assert_eq!(presenter(&u)["current"], json!(a), "{what}");
                }
                Act::Restart => {
                    press(&mut u, Key::N, Modifiers::NONE);
                    press(&mut u, Key::N, Modifiers::NONE);
                    assert_eq!(screen(&u), "C");
                    key(&mut u);
                    assert_eq!(screen(&u), "A", "{what}");
                    assert_eq!(presenter(&u)["history"], json!([]), "{what}: the history is gone");
                }
                Act::NextSize => {
                    assert_eq!(presenter(&u)["scale"], "actual");
                    key(&mut u);
                    assert_eq!(presenter(&u)["scale"], "fit", "{what}");
                }
                Act::Fullscreen => {
                    let out = key(&mut u);
                    let cmds = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default();
                    assert!(cmds.contains(&egui::ViewportCommand::Fullscreen(true)), "{what}: {cmds:?}");
                }
                Act::Background => {
                    assert_eq!(presenter(&u)["background"], json!("page"));
                    key(&mut u);
                    assert_eq!(presenter(&u)["background"], json!("#1E1E1E"), "{what}: the first preset");
                    for _ in 0..5 {
                        key(&mut u);
                    }
                    assert_eq!(presenter(&u)["background"], json!("page"), "{what}: round again after the five presets");
                }
                Act::Comments => {
                    assert_eq!(presenter(&u)["comments"], json!(false));
                    key(&mut u);
                    assert_eq!(presenter(&u)["comments"], json!(true), "{what}");
                    key(&mut u);
                    assert_eq!(presenter(&u)["comments"], json!(false), "{what}: again hides them");
                }
                Act::Help => {
                    key(&mut u);
                    assert_eq!(presenter(&u)["shortcuts"], json!(true), "{what}");
                    key(&mut u);
                    assert_eq!(presenter(&u)["shortcuts"], json!(false), "{what}: again closes the list");
                }
                Act::HideUi => {
                    assert_eq!(presenter(&u)["hideUi"], json!(false));
                    key(&mut u);
                    assert_eq!(presenter(&u)["hideUi"], json!(true), "{what}");
                    assert!(presenter(&u)["controls"]["close"].is_null(), "{what}: the bar is gone");
                    key(&mut u);
                    assert_eq!(presenter(&u)["hideUi"], json!(false), "{what}: again brings it back");
                    assert!(!presenter(&u)["controls"]["close"].is_null(), "{what}: with its buttons");
                }
                Act::Close => {
                    key(&mut u);
                    assert!(u.app.ui.present.is_none(), "{what}");
                }
            }
        }
    }
}

/// The list shows each row's label and keys, built from the table.
#[test]
fn the_question_mark_opens_a_list_of_every_shortcut() {
    let (mut u, _) = abc();
    let closed = frame_out(&mut u, vec![]);
    assert!(!has_text(&closed, &t("Next screen")), "the list is closed");
    // The bar's "?" opens it.
    click(&mut u, text_at(&closed, "?"));
    assert_eq!(presenter(&u)["shortcuts"], json!(true), "the ? button");
    let out = frame_out(&mut u, vec![]);
    for row in SHORTCUTS {
        assert!(has_text(&out, &t(row.label)), "the list has {:?}", row.label);
        for c in row.chords {
            assert!(has_text(&out, &crate::toolbar::pretty_shortcut(c.spec)), "the list shows {:?} for {:?}", c.spec, row.label);
        }
    }
    // The prototype keeps working under it, and the key closes it (so does Escape, once).
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!(screen(&u), "B");
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert_eq!(presenter(&u)["shortcuts"], json!(false), "Escape closes the list");
    assert!(u.app.ui.present.is_some(), "and not the presentation");
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_none(), "the next one does");
}

#[test]
fn the_options_menu_offers_the_list_and_prints_the_tables_keys() {
    let (mut u, _) = abc();
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    let menu = frame_out(&mut u, vec![]);
    // The hints come from the table.
    assert!(has_text(&menu, &crate::toolbar::pretty_shortcut(ux::spec(Act::Restart))), "{:?}", texts(&menu));
    click(&mut u, text_at(&menu, &t("Keyboard shortcuts")));
    assert_eq!(presenter(&u)["shortcuts"], json!(true));
    // A menu that is open takes Escape: the presentation stays.
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_some(), "Escape closed the menu only");
    assert_eq!(presenter(&u)["shortcuts"], json!(true), "and left the list");
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert_eq!(presenter(&u)["shortcuts"], json!(false));
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_none());
}

#[test]
fn the_prototypes_own_keys_win_over_the_presenters_except_shift_c() {
    let mut s = Session::new();
    let [a, b, c] = [("A", 0.0), ("B", 400.0), ("C", 800.0)].map(|(n, x)| frame(&mut s, n, x));
    // On A: N goes to C (not the next screen, B); Z and Shift+C are also the prototype's.
    let hot = id(&ex(&mut s, "shape.rectangle", json!({"parent": a, "x": 10, "y": 10, "width": 50, "height": 50})));
    for k in ["N", "Z", "Shift+C"] {
        ex(
            &mut s,
            "proto.interaction.add",
            json!({"id": hot, "trigger": {"type": "keyPress", "key": k}, "action": {"type": "navigate", "dest": c}, "transition": {"type": "instant"}}),
        );
    }
    let _ = b;
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": a}));
    press(&mut u, Key::Z, Modifiers::NONE);
    assert_eq!(
        (screen(&u), presenter(&u)["scale"].clone()),
        ("C".to_string(), json!("actual")),
        "Z is the prototype's: it navigated, the size stayed"
    );
    u.run("proto.present", json!({"start": a}));
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!(screen(&u), "C", "N is the prototype's");
    u.run("proto.present", json!({"start": a}));
    press(&mut u, Key::C, Modifiers::SHIFT);
    assert_eq!(presenter(&u)["comments"], json!(true), "Shift+C always shows the comments");
    assert_eq!(screen(&u), "A", "and the prototype doesn't see it");
}

#[test]
fn the_pane_takes_only_the_keys_that_fit_it() {
    let (mut u, [a, ..]) = abc();
    u.app.ui.present = None;
    u.run("proto.preview", json!({"start": a}));
    // (A window shows its content from its second frame.)
    u.frames(2, Vec::new);
    let c = u.pane().center();
    let step = |u: &mut Ui, key| {
        let mut events = vec![Event::PointerMoved(c)];
        events.extend(key_events(key, Modifiers::NONE));
        frame_out(u, events);
    };
    step(&mut u, Key::N);
    assert_eq!(u.preview()["name"], "B", "N steps");
    step(&mut u, Key::R);
    assert_eq!(u.preview()["name"], "A", "R restarts");
    step(&mut u, Key::Questionmark);
    step(&mut u, Key::C);
    step(&mut u, Key::Z);
    let p = u.preview();
    assert_eq!((&p["shortcuts"], &p["comments"], &p["scale"]), (&json!(false), &json!(false), &json!("actual")), "?, C and Z are the full window's");
}

/// Screens A, B, C and a frame "Menu" side by side, A presented. On A: "Next" (a layer at the
/// top) goes to C on a click, "Open" (below it) opens the menu as an overlay.
fn linked() -> (Ui, [NodeId; 3], NodeId) {
    let mut s = Session::new();
    let [a, b, c] = [("A", 0.0), ("B", 400.0), ("C", 800.0)].map(|(n, x)| frame(&mut s, n, x));
    let menu = frame(&mut s, "Menu", 1200.0);
    let layer = |s: &mut Session, name: &str, y: f64| {
        id(&ex(s, "shape.rectangle", json!({"parent": a, "x": 10, "y": y, "width": 100, "height": 40, "props": {"name": name}})))
    };
    let (next, open) = (layer(&mut s, "Next", 10.0), layer(&mut s, "Open", 100.0));
    let on_click =
        |layer: NodeId, action: Value| json!({"id": layer, "trigger": {"type": "onClick"}, "action": action, "transition": {"type": "instant"}});
    ex(&mut s, "proto.interaction.add", on_click(next, json!({"type": "navigate", "dest": c})));
    ex(&mut s, "proto.interaction.add", on_click(open, json!({"type": "openOverlay", "dest": menu})));
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": a}));
    (u, [a, b, c], menu)
}

/// The middle of each hotspot, top to bottom.
fn hotspots(u: &Ui) -> Vec<Pos2> {
    let mut v: Vec<Pos2> = presenter(u)["hotspots"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|h| {
            let f = |i: usize| h["rect"][i].as_f64().unwrap_or(0.0) as f32;
            pos2((f(0) + f(2)) / 2.0, (f(1) + f(3)) / 2.0)
        })
        .collect();
    v.sort_by(|p, q| p.y.total_cmp(&q.y));
    v
}

fn click_hotspot(u: &mut Ui, k: usize) {
    let at = hotspots(u)[k];
    click(u, at);
}

fn position(u: &Ui) -> Value {
    presenter(u)["position"].clone()
}

fn current(u: &Ui) -> Value {
    presenter(u)["current"].clone()
}

#[test]
fn the_bar_says_which_screen_of_the_page_this_is_and_steps_with_buttons() {
    let (mut u, [a, b, c], _) = linked();
    // The page has A, B, C and the menu frame: four top-level frames in reading order.
    assert_eq!(position(&u), json!([1, 4]));
    assert!(has_text(&frame_out(&mut u, vec![]), "1 / 4"));
    // Keys: N, Space and the right arrow step on; the left arrow back.
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!((position(&u), current(&u)), (json!([2, 4]), json!(b)));
    press(&mut u, Key::Space, Modifiers::NONE);
    assert_eq!(position(&u), json!([3, 4]));
    press(&mut u, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(position(&u), json!([2, 4]));
    // The buttons beside the counter do the same and the text follows.
    click_control(&mut u, "next");
    assert_eq!((position(&u), current(&u)), (json!([3, 4]), json!(c)));
    assert!(has_text(&frame_out(&mut u, vec![]), "3 / 4"));
    click_control(&mut u, "prev");
    click_control(&mut u, "prev");
    assert_eq!((position(&u), current(&u)), (json!([1, 4]), json!(a)));
    // At the first screen "previous" does nothing; at the last, "next".
    click_control(&mut u, "prev");
    assert_eq!(position(&u), json!([1, 4]));
    for _ in 0..5 {
        click_control(&mut u, "next");
    }
    assert_eq!(position(&u), json!([4, 4]));
    // Restarting and a click on a link update it too.
    click_control(&mut u, "restart");
    assert_eq!(position(&u), json!([1, 4]));
    click_hotspot(&mut u, 0);
    assert_eq!(current(&u), json!(c), "the link led to C");
    assert_eq!(position(&u), json!([3, 4]), "a click that navigates updates the counter");
}

#[test]
fn an_overlay_keeps_the_counter_and_the_stepping_waits_for_it_to_close() {
    let (mut u, [a, ..], menu) = linked();
    click_hotspot(&mut u, 1);
    assert_eq!(presenter(&u)["overlays"], json!([menu]));
    // The screen under the overlay is still screen 1.
    assert_eq!(position(&u), json!([1, 4]));
    assert!(has_text(&frame_out(&mut u, vec![]), "1 / 4"));
    // Neither the buttons nor the keys step while it is open.
    click_control(&mut u, "next");
    press(&mut u, Key::N, Modifiers::NONE);
    press(&mut u, Key::ArrowRight, Modifiers::NONE);
    assert_eq!((position(&u), current(&u)), (json!([1, 4]), json!(a)));
    // Closed (a click outside it), they step again.
    click(&mut u, pos2(20.0, 450.0));
    assert_eq!(presenter(&u)["overlays"], json!([]));
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!(position(&u), json!([2, 4]));
}

#[test]
fn a_page_with_one_screen_has_no_counter_and_flows_are_not_what_it_counts() {
    // One frame: nothing to step through, no counter and no buttons.
    let mut s = Session::new();
    let only = frame(&mut s, "Only", 0.0);
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": only}));
    assert_eq!(position(&u), Value::Null);
    let out = frame_out(&mut u, vec![]);
    assert!(!texts(&out).iter().any(|(t, _)| t.contains(" / ")), "{:?}", texts(&out));
    assert!(presenter(&u)["controls"]["next"].is_null() && presenter(&u)["controls"]["prev"].is_null());
    // Two flows on a page of five screens: the counter is the page's, whichever flow plays.
    let mut s = Session::new();
    let ids: Vec<NodeId> = (0..5).map(|i| frame(&mut s, &format!("S{i}"), 400.0 * f64::from(i))).collect();
    ex(&mut s, "proto.flow.add", json!({"start": ids[0], "name": "One"}));
    ex(&mut s, "proto.flow.add", json!({"start": ids[3], "name": "Two"}));
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": ids[3]}));
    assert_eq!(position(&u), json!([4, 5]), "the second flow starts at the page's fourth screen");
    press(&mut u, Key::N, Modifiers::NONE);
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!(position(&u), json!([5, 5]), "and the keys go on past the flow's own screens");
    // Frames inside a section count too, in reading order with the page's other frames.
    let mut s = Session::new();
    let a = frame(&mut s, "A", 0.0);
    let sec = id(&ex(&mut s, "node.create", json!({"type": "section", "x": 380, "y": -20, "width": 700, "height": 500})));
    let b = frame(&mut s, "B", 400.0);
    ex(&mut s, "node.reparent", json!({"ids": [b], "parent": sec}));
    let c = frame(&mut s, "C", 800.0);
    ex(&mut s, "node.reparent", json!({"ids": [c], "parent": sec}));
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": a}));
    press(&mut u, Key::N, Modifiers::NONE);
    assert_eq!((position(&u), current(&u)), (json!([2, 3]), json!(b)));
}

#[test]
fn a_long_name_gives_way_to_the_counter_and_the_pane_has_one_too() {
    let mut s = Session::new();
    let long = "A screen with a really rather long name that goes on and on and on and on";
    let a = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 400, "name": long})));
    frame(&mut s, "B", 400.0);
    let mut u = Ui::new(s);
    u.size = vec2(700.0, 500.0);
    u.run("proto.present", json!({"start": a}));
    let out = frame_out(&mut u, vec![]);
    // The counter is still on the bar, left of the buttons at the right, and its text isn't cut.
    assert!(has_text(&out, "1 / 2"));
    let (p, help) = (control(&u, false, "position"), control(&u, false, "help"));
    assert!(p.x < help.x - 20.0, "{p:?} {help:?}");
    // The preview pane shows it too: with its buttons when they fit (here the name is cut to make
    // room), as text alone when they don't (a flow picker takes the room), not at all when even
    // that is too much.
    u.app.ui.present = None;
    u.run("proto.preview", json!({"start": a}));
    u.frames(2, Vec::new);
    assert_eq!(u.preview()["position"], json!([1, 2]));
    let pane = u.preview();
    assert!(!pane["controls"]["position"].is_null() && !pane["controls"]["next"].is_null(), "{}", pane["controls"]);
    u.run("proto.flow.add", json!({"start": a, "name": "Flow one"}));
    u.frames(3, Vec::new);
    let pane = u.preview();
    assert!(!pane["controls"]["position"].is_null() && pane["controls"]["next"].is_null(), "{}", pane["controls"]);
}

fn screen_rect(u: &Ui) -> [f64; 4] {
    let r = presenter(u)["screenRect"].clone();
    [0, 1, 2, 3].map(|i| r[i].as_f64().unwrap_or(f64::NAN))
}

fn cmd_backslash(u: &mut Ui) -> egui::FullOutput {
    press(u, Key::Backslash, held(With::Cmd))
}

fn move_to(u: &mut Ui, at: Pos2) {
    frame_out(u, vec![Event::PointerMoved(at)]);
}

/// Frames until the bar has slid all the way (or away).
fn settle_bar(u: &mut Ui) {
    u.frames(20, Vec::new);
}

fn bar_texts_shown(out: &egui::FullOutput) -> bool {
    ["Restart", "Close", "Options"].iter().any(|l| has_text(out, &t(l)))
}

#[test]
fn hiding_the_ui_leaves_only_the_prototype_centred_in_the_whole_window() {
    let (mut u, [a, ..]) = abc();
    u.run("proto.presentOptions", json!({"inspect": true}));
    u.frames(2, Vec::new);
    assert!(bar_texts_shown(&frame_out(&mut u, vec![])), "the bar shows");
    // The Inspect panel (280 wide) and the 40 high bar take room: the prototype is centred in the rest.
    assert_eq!(screen_rect(&u), [430.0, 270.0, 730.0, 670.0]);
    cmd_backslash(&mut u);
    let out = frame_out(&mut u, vec![]);
    assert_eq!(presenter(&u)["hideUi"], json!(true));
    assert!(!bar_texts_shown(&out), "{:?}", texts(&out));
    assert!(presenter(&u)["controls"].as_object().is_some_and(|c| c.is_empty()), "no controls: {}", presenter(&u)["controls"]);
    assert_eq!(screen_rect(&u), [570.0, 250.0, 870.0, 650.0], "centred in the whole window, panel and bar gone");
    // It follows the window: a different size, still centred and fitted.
    u.size = vec2(1000.0, 700.0);
    u.frames(2, Vec::new);
    assert_eq!(screen_rect(&u), [350.0, 150.0, 650.0, 550.0]);
    u.run("proto.presentOptions", json!({"scale": "fit"}));
    u.size = vec2(1440.0, 900.0);
    u.frames(2, Vec::new);
    let r = screen_rect(&u);
    assert_eq!((r[0] + r[2]) / 2.0, 720.0);
    assert_eq!((r[1] + r[3]) / 2.0, 450.0);
    // A viewing option, kept for the next presentation (which starts hidden).
    assert!(u.app.ui.present_view.hide_ui);
    assert_eq!(u.app.inspect()["presentOptions"]["hideUi"], json!(true));
    u.run("proto.present", json!({"start": a}));
    assert_eq!(presenter(&u)["hideUi"], json!(true));
    // Cmd+\ again brings it back, with the panel.
    cmd_backslash(&mut u);
    assert!(bar_texts_shown(&frame_out(&mut u, vec![])));
    assert!(!u.app.ui.present_view.hide_ui);
}

#[test]
fn the_options_menu_hides_the_ui_and_the_hint_is_the_tables() {
    let (mut u, _) = abc();
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    let menu = frame_out(&mut u, vec![]);
    assert!(has_text(&menu, &crate::toolbar::pretty_shortcut(ux::spec(Act::HideUi))), "{:?}", texts(&menu));
    click(&mut u, text_at(&menu, &t("Hide UI")));
    assert_eq!(presenter(&u)["hideUi"], json!(true));
    assert!(!bar_texts_shown(&frame_out(&mut u, vec![])));
}

#[test]
fn the_bar_slides_in_at_the_top_edge_and_out_again() {
    let (mut u, _) = abc();
    cmd_backslash(&mut u);
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(0.0));
    // A few points from the top edge: it slides in (not at once), to its full height.
    move_to(&mut u, pos2(700.0, 2.0));
    let first = presenter(&u)["barReveal"].as_f64().unwrap_or(0.0);
    assert!(first > 0.0 && first < 1.0, "sliding: {first}");
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(1.0));
    let out = frame_out(&mut u, vec![]);
    assert!(bar_texts_shown(&out));
    let close = &presenter(&u)["controls"]["close"];
    assert_eq!((close[1].as_f64(), close[3].as_f64()), (Some(8.0), Some(32.0)), "the bar is at the top: {close}");
    // It stays while the pointer is on it, goes when it leaves.
    move_to(&mut u, pos2(700.0, 30.0));
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(1.0));
    move_to(&mut u, pos2(700.0, 300.0));
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(0.0));
    assert!(!bar_texts_shown(&frame_out(&mut u, vec![])));
    // The pointer a little below the edge doesn't ask for it.
    move_to(&mut u, pos2(700.0, 12.0));
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(0.0));
}

#[test]
fn the_bar_that_slid_in_works_and_the_prototype_under_it_does_not_see_the_click() {
    // A 1440×900 screen with a link in its top right corner, right under the Restart button.
    let mut s = Session::new();
    let a = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 1440, "height": 900, "name": "A"})));
    let c = frame(&mut s, "C", 2000.0);
    let link = id(&ex(&mut s, "shape.rectangle", json!({"parent": a, "x": 1300, "y": 0, "width": 140, "height": 40, "props": {"name": "Link"}})));
    ex(
        &mut s,
        "proto.interaction.add",
        json!({"id": link, "trigger": {"type": "onClick"}, "action": {"type": "navigate", "dest": c}, "transition": {"type": "instant"}}),
    );
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": a}));
    cmd_backslash(&mut u);
    settle_bar(&mut u);
    move_to(&mut u, pos2(700.0, 1.0));
    settle_bar(&mut u);
    // Restart is where the link is. The click is the bar's.
    click_control(&mut u, "restart");
    assert_eq!(presenter(&u)["current"], json!(a), "the link under the button didn't fire");
    // The Options menu opens from it and keeps the bar while the pointer is in the menu.
    click_control(&mut u, "options");
    u.frames(2, Vec::new);
    move_to(&mut u, pos2(1300.0, 120.0));
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(1.0), "the open menu keeps the bar");
    // Escape closes the menu only; the next one shows the UI again; the third ends the presentation.
    press(&mut u, Key::Escape, Modifiers::NONE);
    settle_bar(&mut u);
    assert!(u.app.ui.present.is_some() && presenter(&u)["hideUi"] == json!(true));
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_some(), "Escape once shows the UI, it doesn't close");
    assert_eq!(presenter(&u)["hideUi"], json!(false));
    assert!(bar_texts_shown(&frame_out(&mut u, vec![])));
    press(&mut u, Key::Escape, Modifiers::NONE);
    assert!(u.app.ui.present.is_none());
}

#[test]
fn a_drag_from_the_prototype_does_not_pull_the_bar_in_and_a_hidden_bar_costs_no_frames() {
    let (mut u, _) = abc();
    cmd_backslash(&mut u);
    settle_bar(&mut u);
    // Press in the prototype, drag past the top edge: the bar stays away until the button is up.
    let b = |at: Pos2, pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    frame_out(&mut u, vec![Event::PointerMoved(pos2(720.0, 400.0)), b(pos2(720.0, 400.0), true)]);
    for y in [300.0, 100.0, 20.0, 2.0] {
        frame_out(&mut u, vec![Event::PointerMoved(pos2(720.0, y))]);
    }
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(0.0), "dragging past the edge");
    frame_out(&mut u, vec![b(pos2(720.0, 2.0), false)]);
    settle_bar(&mut u);
    assert_eq!(presenter(&u)["barReveal"].as_f64(), Some(1.0), "released at the edge: now it asks for the bar");
    // Hidden and away, nothing asks for another frame.
    move_to(&mut u, pos2(720.0, 400.0));
    settle_bar(&mut u);
    assert!(!u.repaints(), "an idle hidden presenter draws no frames: {:?}", u.repaint);
}

#[test]
fn hide_ui_is_reachable_from_the_control_channel() {
    let (mut u, _) = abc();
    let set = |u: &mut Ui, v: Value| crate::control::handle(&mut u.app, &u.ctx, "ui.set", &json!({"presentHideUi": v}));
    set(&mut u, json!(true)).unwrap();
    u.frames(2, Vec::new);
    assert_eq!(presenter(&u)["hideUi"], json!(true));
    assert_eq!(u.app.inspect()["presentOptions"]["hideUi"], json!(true));
    assert!(!bar_texts_shown(&frame_out(&mut u, vec![])));
    assert!(set(&mut u, json!("yes")).is_err(), "only true or false");
    set(&mut u, json!(false)).unwrap();
    u.frames(2, Vec::new);
    assert!(bar_texts_shown(&frame_out(&mut u, vec![])));
    // Between presentations it sets what the next one starts with.
    u.app.ui.present = None;
    set(&mut u, json!(true)).unwrap();
    assert_eq!(u.app.inspect()["presentOptions"]["hideUi"], json!(true));
    let a = u.app.session.doc().unwrap().top_frames(u.app.session.page().unwrap())[0];
    u.run("proto.present", json!({"start": a}));
    assert_eq!(presenter(&u)["hideUi"], json!(true));
}

use crate::present::raster_tests::Rec;
use crate::present::ux::Background;

fn px(img: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let p = img.get_pixel(x, y).0;
    [p[0], p[1], p[2]]
}

#[test]
fn backgrounds_parse_print_and_cycle() {
    for (text, want) in [
        ("page", Some(Background::Page)),
        (" Page ", Some(Background::Page)),
        ("WHITE", Some(Background::Color([255, 255, 255]))),
        ("grey", Some(Background::Color([128, 128, 128]))),
        ("#336699", Some(Background::Color([0x33, 0x66, 0x99]))),
        ("369", Some(Background::Color([0x33, 0x66, 0x99]))),
        ("#33669980", Some(Background::Color([0x33, 0x66, 0x99]))),
        ("", None),
        ("blue", None),
        ("#12345", None),
        ("#gg0000", None),
    ] {
        assert_eq!(Background::parse(text), want, "{text:?}");
    }
    for bg in [Background::Page, Background::Color([0, 0, 0]), Background::Color([0x33, 0x66, 0x99])] {
        assert_eq!(Background::parse(&bg.id()), Some(bg), "{}", bg.id());
    }
    // The B key goes through the page's colour and the presets, in the menu's order, and round.
    let mut bg = Background::Page;
    let mut seen = vec![bg.id()];
    for _ in 0..6 {
        bg = bg.next();
        seen.push(bg.id());
    }
    assert_eq!(seen, ["page", "#1E1E1E", "#808080", "#E6E6E6", "#FFFFFF", "#000000", "page"]);
    assert_eq!(Background::Color([1, 2, 3]).next(), Background::Page, "a colour of the viewer's own goes back to the page's");
    // Light colours get dark ink and a shadow under a device; mid greys and darks don't.
    let light = |c: [u8; 3]| ux::is_light(egui::Color32::from_rgb(c[0], c[1], c[2]));
    assert!(light([255, 255, 255]) && light([0xE6, 0xE6, 0xE6]) && light([0xF5, 0xF5, 0xF5]));
    assert!(!light([0x80, 0x80, 0x80]) && !light([0x1E, 0x1E, 0x1E]) && !light([0, 0, 0]) && !light([0x0D, 0x99, 0xFF]));
}

#[test]
fn the_colour_around_the_prototype_is_the_viewers_to_pick_and_the_files_stays() {
    let mut s = Session::new();
    let a = frame(&mut s, "A", 0.0);
    ex(&mut s, "node.set", json!({"id": a, "props": {"fills": ["#FFFFFF"]}}));
    frame(&mut s, "B", 400.0);
    let mut r = Rec::new(s);
    r.run("proto.present", json!({"start": a}));
    let (page_before, dirty_before) = {
        let ds = r.u.app.session.ds().unwrap();
        (ds.doc.node(ds.page).map(|n| n.kind.clone()), ds.is_dirty())
    };
    let around = |r: &Rec| px(&r.image(), 40, 450);
    let page = [0x1E, 0x1E, 0x1E];
    assert_eq!(around(&r), page, "by default the Background of the Prototype settings");
    assert_eq!(px(&r.image(), 720, 470), [255, 255, 255], "and the frame itself is the file's");
    // The control channel picks one: a preset, then a hex colour, then back to the page's.
    let set = |r: &mut Rec, v: &str| {
        let res = crate::control::handle(&mut r.u.app, &r.u.ctx, "ui.set", &json!({"presentBackground": v}));
        r.frames(2);
        res
    };
    set(&mut r, "white").unwrap();
    assert_eq!(around(&r), [255, 255, 255]);
    assert_eq!(px(&r.image(), 700, 10), [0x2C, 0x2C, 0x2C], "the bar keeps its colours");
    set(&mut r, "#336699").unwrap();
    assert_eq!(around(&r), [0x33, 0x66, 0x99]);
    assert_eq!(r.u.app.inspect()["presentOptions"]["background"], json!("#336699"));
    assert!(set(&mut r, "chartreuse").is_err(), "unknown colours are refused");
    assert_eq!(around(&r), [0x33, 0x66, 0x99], "and change nothing");
    // Not the file's: the page's setting and the file's unsaved state are as they were.
    {
        let ds = r.u.app.session.ds().unwrap();
        assert_eq!((ds.doc.node(ds.page).map(|n| n.kind.clone()), ds.is_dirty()), (page_before, dirty_before));
    }
    // Kept for the next presentation.
    r.u.app.ui.present = None;
    r.run("proto.present", json!({"start": a}));
    assert_eq!(around(&r), [0x33, 0x66, 0x99]);
    set(&mut r, "page").unwrap();
    assert_eq!(around(&r), page);
    // The B key.
    press(&mut r.u, Key::B, Modifiers::NONE);
    r.frames(2);
    assert_eq!(around(&r), [0x1E, 0x1E, 0x1E], "the first preset is dark");
    press(&mut r.u, Key::B, Modifiers::NONE);
    r.frames(2);
    assert_eq!(around(&r), [0x80, 0x80, 0x80]);
}

#[test]
fn the_options_menu_picks_a_swatch_or_types_a_hex_colour_and_stays_open() {
    let (mut u, _) = abc();
    let out = frame_out(&mut u, vec![]);
    click(&mut u, text_at(&out, &t("Options")));
    let menu = frame_out(&mut u, vec![]);
    for label in ["Page colour", "Dark", "Mid grey", "Light", "White", "Black"] {
        assert!(has_text(&menu, &t(label)), "the menu has {label}: {:?}", texts(&menu));
    }
    assert!(
        has_text(&menu, "✓ Page colour") || texts(&menu).iter().any(|(s, _)| s.contains("✓") && s.contains(&t("Page colour"))),
        "the page's colour is the one ticked"
    );
    click(&mut u, text_at(&menu, &t("White")));
    assert_eq!(presenter(&u)["background"], json!("#FFFFFF"));
    // The menu is still open (colours can be tried one after the other).
    let menu = frame_out(&mut u, vec![]);
    assert!(has_text(&menu, &t("Black")), "still open");
    click(&mut u, text_at(&menu, &t("Black")));
    assert_eq!(presenter(&u)["background"], json!("#000000"));
    // The hex field takes a colour typed in (Enter commits); what isn't a colour changes nothing.
    let menu = frame_out(&mut u, vec![]);
    let field = texts(&menu).into_iter().find(|(s, _)| s == "000000").expect("the hex field shows the colour drawn").1;
    for (typed, want) in [("336699", "#336699"), ("nope", "#336699")] {
        click(&mut u, field);
        frame_out(&mut u, key_events(Key::A, Modifiers::COMMAND));
        frame_out(&mut u, vec![Event::Text(typed.to_string())]);
        frame_out(&mut u, key_events(Key::Enter, Modifiers::NONE));
        u.frames(2, Vec::new);
        assert_eq!(presenter(&u)["background"], json!(want), "typed {typed:?}");
    }
    assert!(u.app.ui.present.is_some());
}

#[test]
fn a_device_gets_a_shadow_on_a_light_background_so_a_pale_body_keeps_its_outline() {
    let (d, color) = (figcraft_doc::proto::PrototypeSettings::DEVICES[0], figcraft_doc::proto::PrototypeSettings::DEVICES[0].colors[1]);
    assert_eq!(color.name, "White");
    let count = |light: bool| {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let screen = Rect::from_min_size(pos2(100.0, 100.0), vec2(d.width as f32, d.height as f32));
            super::device_body(ui.painter(), d.name, screen, false, color, light);
        });
        out.textures_delta.clear();
        out.shapes.len()
    };
    assert!(count(true) > count(false), "the shadow is one more shape");
}
