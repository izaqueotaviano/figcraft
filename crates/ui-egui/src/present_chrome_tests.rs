//! The presenter's own controls: the shortcut table that its keys, the Options menu's hints and the
//! "?" list all read, and stepping through the page's screens. Driven headless through egui (the
//! harness is `present_tests`'); the window is 1440×900 and the presenter's bar takes 40.

use egui::{Event, Key, Modifiers, Pos2, Rect, Shape, pos2};
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

/// `s` translated (the tests read the screen in whatever language the UI is set to).
pub(crate) fn t(s: &str) -> String {
    crate::i18n::tr(s).to_string()
}

pub(crate) fn has_text(out: &egui::FullOutput, s: &str) -> bool {
    texts(out).iter().any(|(t, _)| t.trim() == s)
}

/// The middle of the text `s` on screen (menu rows have a check mark's room before them).
pub(crate) fn text_at(out: &egui::FullOutput, s: &str) -> Pos2 {
    texts(out).into_iter().find(|(t, _)| t.trim() == s).unwrap_or_else(|| panic!("no text {s:?} on screen")).1
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
