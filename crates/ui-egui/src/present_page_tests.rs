//! The presenter as a web page's viewer: where the prototype sits in the window (centred, never
//! flush left after scrolling and a resize), a page taller than the window scrolling through it
//! at 100% (the wheel, drags, "scroll to", preserved scroll, fixed and sticky layers), the wheel's
//! coast and its routing per axis, and hover states that keep the scroll. Driven headless through
//! egui (the harness is `present_tests`'); the window is 1440×900 and the presenter's bar takes
//! 40, so the prototype's area is 1440×860 from y 40.

use egui::{Event, MouseWheelUnit, PointerButton, Pos2, TouchPhase, Vec2, pos2, vec2};
use figcraft_doc::NodeId;
use figcraft_engine::Session;
use serde_json::{Value, json};

use super::Presenter;
use crate::present_tests::{Ui, id, setup};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd} {p}: {e}"))
}

fn frame(s: &mut Session, name: &str, x: f64, w: f64, h: f64) -> NodeId {
    id(&ex(s, "frame.create", json!({"x": x, "y": 0, "width": w, "height": h, "name": name})))
}

fn rect(s: &mut Session, name: &str, parent: NodeId, [x, y, w, h]: [f64; 4]) -> NodeId {
    id(&ex(s, "shape.rectangle", json!({"x": x, "y": y, "width": w, "height": h, "parent": parent, "props": {"name": name}})))
}

/// `layer` runs `action` on a click (instant).
fn on_click(s: &mut Session, layer: NodeId, action: Value) {
    ex(s, "proto.interaction.add", json!({"id": layer, "trigger": {"type": "onClick"}, "action": action, "transition": {"type": "instant"}}));
}

/// `layer` opens a link while hovered (a "while hovering" state that changes nothing on screen).
fn hover_link(layer: NodeId) -> Value {
    json!({"id": layer, "trigger": {"type": "onHover"}, "action": {"type": "openUrl", "url": "https://example.com"}})
}

/// A page without a device: frame "Page" (`w`×`h`, presented) with what `build` adds, and a
/// frame "Next" (`w`×`h`) beside it.
fn page(w: f64, h: f64, build: impl FnOnce(&mut Session, NodeId, NodeId)) -> (Ui, NodeId, NodeId) {
    let mut s = Session::new();
    let page = frame(&mut s, "Page", 0.0, w, h);
    let next = frame(&mut s, "Next", w + 100.0, w, h);
    build(&mut s, page, next);
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": page}));
    (u, page, next)
}

fn presenter(u: &Ui) -> Value {
    u.app.inspect()["presenter"].clone()
}

fn player(u: &Ui) -> &Presenter {
    u.app.ui.present.as_ref().expect("presenting")
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn pan(u: &Ui) -> (f64, f64) {
    let p = presenter(u);
    (num(&p["pan"][0]), num(&p["pan"][1]))
}

/// The presented frame's rect on screen (x0, y0, x1, y1).
fn screen(u: &Ui) -> [f64; 4] {
    let r = presenter(u)["screenRect"].clone();
    [num(&r[0]), num(&r[1]), num(&r[2]), num(&r[3])]
}

/// The centre of the presenter's hotspot on `layer`, if it shows.
fn hotspot(u: &Ui, layer: NodeId) -> Option<Pos2> {
    let hot = presenter(u)["hotspots"].as_array().cloned().unwrap_or_default();
    let h = hot.iter().find(|h| h["id"] == json!(layer))?;
    let r: Vec<f32> = h["rect"].as_array()?.iter().filter_map(Value::as_f64).map(|v| v as f32).collect();
    Some(pos2((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0))
}

/// The top of the presenter's hotspot on `layer` (window points).
fn hotspot_top(u: &Ui, layer: NodeId) -> f64 {
    let hot = presenter(u)["hotspots"].as_array().cloned().unwrap_or_default();
    let h = hot.iter().find(|h| h["id"] == json!(layer)).unwrap_or_else(|| panic!("no hotspot on {layer}: {hot:?}"));
    num(&h["rect"][1])
}

fn wheel_event(d: Vec2, phase: TouchPhase) -> Event {
    Event::MouseWheel { unit: MouseWheelUnit::Point, delta: d, phase, modifiers: Default::default() }
}

/// The pointer at `at` and the wheel turned by `d` (points; a mouse wheel's step when large,
/// a trackpad's below 8).
fn wheel(at: Pos2, d: Vec2) -> Vec<Event> {
    vec![Event::PointerMoved(at), wheel_event(d, TouchPhase::Move)]
}

fn click(u: &mut Ui, at: Pos2) {
    u.frame(vec![Event::PointerMoved(at)]);
    u.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    u.frame(vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
}

/// Click the middle of `layer`'s hotspot.
fn click_on(u: &mut Ui, layer: NodeId) {
    let at = hotspot(u, layer).unwrap_or_else(|| panic!("no hotspot on {layer}"));
    click(u, at);
}

/// Press at `from`, move by `by` in five steps, release.
fn drag(u: &mut Ui, from: Pos2, by: Vec2) {
    u.frame(vec![Event::PointerMoved(from)]);
    u.frame(vec![Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
    for k in 1..=5 {
        u.frame(vec![Event::PointerMoved(from + by * (k as f32 / 5.0))]);
    }
    u.frame(vec![Event::PointerButton { pos: from + by, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
}

fn present_scroll(u: &mut Ui, x: f64, y: f64) {
    crate::control::handle(&mut u.app, &u.ctx, "ui.set", &json!({"presentScroll": [x, y]})).unwrap();
    u.frame(vec![]);
}

const MIDDLE: Pos2 = pos2(720.0, 470.0);

#[test]
fn a_tall_page_shows_at_100_percent_from_its_top_and_the_wheel_scrolls_it() {
    let (mut u, ..) = page(1200.0, 3000.0, |_, _, _| {});
    let p = presenter(&u);
    assert_eq!((p["scale"].as_str(), num(&p["zoom"])), (Some("actual"), 1.0), "100% by default");
    // It fits across (with the padding): centred. Down it overflows: from its top, edge to edge.
    assert_eq!(screen(&u), [120.0, 40.0, 1320.0, 3040.0]);
    assert_eq!(p["panRange"], json!([0.0, 2140.0]));
    let rev = player(&u).rev;
    u.frames(4, || wheel(MIDDLE, vec2(0.0, -80.0)));
    u.frames(60, Vec::new);
    let (x, y) = pan(&u);
    assert!(x == 0.0 && y > 300.0, "{x} {y}");
    let s = screen(&u);
    assert!((s[1] - (40.0 - y)).abs() < 0.01 && s[0] == 120.0, "{s:?} {y}");
    assert_eq!(player(&u).rev, rev, "the view pans the page's texture: nothing rendered again");
}

#[test]
fn a_scrolled_view_stays_centred_when_the_area_narrows() {
    // 1300 wide: it fits the window across, not the area left beside the Inspect panel.
    let (mut u, ..) = page(1300.0, 3000.0, |_, _, _| {});
    u.frames(4, || wheel(MIDDLE, vec2(0.0, -80.0)));
    u.frames(60, Vec::new);
    let (x, y) = pan(&u);
    assert!(x == 0.0 && y > 100.0, "{x} {y}");
    let centred = |u: &Ui, why: &str| {
        let p = presenter(u);
        let range = num(&p["panRange"][0]);
        assert!(range > 50.0, "{why}: {p}");
        assert_eq!(num(&p["pan"][0]), range / 2.0, "{why}: centred across, not flush left: {p}");
        assert!((num(&p["pan"][1]) - y).abs() < 1e-6, "{why}: as far down as it was: {p}");
    };
    // The Inspect panel (the bar's button) takes 280 points of the window.
    u.app.ui.present.as_mut().unwrap().view.inspect = true;
    u.frames(2, Vec::new);
    centred(&u, "Inspect open");
    // The window narrower still.
    u.size = vec2(1100.0, 900.0);
    u.frames(2, Vec::new);
    centred(&u, "a narrower window");
    // Back: it fits again, centred.
    u.size = vec2(1440.0, 900.0);
    u.app.ui.present.as_mut().unwrap().view.inspect = false;
    u.frames(2, Vec::new);
    let s = screen(&u);
    assert_eq!((s[0] + s[2]) / 2.0, 720.0, "{s:?}");
}

#[test]
fn a_page_narrower_than_the_window_stays_centred_after_scrolling() {
    let (mut u, ..) = page(1200.0, 3000.0, |_, _, _| {});
    u.frames(4, || wheel(MIDDLE, vec2(0.0, -80.0)));
    u.frames(60, Vec::new);
    let y = pan(&u).1;
    let mid = |u: &Ui| {
        let s = screen(u);
        (s[0] + s[2]) / 2.0
    };
    assert_eq!(mid(&u), 720.0);
    // Narrower than the page: it pans across from the middle.
    u.size = vec2(1000.0, 900.0);
    u.frames(2, Vec::new);
    assert_eq!(mid(&u), 500.0);
    u.size = vec2(1440.0, 900.0);
    u.frames(2, Vec::new);
    assert_eq!(mid(&u), 720.0);
    assert!((pan(&u).1 - y).abs() < 1e-6, "the scroll kept");
}

#[test]
fn the_wheel_coasts_on_by_itself_after_its_last_input() {
    let (mut u, ..) = page(1200.0, 6000.0, |_, _, _| {});
    u.frames(2, || wheel(MIDDLE, vec2(0.0, -100.0)));
    // No more input, frames drawn only when asked for: the page goes on (egui's smoothing of the
    // wheel's steps, then the coast) until it comes to rest, and only then does the window go idle
    // (else the coast would start, or go on, only when the mouse next moves).
    let (mut coasted, mut idled, mut last) = (false, false, pan(&u).1);
    for k in 0..400 {
        idled |= u.next_frame();
        let (y, flinging) = (pan(&u).1, presenter(&u)["flinging"] == json!(true));
        assert!(!idled || (y == last && !flinging), "frame {k}: still moving at {y} after the window went idle");
        coasted |= flinging;
        last = y;
    }
    assert!(coasted, "it coasted");
    assert!(idled, "and at rest the window sleeps");
}

#[test]
fn a_trackpads_own_momentum_isnt_doubled_by_a_coast() {
    let (mut u, ..) = page(1200.0, 6000.0, |_, _, _| {});
    u.frame(vec![Event::PointerMoved(MIDDLE), wheel_event(Vec2::ZERO, TouchPhase::Start)]);
    let mut total = 0.0;
    // Fingers on the trackpad.
    for _ in 0..8 {
        u.frame(vec![wheel_event(vec2(0.0, -6.0), TouchPhase::Move)]);
        total += 6.0;
    }
    u.frame(vec![wheel_event(Vec2::ZERO, TouchPhase::End)]);
    // The system's momentum, slowing down, sometimes two frames apart.
    for (k, d) in [6.0, 6.0, 5.0, 5.0, 5.0, 4.0, 4.0, 4.0, 3.0, 3.0, 3.0, 2.0, 2.0, 2.0, 2.0, 1.0].into_iter().enumerate() {
        u.frame(vec![wheel_event(vec2(0.0, -d as f32), TouchPhase::Move)]);
        total += d;
        if k % 4 == 3 && k < 12 {
            u.frames(2, Vec::new);
        }
        assert_eq!(presenter(&u)["flinging"], json!(false), "event {k}: no coast while the momentum comes");
        assert_eq!(pan(&u).1, total, "event {k}: the page moves with the momentum only");
    }
    // Still for less than the grace period: no coast yet.
    u.frames(2, Vec::new);
    assert_eq!(presenter(&u)["flinging"], json!(false));
    assert_eq!(pan(&u).1, total);
}

/// A page 1000×3000 (it scrolls down through the window) with a carousel 1000×600 at y 200 that
/// scrolls sideways through a strip 3000 wide.
fn carousel() -> (Ui, NodeId) {
    let mut c = NodeId(0);
    let (u, ..) = page(1000.0, 3000.0, |s, page, _| {
        c = id(&ex(s, "frame.create", json!({"x": 0, "y": 200, "width": 1000, "height": 600, "parent": page, "name": "Carousel"})));
        ex(s, "node.set", json!({"id": c, "props": {"overflow": "horizontal"}}));
        rect(s, "Strip", c, [0.0, 0.0, 3000.0, 600.0]);
    });
    (u, c)
}

fn across(u: &Ui, f: NodeId) -> f64 {
    player(u).scroll.get(&f).map_or(0.0, |s| s.0)
}

#[test]
fn each_axis_of_the_wheel_scrolls_what_moves_that_way() {
    let (mut u, c) = carousel();
    // Near the carousel's top (at 240 on screen): as the page scrolls up it stays over it.
    let at = pos2(720.0, 260.0);
    // Diagonal: across the carousel scrolls, down the page.
    u.frames(2, || wheel(at, vec2(-30.0, -60.0)));
    u.frames(60, Vec::new);
    let (x, y) = (across(&u, c), pan(&u).1);
    assert!(x > 20.0 && y > 40.0, "carousel {x}, page {y}");
    // Mostly down: the little sideways jitter is ignored.
    u.frames(2, || wheel(at, vec2(-5.0, -60.0)));
    u.frames(60, Vec::new);
    assert_eq!(across(&u, c), x, "jitter across ignored");
    assert!(pan(&u).1 > y + 40.0, "the page scrolled on");
}

#[test]
fn a_drag_scrolls_what_moves_along_it() {
    let (mut u, c) = carousel();
    // Up over the carousel: the page scrolls, not the carousel.
    drag(&mut u, pos2(720.0, 400.0), vec2(0.0, -100.0));
    u.frames(300, Vec::new);
    assert_eq!(presenter(&u)["flinging"], json!(false));
    let y = pan(&u).1;
    assert!(y > 90.0, "{y}");
    assert_eq!(across(&u, c), 0.0);
    // Sideways over it: the carousel.
    let mid = 40.0 + 200.0 + 300.0 - y as f32;
    drag(&mut u, pos2(900.0, mid), vec2(-100.0, 0.0));
    u.frames(60, Vec::new);
    assert!(across(&u, c) > 90.0, "{}", across(&u, c));
    assert_eq!(pan(&u).1, y);
}

#[test]
fn leaving_a_while_hovering_state_keeps_the_scroll() {
    // On a device the frame scrolls on the screen; a layer over all of it is hovered.
    let (mut u, a, ..) = setup();
    let cover = id(&u.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 393, "height": 2000, "parent": a, "props": {"name": "Cover"}})));
    u.run("proto.interaction.add", hover_link(cover));
    u.run("proto.present", json!({"start": a}));
    let at = hotspot(&u, cover).unwrap();
    u.frames(2, || vec![Event::PointerMoved(at)]);
    assert!(player(&u).temp.is_some(), "hovering");
    // Scrolled while hovering…
    u.frames(10, || wheel(at, vec2(0.0, -6.0)));
    let scrolled = num(&presenter(&u)["scroll"][1]);
    assert_eq!(scrolled, 60.0);
    // …and left: the state reverts, the scroll stays.
    u.frame(vec![Event::PointerMoved(pos2(100.0, 450.0))]);
    u.frames(2, Vec::new);
    assert!(player(&u).temp.is_none(), "reverted");
    assert_eq!(num(&presenter(&u)["scroll"][1]), scrolled);
}

#[test]
fn the_wheel_isnt_lost_in_the_frame_a_hover_state_reverts() {
    let mut button = NodeId(0);
    let (mut u, ..) = page(1200.0, 3000.0, |s, page, _| {
        button = rect(s, "Button", page, [100.0, 100.0, 300.0, 100.0]);
        ex(s, "proto.interaction.add", hover_link(button));
    });
    let at = hotspot(&u, button).unwrap();
    u.frames(2, || vec![Event::PointerMoved(at)]);
    assert!(player(&u).temp.is_some(), "hovering");
    // Off the button with a trackpad's step in the same frame.
    u.frame(wheel(pos2(720.0, 600.0), vec2(0.0, -6.0)));
    assert!(player(&u).temp.is_none(), "reverted");
    assert_eq!(pan(&u).1, 6.0, "and scrolled");
}

#[test]
fn the_outgoing_frame_keeps_its_place_during_a_transition() {
    let mut button = NodeId(0);
    let (mut u, _, next) = page(1200.0, 3000.0, |s, page, next| {
        button = rect(s, "Button", page, [100.0, 2900.0, 300.0, 40.0]);
        ex(
            s,
            "proto.interaction.add",
            json!({"id": button, "trigger": {"type": "onClick"}, "action": {"type": "navigate", "dest": next}, "transition": {"type": "dissolve"}, "durationMs": 300}),
        );
    });
    present_scroll(&mut u, 0.0, 99_999.0);
    assert_eq!(pan(&u).1, 2140.0, "at the bottom");
    let was = player(&u).shown.map(|s| s.1.origin).unwrap();
    assert_eq!(was.y, 40.0 - 2140.0);
    click_on(&mut u, button);
    let p = player(&u);
    assert_eq!(p.current, next);
    let from = p.anim.as_ref().and_then(|a| a.from_place).map(|pl| pl.origin);
    assert_eq!(from, Some(was), "the page left stays scrolled where it was");
    assert_eq!(pan(&u).1, 0.0, "the next page starts at its top");
}

#[test]
fn scroll_to_and_preserved_scroll_pan_a_page_without_a_device() {
    let (mut link, mut keep, mut top, mut section) = (NodeId(0), NodeId(0), NodeId(0), NodeId(0));
    let (mut u, page_id, next) = page(1200.0, 3000.0, |s, page, next| {
        section = rect(s, "Section", page, [0.0, 2000.0, 1200.0, 400.0]);
        link = rect(s, "Link", page, [100.0, 100.0, 200.0, 40.0]);
        on_click(s, link, json!({"type": "scrollTo", "dest": section, "offset": [0, 60]}));
        keep = rect(s, "Keep", page, [100.0, 600.0, 200.0, 40.0]);
        on_click(s, keep, json!({"type": "navigate", "dest": next, "preserveScroll": true}));
        top = rect(s, "Top", page, [400.0, 600.0, 200.0, 40.0]);
        on_click(s, top, json!({"type": "navigate", "dest": next}));
    });
    // An anchor link: the section comes to the window's top, 60 short of it.
    click_on(&mut u, link);
    assert_eq!(pan(&u), (0.0, 1940.0));
    // The control channel scrolls the page too (across it stays centred).
    present_scroll(&mut u, 300.0, 500.0);
    assert_eq!(pan(&u), (0.0, 500.0));
    // Navigating keeps the scroll position when asked to, else starts at the top.
    click_on(&mut u, keep);
    assert_eq!((presenter(&u)["current"].clone(), pan(&u)), (json!(next), (0.0, 500.0)));
    u.run("proto.present", json!({"start": page_id}));
    present_scroll(&mut u, 0.0, 500.0);
    click_on(&mut u, top);
    assert_eq!((presenter(&u)["current"].clone(), pan(&u)), (json!(next), (0.0, 0.0)));
}

#[test]
fn an_animated_scroll_to_pans_the_page_over_its_duration() {
    let (mut link, mut section) = (NodeId(0), NodeId(0));
    let (mut u, ..) = page(1200.0, 3000.0, |s, page, _| {
        section = rect(s, "Section", page, [0.0, 2000.0, 1200.0, 400.0]);
        link = rect(s, "Link", page, [100.0, 100.0, 200.0, 40.0]);
        ex(
            s,
            "proto.interaction.add",
            json!({"id": link, "trigger": {"type": "onClick"}, "action": {"type": "scrollTo", "dest": section}, "transition": {"type": "dissolve"}, "durationMs": 300, "easing": "linear"}),
        );
    });
    click_on(&mut u, link);
    u.frames(5, Vec::new);
    let y = pan(&u).1;
    assert!(y > 100.0 && y < 2000.0, "on its way: {y}");
    u.frames(30, Vec::new);
    assert_eq!(pan(&u).1, 2000.0);
}

#[test]
fn fixed_and_sticky_layers_stay_put_while_the_page_scrolls() {
    let ids = std::cell::RefCell::new(Vec::new());
    let (mut u, ..) = page(1200.0, 3000.0, |s, page, next| {
        let go = json!({"type": "navigate", "dest": next});
        let header = rect(s, "Header", page, [0.0, 0.0, 1200.0, 60.0]);
        let footer = rect(s, "Footer", page, [0.0, 2940.0, 1200.0, 60.0]);
        let sticky = rect(s, "Tabs", page, [0.0, 300.0, 1200.0, 50.0]);
        let item = rect(s, "Item", page, [100.0, 1000.0, 200.0, 100.0]);
        ex(s, "node.set", json!({"id": header, "props": {"scrollPosition": "fixed"}}));
        ex(s, "node.set", json!({"id": footer, "props": {"scrollPosition": "fixed", "constraints": {"vertical": "bottom"}}}));
        ex(s, "node.set", json!({"id": sticky, "props": {"scrollPosition": "sticky"}}));
        for l in [header, footer, sticky, item] {
            on_click(s, l, go.clone());
        }
        ids.borrow_mut().extend([header, footer, sticky, item]);
    });
    let [header, footer, sticky, item] = <[NodeId; 4]>::try_from(ids.into_inner()).unwrap();
    let tops = |u: &Ui, want: [f64; 3], why: &str| {
        let got = [hotspot_top(u, header), hotspot_top(u, footer), hotspot_top(u, sticky)];
        assert!(got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.01), "{why}: {got:?}");
    };
    tops(&u, [40.0, 840.0, 340.0], "at the top: the header at the window's top, the footer at its bottom, the tabs in place");
    assert!(hotspot(&u, item).is_none(), "below the window");
    let rev = player(&u).rev;
    u.frames(4, || wheel(MIDDLE, vec2(0.0, -150.0)));
    u.frames(60, Vec::new);
    let y = pan(&u).1;
    assert!(y > 400.0 && y < 900.0, "{y}");
    tops(&u, [40.0, 840.0, 40.0], "scrolled: fixed layers stay, the tabs stop at the top");
    assert!((hotspot_top(&u, item) - (1040.0 - y)).abs() < 0.01);
    assert_eq!(player(&u).rev, rev, "panned: nothing rendered again");
    assert_eq!(player(&u).shown_pins.len(), 3, "each pinned layer drawn from its own texture");
    // Clicks reach the pinned layers where they show.
    click_on(&mut u, header);
    assert_eq!(presenter(&u)["name"], "Next");
}

#[test]
fn a_tall_frame_on_a_device_scrolls_even_when_set_not_to() {
    let mut s = Session::new();
    ex(&mut s, "proto.settings", json!({"device": "iPhone 16"}));
    let a = frame(&mut s, "A", 0.0, 393.0, 2000.0);
    // A frame inside it keeps its own setting: not scrolling.
    let inner = id(&ex(&mut s, "frame.create", json!({"x": 0, "y": 100, "width": 393, "height": 300, "parent": a, "name": "Inner"})));
    rect(&mut s, "Tall", inner, [0.0, 0.0, 393.0, 900.0]);
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": a}));
    assert_eq!(presenter(&u)["scrollRange"], json!([0.0, 1148.0]));
    let s = screen(&u);
    let at = pos2(((s[0] + s[2]) / 2.0) as f32, (s[1] + 200.0) as f32);
    u.frames(4, || wheel(at, vec2(0.0, -80.0)));
    u.frames(60, Vec::new);
    assert!(num(&presenter(&u)["scroll"][1]) > 100.0);
    assert_eq!(player(&u).scroll.get(&inner), None, "the inner frame doesn't scroll");
    present_scroll(&mut u, 0.0, 5000.0);
    assert_eq!(presenter(&u)["scroll"], json!([0.0, 1148.0]));
}

#[test]
fn responsive_takes_the_windows_width_keeps_the_height_and_scrolls() {
    let mut s = Session::new();
    let wide = frame(&mut s, "Wide", 0.0, 600.0, 400.0);
    let tall = frame(&mut s, "Tall", 700.0, 600.0, 3000.0);
    ex(&mut s, "proto.settings", json!({"responsive": true}));
    let mut u = Ui::new(s);
    u.run("proto.present", json!({"start": wide}));
    let p = presenter(&u);
    assert_eq!((p["frameSize"].clone(), p["zoom"].clone(), p["panRange"].clone()), (json!([1440.0, 400.0]), json!(1.0), json!([0.0, 0.0])));
    // A tall page keeps its height and scrolls through the window.
    u.run("proto.present", json!({"start": tall}));
    let p = presenter(&u);
    assert_eq!((p["frameSize"].clone(), p["panRange"].clone()), (json!([1440.0, 3000.0]), json!([0.0, 2140.0])));
    u.frames(4, || wheel(MIDDLE, vec2(0.0, -80.0)));
    u.frames(60, Vec::new);
    assert!(pan(&u).1 > 100.0);
    // Turned off in the player (here by the command): the frame as designed; the file keeps its setting.
    let undo = u.app.session.ds().unwrap().history.undo.len();
    u.run("proto.presentOptions", json!({"responsive": false}));
    assert_eq!(presenter(&u)["frameSize"], json!([600.0, 3000.0]));
    assert_eq!(u.app.inspect()["presentOptions"]["responsive"], json!(false));
    let doc = u.app.session.doc().unwrap();
    assert!(crate::present::page_settings(doc, tall).responsive, "the file's setting is untouched");
    assert_eq!(u.app.session.ds().unwrap().history.undo.len(), undo, "nothing to undo");
    // The next presentation starts from the file's setting again.
    u.run("proto.present", json!({"start": tall}));
    assert_eq!(presenter(&u)["frameSize"], json!([1440.0, 3000.0]));
}

#[test]
fn overlays_sit_on_the_part_of_a_tall_page_in_the_window() {
    let (mut button, mut close) = (NodeId(0), NodeId(0));
    let (mut u, ..) = page(1200.0, 3000.0, |s, page, _| {
        let menu = frame(s, "Menu", 3000.0, 400.0, 200.0);
        close = rect(s, "Close", menu, [0.0, 0.0, 400.0, 200.0]);
        on_click(s, close, json!({"type": "closeOverlay"}));
        button = rect(s, "Button", page, [100.0, 100.0, 200.0, 40.0]);
        on_click(s, button, json!({"type": "openOverlay", "dest": menu}));
    });
    click_on(&mut u, button);
    u.frame(vec![]);
    // Centred on the window's part of the page (40 to 900), not on the whole 3000 high page.
    assert_eq!(hotspot(&u, close), Some(pos2(720.0, 470.0)));
}
