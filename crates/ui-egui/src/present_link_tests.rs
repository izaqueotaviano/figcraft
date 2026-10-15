//! Open-link actions in the player: which links open, and in a new tab or in the page itself.
//! Services record what they are asked to open (nothing opens a browser); gestures go through
//! egui headless on the harness's clock.

use std::sync::{Arc, Mutex};

use egui::{Event, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::present_tests::{Ui as Harness, id};
use crate::{FigApp, Services};

/// What the services were asked to open: (address, "tab" or "page").
type Opened = Arc<Mutex<Vec<(String, &'static str)>>>;

struct Recorder(Opened);

impl Services for Recorder {
    fn open_url(&mut self, url: &str) {
        self.0.lock().unwrap().push((url.to_string(), "tab"));
    }
    fn open_url_here(&mut self, url: &str) {
        self.0.lock().unwrap().push((url.to_string(), "page"));
    }
}

/// A Services that only knows the desktop app's way: its `open_url_here` is the trait's default.
struct Desktop(Opened);

impl Services for Desktop {
    fn open_url(&mut self, url: &str) {
        self.0.lock().unwrap().push((url.to_string(), "browser"));
    }
}

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

/// A presented frame "A" with a 100 × 60 layer that opens `action` on a click, under `services`.
fn linked(action: Value, services: Box<dyn Services>) -> (Harness, Pos2) {
    let mut s = Session::new();
    let a = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 0, "width": 300, "height": 400, "name": "A"})));
    let hot = id(&ex(&mut s, "shape.rectangle", json!({"x": 50, "y": 50, "width": 100, "height": 60, "parent": a})));
    ex(&mut s, "proto.interaction.add", json!({"id": hot, "action": action, "transition": {"type": "instant"}}));
    let mut u = Harness { ctx: egui::Context::default(), app: FigApp::new(s, services), time: 0.0, size: vec2(1440.0, 900.0), repaint: None };
    u.frames(3, Vec::new);
    u.run("proto.present", json!({"start": a}));
    u.frames(2, Vec::new);
    let at = hotspot(&u, hot).center();
    (u, at)
}

fn hotspot(u: &Harness, layer: NodeId) -> Rect {
    let hot = u.app.inspect()["presenter"]["hotspots"].as_array().cloned().unwrap_or_default();
    let h = hot.iter().find(|h| h["id"] == json!(layer)).unwrap_or_else(|| panic!("no hotspot on {layer}: {hot:?}"));
    let r: Vec<f32> = h["rect"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
    Rect::from_min_max(pos2(r[0], r[1]), pos2(r[2], r[3]))
}

fn click(u: &mut Harness, at: Pos2) {
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    u.frame(vec![Event::PointerMoved(at)]);
    u.frame(vec![button(true)]);
    u.frame(vec![button(false)]);
    u.frames(2, Vec::new);
}

fn toast(u: &Harness) -> Option<String> {
    u.app.ui.toast.as_ref().map(|t| t.0.clone())
}

#[test]
fn a_link_opens_in_a_new_tab_or_in_the_page_as_its_action_says() {
    // (action, what opens, why)
    let cases = [
        (json!({"type": "openUrl", "url": "https://example.com"}), "tab", "by default"),
        (json!({"type": "openUrl", "url": "https://example.com", "newTab": true}), "tab", "on"),
        (json!({"type": "openUrl", "url": "https://example.com", "newTab": false}), "page", "off: the page itself"),
    ];
    for (action, how, why) in cases {
        let opened = Opened::default();
        let (mut u, at) = linked(action.clone(), Box::new(Recorder(opened.clone())));
        click(&mut u, at);
        assert_eq!(*opened.lock().unwrap(), [("https://example.com".to_string(), how)], "{action}: {why}");
        assert_eq!(toast(&u), None, "{action}: nothing to complain about");
        // Once per click.
        u.frames(5, Vec::new);
        assert_eq!(opened.lock().unwrap().len(), 1, "{action}");
    }
}

#[test]
fn the_desktop_app_opens_the_system_browser_either_way() {
    for new_tab in [true, false] {
        let opened = Opened::default();
        let (mut u, at) = linked(json!({"type": "openUrl", "url": "https://example.com", "newTab": new_tab}), Box::new(Desktop(opened.clone())));
        click(&mut u, at);
        assert_eq!(*opened.lock().unwrap(), [("https://example.com".to_string(), "browser")], "new tab {new_tab}");
    }
}

#[test]
fn only_web_mail_and_phone_links_open() {
    // (what the file says, what opens (None: nothing, and the viewer is told))
    let cases = [
        ("example.com/docs", Some("https://example.com/docs")),
        ("mailto:me@example.com", Some("mailto:me@example.com")),
        ("tel:+15551234567", Some("tel:+15551234567")),
        ("javascript:alert(document.cookie)", None),
        ("file:///etc/passwd", None),
        ("data:text/html,<b>x</b>", None),
        ("ms-msdt:/id PCWDiagnostic", None),
        ("java\tscript:alert(1)", None),
    ];
    for new_tab in [true, false] {
        for (url, want) in cases {
            let opened = Opened::default();
            let (mut u, at) = linked(json!({"type": "openUrl", "url": url, "newTab": new_tab}), Box::new(Recorder(opened.clone())));
            click(&mut u, at);
            let how = if new_tab { "tab" } else { "page" };
            let got = opened.lock().unwrap().clone();
            match want {
                Some(link) => assert_eq!(got, [(link.to_string(), how)], "{url}"),
                None => {
                    assert!(got.is_empty(), "{url}: opened {got:?}");
                    assert!(toast(&u).is_some_and(|t| t.contains("only web, mail and phone links open")), "{url}: {:?}", toast(&u));
                }
            }
        }
    }
}
