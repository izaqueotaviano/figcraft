//! Every main-menu item does something you can see when clicked: it changes the document, the
//! selection or view, opens a dialog, panel or popup, or says what happened (a message). An item
//! that can't do anything now is greyed, with the reason on hover ([`crate::menus::why_disabled`]),
//! instead of flashing the menu shut. Items that need a value ask for it instead of failing.

use egui::{Pos2, RawInput, Rect, Shape, epaint::ClippedShape, vec2};
use figcraft_engine::Session;
use figcraft_engine::catalog::{MENU, SEP};
use serde_json::{Value, json};

use crate::FigApp;

/// File ▸ Open recent is a submenu, not an action.
const OUTSIDE: &[&str] = &["file.openRecent"];

/// Platform services that note what was asked of them: a file picker, the clipboard or the
/// browser is something the user sees.
#[derive(Clone, Default)]
struct Spy(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

impl Spy {
    fn note(&self, what: &str) {
        if let Ok(mut v) = self.0.lock() {
            v.push(what.to_string());
        }
    }
    fn count(&self) -> usize {
        self.0.lock().map(|v| v.len()).unwrap_or(0)
    }
}

impl crate::Services for Spy {
    fn pick_open(&mut self, _title: &str, _exts: &[&str]) -> Option<std::path::PathBuf> {
        self.note("pick_open");
        None
    }
    fn pick_save(&mut self, _title: &str, _name: &str, _exts: &[&str]) -> Option<std::path::PathBuf> {
        self.note("pick_save");
        None
    }
    fn pick_folder(&mut self, _title: &str) -> Option<std::path::PathBuf> {
        self.note("pick_folder");
        None
    }
    fn clipboard_set_text(&mut self, _s: &str) {
        self.note("clipboard");
    }
    fn clipboard_set_png(&mut self, _w: u32, _h: u32, _rgba: &[u8]) {
        self.note("clipboard");
    }
    fn open_url(&mut self, _url: &str) {
        self.note("open_url");
    }
}

/// Items that show or set something that may already be shown or set (a panel, a zoom level, a
/// theme): they must have an effect in at least one of the states the sweep starts from.
const IDEMPOTENT: &[&str] = &[
    "view.layers",
    "view.assets",
    "view.design",
    "view.prototype",
    "view.zoom100",
    "view.zoom50",
    "view.zoom200",
    "view.zoomFit",
    "view.themeLight",
    "view.themeDark",
    "view.themeSystem",
    "select.all",
    "edit.pickColor",
    "libraries.open",
];

/// The states a sweep starts from.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Start {
    /// A rectangle selected; the default panels.
    Selected,
    /// Nothing selected.
    Empty,
    /// Nothing selected, the left sidebar collapsed on Assets, the Prototype tab, zoomed to 50%,
    /// the dark theme, the Pen tool and a frame expanded in the layers panel.
    Elsewhere,
}

struct H {
    ctx: egui::Context,
    app: FigApp,
    spy: Spy,
}

fn id_of(v: &Value) -> figcraft_doc::NodeId {
    figcraft_engine::params::id_of(v.get("id").unwrap()).unwrap()
}

impl H {
    fn new(start: Start) -> Self {
        let mut s = Session::new();
        let rect = id_of(&s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap());
        s.execute("text.create", &json!({"x": 10, "y": 100, "text": "Hello"})).unwrap();
        let frame = id_of(&s.execute("frame.create", &json!({"x": 300, "y": 0, "width": 200, "height": 200})).unwrap());
        s.execute("shape.rectangle", &json!({"x": 320, "y": 20, "width": 40, "height": 40, "parent": frame})).unwrap();
        let ds = s.ds_mut().unwrap();
        ds.selection = if start == Start::Selected { vec![rect] } else { vec![] };
        let spy = Spy::default();
        let mut h = Self { ctx: egui::Context::default(), app: FigApp::new(s, Box::new(spy.clone())), spy };
        h.app.ui.home = false;
        // The first frames size the view (and fit the file in it).
        for _ in 0..3 {
            h.frame();
        }
        if start == Start::Elsewhere {
            h.app.ui.left_collapsed = true;
            h.app.ui.left_tab = crate::LeftTab::Assets;
            h.app.ui.right_tab = crate::RightTab::Prototype;
            h.app.ui.expanded.insert(frame);
            h.app.session.execute("view.zoom50", &json!({})).unwrap();
            h.app.session.execute("view.themeDark", &json!({})).unwrap();
            h.app.session.execute("tool.set", &json!({"tool": "pen"})).unwrap();
            h.app.handle_ui_requests();
            for _ in 0..2 {
                h.frame();
            }
        }
        h
    }

    /// A frame (always at the same time, so nothing animates) and what it drew, less the canvas
    /// image (renders arrive whenever the render thread is done).
    fn frame(&mut self) -> String {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), time: Some(1.0), ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(input, |ui| app.ui(ui));
        out.textures_delta.clear();
        let kept: Vec<&ClippedShape> = out.shapes.iter().filter(|s| !matches!(s.shape, Shape::Mesh(_))).collect();
        format!("{kept:?}")
    }

    /// What the session holds that a menu item can change without drawing anything new.
    fn state(&self) -> String {
        let s = &self.app.session;
        let ds = s.ds().ok();
        format!(
            "{:?}|{:?}|{:?}|{:?}|{}|{:?}|{:?}|{:?}|{}|{}|{}|{}",
            ds.map(|d| std::sync::Arc::as_ptr(&d.doc) as usize),
            ds.map(|d| d.selection.clone()),
            ds.map(|d| d.view.clone()),
            ds.map(|d| d.page),
            s.docs.len(),
            s.tool_info(),
            self.app.ui.toast.as_ref().map(|t| t.0.clone()),
            s.prefs,
            s.clipboard.nodes.len(),
            self.app.ui.expanded.len(),
            self.app.window_requests.len(),
            self.spy.count(),
        )
    }
}

/// What clicking each enabled main-menu item did, from `start`: `None` when nothing visible
/// happened, else the error it showed (empty: none).
fn sweep(start: Start) -> Vec<(&'static str, Option<String>)> {
    let mut out = vec![];
    for m in MENU.iter().filter(|m| m.label != SEP && !m.id.is_empty() && !OUTSIDE.contains(&m.id)) {
        // Only the menus the main menu shows.
        let top = m.menu.split('/').next().unwrap_or_default();
        if !crate::menus::TOP_MENUS.contains(&top) {
            continue;
        }
        let mut h = H::new(start);
        if !crate::menus::enabled(&h.app, m.id) {
            continue;
        }
        let (before_state, before_shapes) = (h.state(), h.frame());
        h.app.ui.last_error = None;
        crate::menus::invoke(&mut h.app, m.id);
        let error = h.app.ui.last_error.clone();
        let after_state = h.state();
        h.frame();
        let after_shapes = h.frame();
        let seen = before_state != after_state || before_shapes != after_shapes;
        out.push((m.id, seen.then(|| error.unwrap_or_default())));
    }
    out
}

#[test]
fn every_enabled_menu_item_does_something_visible_or_is_greyed() {
    let runs = [Start::Selected, Start::Empty, Start::Elsewhere].map(|s| (s, sweep(s)));
    let mut silent = vec![];
    let mut unasked = vec![];
    let mut needs_selection = vec![];
    for (start, results) in &runs {
        for (id, seen) in results {
            match seen {
                // An item that may already be so needs an effect from one of the starts.
                None if IDEMPOTENT.contains(id) => {
                    let anywhere = runs.iter().any(|(_, r)| r.iter().any(|(i, s)| i == id && s.is_some()));
                    if !anywhere && !silent.contains(&format!("{id} (always)")) {
                        silent.push(format!("{id} (always)"));
                    }
                }
                // The Elsewhere start checks the idempotent items only.
                None if *start != Start::Elsewhere => silent.push(format!("{id} ({start:?})")),
                None => {}
                // A value the menu never gave (the item should ask for it).
                Some(e) if e.contains("must be") => unasked.push(format!("{id}: {e}")),
                // A selection the item should have been greyed without.
                Some(e) if *start != Start::Selected && e.contains("nothing is selected") => needs_selection.push(*id),
                Some(_) => {}
            }
        }
    }
    assert!(silent.is_empty(), "enabled menu items that do nothing visible: {silent:?}");
    assert!(unasked.is_empty(), "menu items that fail for a value they never ask for: {unasked:?}");
    assert!(needs_selection.is_empty(), "menu items to grey with nothing selected: {needs_selection:?}");
}

#[test]
fn greyed_items_say_why() {
    let h = H::new(Start::Empty);
    // Acting on the selection, with nothing selected.
    assert_eq!(crate::menus::why_disabled(&h.app, "object.group"), Some("Select a layer first"));
    assert_eq!(crate::menus::why_disabled(&h.app, "align.left"), Some("Select a layer first"));
    // Nothing to redo in a new file; one page.
    assert_eq!(crate::menus::why_disabled(&h.app, "edit.redo"), Some("Nothing to redo"));
    assert_eq!(crate::menus::why_disabled(&h.app, "view.nextPage"), Some("This is the last page"));
    // Ids no command implements.
    assert_eq!(crate::menus::why_disabled(&h.app, "no.suchCommand"), Some("Not available yet"));
    // And what can run, can.
    assert_eq!(crate::menus::why_disabled(&h.app, "view.zoomIn"), None);
    assert_eq!(crate::menus::why_disabled(&h.app, "edit.undo"), None);
}

#[test]
fn items_that_need_a_value_ask_for_it() {
    // Text ▸ Create link opens a prompt for the address, which then makes the link.
    let mut h = H::new(Start::Empty);
    let text = h.app.session.doc().unwrap().nodes().find(|n| n.kind.text().is_some()).map(|n| n.id).unwrap();
    h.app.session.ds_mut().unwrap().selection = vec![text];
    crate::menus::invoke(&mut h.app, "text.link");
    let p = h.app.ui.prompt.clone().expect("a prompt opens");
    assert_eq!(p.kind, crate::dialogs::PromptKind::Link);
    h.frame();
    // Just the scheme it starts with isn't an address yet.
    assert!(h.frame().contains("Type a web address"));
    h.app.ui.prompt.as_mut().unwrap().value = "https://example.com".into();
    let before = h.app.session.ds().unwrap().doc.clone();
    // Enter submits.
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))),
        time: Some(1.0),
        events: vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
        ..Default::default()
    };
    let app = &mut h.app;
    h.ctx.run_ui(input, |ui| app.ui(ui)).textures_delta.clear();
    assert!(h.app.ui.prompt.is_none(), "the prompt closes once the link is made");
    assert!(!std::sync::Arc::ptr_eq(&before, &h.app.session.ds().unwrap().doc), "the link was made");
    // Vector ▸ Offset path asks for the distance.
    let mut h = H::new(Start::Selected);
    crate::menus::invoke(&mut h.app, "vector.offset");
    assert_eq!(h.app.ui.prompt.as_ref().map(|p| p.kind), Some(crate::dialogs::PromptKind::Offset));
    // File ▸ Rename puts the file name field in the layers panel into editing.
    let mut h = H::new(Start::Elsewhere);
    crate::menus::invoke(&mut h.app, "file.rename");
    assert!(!h.app.ui.left_collapsed && h.app.ui.left_tab == crate::LeftTab::File);
    h.frame();
    assert!(h.ctx.memory(|m| m.has_focus(egui::Id::new("filename").with("te"))));
}

/// Where the AccessKit tree puts the control `label` of `role` (its middle), if it's there.
fn control_at(u: &mut crate::layers_drag_tests::Ui, role: egui::accesskit::Role, label: &str) -> Option<Pos2> {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let out = u.frame_output(vec![]);
    let nodes = out.platform_output.accesskit_update?.nodes;
    let n = nodes.iter().map(|(_, n)| n).find(|n| n.role() == role && n.label() == Some(label))?;
    let b = n.bounds()?;
    Some(Pos2::new(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
}

fn click(u: &mut crate::layers_drag_tests::Ui, p: Pos2) {
    u.button(p, true);
    u.button(p, false);
    u.frame(vec![]);
}

#[test]
fn every_toolbar_tool_is_picked_by_a_click_on_it_or_in_its_flyout() {
    use egui::accesskit::Role;
    let mut s = Session::new();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap();
    let mut u = crate::layers_drag_tests::Ui::new(s);
    u.app.ui.home = false;
    let mut from_flyouts = 0;
    for g in crate::toolbar::GROUPS {
        for (tool, _) in g.tools {
            u.app.run("tool.set", json!({"tool": "move"}));
            let label = crate::i18n::tr(tool.label()).to_string();
            // The group's button shows the tool last picked from it; the others are in its flyout.
            match control_at(&mut u, Role::Button, &label) {
                Some(p) => click(&mut u, p),
                None => {
                    let chevron = control_at(&mut u, Role::Button, crate::i18n::tr(g.name)).unwrap_or_else(|| panic!("no flyout button {}", g.name));
                    click(&mut u, chevron);
                    assert_eq!(u.app.ui.tool_flyout, Some(g.key), "the {} flyout opens and stays open", g.name);
                    let item = control_at(&mut u, Role::MenuItemCheckBox, &label).unwrap_or_else(|| panic!("no {label} in the {} flyout", g.name));
                    click(&mut u, item);
                    assert_eq!(u.app.ui.tool_flyout, None, "picking from the flyout closes it");
                    from_flyouts += 1;
                }
            }
            assert_eq!(u.app.session.tool.tool, *tool, "a click on {label} picks it");
        }
    }
    // Each group shows one tool at a time: the rest were picked from flyouts.
    let tools: usize = crate::toolbar::GROUPS.iter().map(|g| g.tools.len()).sum();
    assert!(from_flyouts >= tools - crate::toolbar::GROUPS.len(), "{from_flyouts} of {tools} tools came from flyouts");
}

/// A press and its release in the same frame: a touchpad tap, or any click while frames are slow.
fn tap(u: &mut crate::layers_drag_tests::Ui, p: Pos2, button: egui::PointerButton) {
    u.frame(vec![
        egui::Event::PointerMoved(p),
        egui::Event::PointerButton { pos: p, button, pressed: true, modifiers: Default::default() },
        egui::Event::PointerButton { pos: p, button, pressed: false, modifiers: Default::default() },
    ]);
    u.frame(vec![]);
}

#[test]
fn menus_opened_by_a_tap_stay_open() {
    use egui::accesskit::Role;
    let mut s = Session::new();
    s.execute("shape.rectangle", &json!({"x": 10, "y": 10, "width": 100, "height": 50})).unwrap();
    let mut u = crate::layers_drag_tests::Ui::new(s);
    u.app.ui.home = false;
    // The main menu, from the logo.
    let logo = control_at(&mut u, Role::Button, crate::i18n::tr("Main menu")).expect("the main menu button");
    tap(&mut u, logo, egui::PointerButton::Primary);
    assert!(u.app.ui.main_menu, "a tap on the logo opens the main menu and it stays open");
    // A tap elsewhere closes it.
    let away = u.app.canvas.rect.center();
    tap(&mut u, away, egui::PointerButton::Primary);
    assert!(!u.app.ui.main_menu);
    // The canvas context menu, from a two-finger tap (secondary).
    tap(&mut u, away, egui::PointerButton::Secondary);
    assert!(u.app.ui.context_menu.is_some(), "a secondary tap opens the context menu and it stays open");
    tap(&mut u, away + egui::vec2(-300.0, -200.0), egui::PointerButton::Primary);
    assert!(u.app.ui.context_menu.is_none());
}

#[test]
fn a_popup_is_just_opened_on_its_first_frame_only() {
    let ctx = egui::Context::default();
    let id = egui::Id::new("a popup");
    let mut seen = vec![];
    // Shown on frames 0–2, closed on frame 3, open again from frame 4.
    for frame in 0..6 {
        ctx.run_ui(RawInput::default(), |ui| {
            if frame != 3 {
                seen.push(crate::menus::just_opened(ui.ctx(), id));
            }
        })
        .textures_delta
        .clear();
    }
    assert_eq!(seen, vec![true, false, false, true, false]);
}
