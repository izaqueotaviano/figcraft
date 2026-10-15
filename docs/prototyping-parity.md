# Prototyping parity

> **Last reviewed:** 2026-10-11 · **Last updated:** 2026-10-15 · **Change:** minor (video in prototypes: video fills that play in the presenter, the "On media hit" and "On media end" triggers and the play/pause, mute, skip and skip-to actions, with the video formats that really play; open link in the same tab: an "Open in a new tab" setting on the action, REST `openInNewTab`, same-tab navigation in the web version; which addresses a link may open) · **Target:** Figma Design (desktop app 126.10.7)

**Breadth ~94%, ready ~74% (estimated).** Every Figma trigger, action, transition and easing exists;
key/gamepad triggers take controller buttons, read from controllers on Windows and in the browser.
Media plays as far as the decoders go: Motion-JPEG video and animated GIF, WebP and PNG play, and the
H.264, HEVC, VP9 and AV1 video most files hold is refused by name
([Supported video](#supported-video)), so ready stays where it was. Every rule of the public
documentation on triggers, actions, overlays, animations, easing, scrolling and interactive
components is a test case with what it implies (`crates/engine/src/proto_behaviour_tests.rs`, 7
tests on the model, and `crates/ui-egui/src/present_behaviour_tests.rs`, 12 tests driving the
player, 21 gestures among them; the tail: `crates/engine/src/proto_tail_tests.rs`, 4 tests, and
`crates/ui-egui/src/present_tail_tests.rs`, 5 tests), and all pass. Not yet checked against
larger real prototypes. Remaining: 2–5 h (controllers off Windows, real prototype QA).

| Group | Figma | FigCraft | Missing |
|---|---|---|---|
| Triggers | On click, on drag, while hovering, while pressing, key/gamepad, mouse enter, mouse leave, mouse down, mouse up, after delay, on media hit, on media end | 12: OnClick, OnHover, OnPress, OnDrag, MouseEnter, MouseLeave, MouseDown, MouseUp, AfterDelay, KeyPress (keys, combinations and controller buttons), MediaHit, MediaEnd | controllers on macOS, Linux and FreeBSD |
| Actions | Navigate to, change to, back, scroll to, open link, open / swap / close overlay, set variable, set variable mode, conditional, play/pause/mute/seek media | 15: Navigate, Back, ScrollTo (with an offset), OpenOverlay, SwapOverlay, CloseOverlay, OpenUrl, ChangeTo, SetVariable, SetVariableMode, Conditional, MediaPlayPause, MediaMuteUnmute, MediaSkip, MediaSkipTo | — (the media they control is limited by the decoders, [Supported video](#supported-video)) |
| Transitions | Instant, dissolve, smart animate, move in/out, push, slide in/out | all 8 | — |
| Easing | Linear, ease in/out/in-and-out, back variants, gentle, quick, bouncy, slow, custom bezier, custom spring | all 13 (springs physical) | — |

## Rules from the documentation

Status: **pass** (a case in the suite holds), **fixed** (the suite found a divergence, now fixed),
**added** (the setting was missing and now exists), **silent** (the documentation doesn't say; the
case pins the current behaviour).

| Rule | Status | Evidence / notes |
|---|---|---|
| On click: press and release on the layer; a drag is not a click | pass | `triggers_fire_on_their_gesture` |
| On drag: a drag that starts on the layer; a click is not a drag | pass | same |
| On drag with a directional animation (move, push, slide): only a drag that way starts it; a drag another way can scroll instead | **fixed** | any drag started it; `on_drag_starts_only_a_drag_its_animation_goes` (exactly diagonal counts for both axes: silent) |
| On drag: the animation it starts follows the pointer while it stays down, then plays on from there | **fixed** | it played by time; `a_drag_started_transition_follows_the_pointer` |
| Mouse down / mouse up: as the button goes down / comes up | pass | `triggers_fire_on_their_gesture` |
| While pressing: shown while the button is down, reverts on release | pass | same |
| While hovering: shown while the pointer is over the layer, reverts when it leaves; mouse enter fires once and doesn't revert; mouse leave fires on leaving | pass | same |
| Key/gamepad: a key or a combination of keys, pressed anywhere on the screen | **fixed** | combinations never fired (`Shift+K` didn't match) and `K` also fired for Shift+K; the editor's recorder now records the modifiers held, and REST files' modifier key codes import as a combination (they imported as the key alone); `key_triggers_fire_for_their_key_or_combination` |
| Key/gamepad on a controller: the trigger takes a controller button in place of a key, pressed anywhere on the screen | **added** | `Gamepad A` … `Gamepad Home` (the standard layout's 17 buttons, face buttons by position) as the trigger's key, picked from the Controller menu beside the key field; the player polls controllers only while the prototype has such a trigger and the window has the focus, and plays each press once (held buttons don't repeat); Windows reads Xbox-compatible controllers, the browser its Gamepad API's standard-mapped ones; `controller_buttons_fire_key_gamepad_triggers`, `controller_buttons_play_key_gamepad_triggers` |
| A controller button is not the keyboard key of the same name; the d-pad is not the arrow keys; a keyboard modifier held makes a button press another input | silent | pinned as stated |
| REST files: controller triggers (`device` other than `KEYBOARD`) | **added** | they were skipped; their code reads as the standard layout's button index (silent: the schema doesn't say what the codes count); two buttons at once or an unknown code is skipped with a warning; `controller_triggers_import_as_their_button` |
| After delay: timed from when the screen shows | pass | `triggers_fire_on_their_gesture` |
| After delay on an overlay: timed from when the overlay opens, again each time it opens | **fixed** | timed from when the screen under it arrived (a toast opened late closed at once) and fired only the first time; `an_overlays_after_delay_counts_from_when_it_opens` |
| Nested layers with the same trigger: the top-most layer's interaction wins; a layer without that trigger lets it through to the one under it | pass | `nested_hotspots_and_click_through`, `hotspots_priority_and_click_through` |
| A layer without interactions over a hotspot | silent | it doesn't block it: clicks reach the hotspot under it |
| Hidden layers aren't hotspots; clipping frames cut their hotspots | pass | same |
| Navigate to: replaces the screen; from an overlay it closes the overlays; preserve scroll position keeps the offset (as far as the destination scrolls) | pass | `overlays_stack_swap_close_and_go_with_navigation`, `navigating_resets_scroll_unless_it_preserves_it` |
| Back: closes the top overlay first, else returns to the previous screen playing its animation backwards; restart empties the history | pass | `back_plays_the_way_in_backwards_and_restart_starts_over`, `back_and_reverting_play_the_animation_the_other_way` |
| Scroll to: every frame around the layer scrolls to bring it in, animated with the interaction's animation | pass | `scroll_to_moves_every_scrolling_frame_around_the_layer` |
| Scroll to with an offset: the layer stops that far from the top-left (room for a fixed header) | **added** | `offset: [x, y]` on the action, X / Y fields in the interaction editor; `scroll_to_brings_the_layer_to_the_top_less_its_offset` (nested scrolling frames: the innermost keeps the offset; silent) |
| Open overlay: positions, background dimming, close on a click outside (on by default, can be turned off) | pass | `a_click_outside_an_overlay_closes_it_unless_turned_off`, `overlay_positions` |
| Overlays stack; swap overlay replaces the top one; close overlay closes it | pass | `overlays_stack_swap_close_and_go_with_navigation` |
| Manual overlay position relative to the hotspot | **added** | `offset: [x, y]` on the action, from the top-left of the layer that opens it (X / Y fields in the interaction editor; without one, below that layer); `present_delay_tests` |
| Delay on mouse enter / leave / down / up | **added** | `delayMs` on the interaction, a Delay field in the editor; fires once after the delay while its screen or overlay still shows (a pointer that leaves early: silent, it still fires); `mouse_triggers_wait_out_their_delay` |
| Open link | pass | a new tab or browser window by default |
| Open link: "Open in a new tab" (the REST schema's `openInNewTab`), on by default; off opens the link in the page the prototype runs in | **added** | only the web version has a page to replace (same-tab navigation through the browser's `location`); the desktop app opens the system browser either way, and the editor's checkbox says so; files and commands without the field mean a new tab, as before; `crates/doc/src/proto_link_tests.rs`, `crates/engine/src/proto_link_tests.rs`, `crates/format/src/rest/link_tests.rs`, `crates/ui-egui/src/present_link_tests.rs` |
| Open link: which addresses open | **added** (silent) | only web (`http`, `https`), mail, phone and text-message links open, an address without a scheme as `https://…`; `javascript:`, `file:`, `data:` and the schemes other software registers don't (the viewer is told), since a file's link runs on the viewer's machine and, in the browser, a same-tab `javascript:` link would run in the app's own page |
| On media hit: the layer's video plays through the time set; once each time playing crosses it | **added** | a skip or seek doesn't fire the times it jumps over, going back and playing across a time again fires it again, a loop fires it each round, a time past the end never fires; time 0 fires when playing starts from the beginning (silent for all of these; pinned); `a_hit_time_fires_once_per_crossing_and_again_after_a_restart`, `skipping_does_not_fire_what_it_jumps_over_but_crossing_again_does` (`crates/ui-egui/src/present_media_tests.rs`), `clock_tests` (`crates/media`) |
| On media end: the layer's video plays to its end | **added** | once, however long the screen stays; a video that loops never ends (silent; pinned); `the_end_fires_once_and_a_loop_never_ends` |
| Play/pause, mute/unmute, skip forward/backward, skip to time | **added** | each takes the video layer to act on (the layer the interaction is on when none is named); toggle, or play / pause and mute / unmute set; skips and seeks stay within the video; muting is state only, since no sound is played; a layer that isn't playing a video on the screen or in an open overlay does nothing; `play_pause_toggles_and_a_video_without_autoplay_waits`, `skip_forward_backward_and_to_a_time_stay_within_the_video`, `mute_is_state_only`, `media_actions_act_on_the_layer_they_name_else_the_one_they_are_on` |
| A video plays while its screen shows; it starts again each time the screen appears (navigating to it, Back, restarting); an overlay's videos start when it opens and stop when it closes while the screen under it plays on | **added** (silent) | `a_screen_shown_again_starts_its_video_over`, `an_overlays_videos_start_when_it_opens_and_stop_when_it_closes`; editing the file while presenting keeps a video playing unless its file, autoplay, loop or muted setting changed; the outgoing screen of a transition keeps the picture it last showed, and a smart-animate transition draws videos at their poster picture until it ends |
| Video fill settings: autoplay (on by default), loop, muted, the poster picture the editor and exports show (a paused video shows it until it plays) | **added** (silent) | `video.set`, the colour picker's Video type; a video without autoplay waits for a play action |
| The window draws frames only while a video plays, and then only the video's own area | **added** | the screen is rendered once with its videos in it; each new picture renders again just the part of the screen the video covers and patches it in, so layers above, below and around it stay composited as in a full render (a video covering over half the screen renders the screen whole); paused or ended videos draw nothing; `frames_are_asked_for_only_while_a_video_plays`, `only_the_videos_area_is_redrawn_and_a_big_one_is_drawn_whole`, `rendering_just_the_area_a_video_covers_matches_the_whole_render` (`crates/render`) |
| Layers whose interactions all follow their video (media hit, media end) aren't hotspots | **added** (silent) | nothing can click them; `layers_that_only_follow_their_video_are_not_hotspots` |
| REST files: `ON_MEDIA_HIT` (`mediaHitTime`), `ON_MEDIA_END` and `UPDATE_MEDIA_RUNTIME` (`mediaAction`, `destinationId`, `amountToSkip`, `newTimestamp`) | **added** | they were skipped with a warning; an unknown action kind or a target that isn't in the file is skipped with one; times are kept within a day; the `.fig` writer has no field it can verify for them and leaves them out with a warning; `crates/format/src/rest/media_tests.rs` |
| Change to: another variant of the instance, in place, with the interaction's animation | pass | `interactive_components_change_in_place` |
| Interactive components: every instance plays the interactions of the variant it shows; an interaction on the instance with the same trigger wins | **fixed** | instances played only interactions copied onto them when they were placed: an instance placed before its main got interactions did nothing, and one placed after kept the first variant's interactions after changing (a toggle went on but never off). New instances no longer copy them, and copies in older files give way to the variant shown; `instances_play_their_variants_interactions` |
| Change to on a layer inside a nested instance changes the instance whose variant set holds the destination | **fixed** | the innermost instance (an icon inside a button) was changed instead |
| Set variable, set variable mode, conditional | pass | `proto_run_tests`, `proto_conditional_tests`, `present_variables_tests` |
| Expressions: a variable reads the mode where the interaction runs (a frame that chose a mode, else the page's); `name[Mode]` reads a given mode whatever is chosen; operators on numbers, text (`+` joins), booleans and comparisons | pass | `mode_lookups_read_a_given_mode_or_the_layers_current_one`, `proto_expr_mode_tests` |
| Set variable mode, then a conditional or set variable later in the same interaction: they see the new mode | pass | `set_variable_mode_changes_what_later_expressions_read` |
| A set variable writes every mode of the variable, so a lookup of any mode reads the new value; a frame inside the screen that chose a mode keeps it after set variable mode | silent | pinned as stated |
| Widgets in a presented prototype react to clicks on their clickable parts | **added** | the plugin is called back as on the canvas (same callback, sandbox and fuel limits), on the prototype's copy: the widget's state is the prototype's, the file and its undo history don't change, restarting starts over (silent; pinned); an interaction on the layer wins over the widget; a plugin that's gone or fails shows its error over the presenter; `widgets_react_to_clicks_on_the_prototypes_own_copy`, `widgets_react_to_clicks_in_the_presented_prototype` |
| Smart animate: layers matched by name and hierarchy tween position, size, rotation, opacity, solid fills and strokes and corner radius; unmatched layers dissolve | pass | `smart_animate_tweens_matched_layers_and_overshoots_with_the_easing`, `smart_interpolate_tweens_matched_and_fades_the_rest` |
| Smart animate with a spring or back easing overshoots (and pulls back) | **fixed** | layers snapped to their end once the curve passed 1 |
| Visibility, text content, gradients and images in smart animate | silent | they show the destination's |
| Move in/out, push, slide in/out move the way their direction names; reversed they go the other way | pass | `back_and_reverting_play_the_animation_the_other_way`, `transitions_move_the_right_way` |
| Easing curves: linear, ease in, ease out, ease in and out, back variants, custom bezier | pass | `easing_curves_and_springs` |
| Spring presets are physical springs (mass 1; stiffness / damping: gentle 100/15, quick 300/20, bouncy 600/15, slow 80/20) | **fixed** | they were ad-hoc curves |
| A spring plays until it settles: its duration follows from its mass, stiffness and damping, not from the duration field | **fixed** | it was squeezed into the duration field's time; the editor shows the spring's duration in place of the field; `springs_play_until_they_settle` |
| Scrolling: horizontal, vertical or both; momentum; fixed layers stay put; sticky layers stop at the edge | pass | `scroll_offsets_fixed_and_sticky`, `present_view_tests` (sticky on the left edge too: silent) |
| Fixed layers draw and take clicks above the content scrolling under them, wherever they are in the layer list | **fixed** | they drew in layer order, under later layers; the file's order doesn't change; `fixed_layers_stay_above_the_content_that_scrolls` |
| Flows and starting points; device frames; presentation options; hotspot hints | pass | `proto_present_tests`, `present_view_tests`, `present_tests` |

## Features

| Feature | Status | Notes |
|---|---|---|
| Connections drawn and dragged on the canvas; interaction editor | done | |
| Several actions per interaction; drag to reorder; conditional blocks with else-if/else | done | one undo step per edit |
| Expressions (`count + 1`, `mode(Theme) == "Dark"`, `accent[Dark]`) | done | checked when entered; `proto.evaluate` computes one where a layer's interaction would run (MCP and agents) |
| Flows, starting points, descriptions | done | |
| Overlays: position, close on click outside, background, animate out, stacking, timers from when they open | done | |
| Scroll behaviour: overflow directions, fixed (drawn on top), sticky, preserve scroll position, scroll to with an offset, momentum and overscroll | done | |
| Key combinations (`Shift+K`), recorded in the editor and imported from REST files | done | |
| Controller buttons on key/gamepad triggers (Controller menu, REST import, `ui.input {gamepad}` on the control channel) | done | read from controllers on Windows (Windows.Gaming.Input) and in the browser (Gamepad API); macOS, Linux and FreeBSD don't read controllers yet; button names follow one layout (no PlayStation or Switch labels) |
| Widgets react to clicks in a presented prototype | done | clicks only, like on the canvas |
| Interactive components: instances play their variant's interactions, overridden per trigger | done | |
| Drag-driven animations that follow the pointer, in their direction | done | |
| Presenter: device frames and colours, rotation, custom sizes, fit/fill/100%, responsive frames | done | device bodies drawn in code |
| Hotspot hints, inspect panel in the presenter, comments in the presenter | done | |
| Preview pane following edits live | done | |
| Edit interactions on several layers at once | done | |
| Opening a link in place (web version) | done | the interaction editor's "Open in a new tab" checkbox; the `.fig` writer has no field it can verify for it and warns |
| Video in prototypes: video fills, media triggers and actions, the player's clock | done, limited by the decoders | plays Motion-JPEG (AVI, MOV, MP4) and animated GIF, WebP and PNG, no sound; H.264, HEVC, VP9, AV1 and the like are refused by name ([Supported video](#supported-video)); the editor's colour picker has a Video type, the interaction editor the media triggers and actions |
| Controllers on macOS, Linux and FreeBSD | missing | the usual Rust controller library links the system's libudev on Linux; the pure-Rust desktop builds read none there yet |
| Prototype in a separate window; mirror to a phone | missing | see [hardware-parity](hardware-parity.md) |
| Share a prototype link | missing | needs multiplayer/sharing |

## Supported video

What plays in a prototype, and what is refused, as of 2026-10-15. The pictures come from
`crates/media` (`figcraft-media`), which reads the containers itself from their public
specifications and decodes pictures with the JPEG decoder of the `image` crate. Nothing in the
product links a C library for it.

- **Plays:** Motion-JPEG (every picture a JPEG) in **AVI** (`MJPG`), and in **MOV and MP4**
  (`jpeg`, or MPEG-4 Visual objects of the JPEG type); **animated GIF, WebP and PNG** (APNG), as videos
  when placed with `video: true` (or by the colour picker's Video type), else they stay the pictures
  that always animated by themselves. Seeking is exact (every Motion-JPEG picture stands alone).
- **Refused, by name:** every other video codec: H.264 (AVC), HEVC, VP8, VP9, AV1, ProRes,
  MPEG-4 Visual and the other codecs of AVI and MOV files, DV, Cinepak and so on; WebM and Matroska
  files, by their codec. The message says `This video codec isn't supported yet: H.264 (avc1)`, and
  the file is not kept in the document. Fragmented MP4 (`moof`) is refused too. Most video that
  people have is H.264 or HEVC in MP4/MOV or VP9 in WebM, so most files will be refused until a
  decoder exists; a Motion-JPEG copy plays (for example, with FFmpeg: `ffmpeg -i in.mp4 -c:v mjpeg
  -q:v 3 -an out.avi`; FFmpeg is a tool here, nothing is linked or shipped).
- **Sound:** not played. Files with sound open (the audio track is ignored; `video.info` and the
  colour picker say `hasAudio`); muting only changes the state that mute and unmute actions toggle.
- **Limits:** 100 MB a file, 36 million pixels (and 16,384 pixels a side) a picture, 500,000
  pictures, 24 hours; animated images at most 500 pictures and 128 MB decoded. Hostile or damaged
  files give an error or the pictures before the damage, never a panic (fuzzed: truncations,
  bit flips and random bytes on every container and the clock).
- **Where it shows:** video fills play in the presenter and the preview pane; the canvas and the
  PNG, JPG, WebP, SVG and PDF exports show the poster picture. `.fig` import skips video fills and
  `.fig` export leaves them out, with a warning. A video travels in the `.figcraft` file like an
  image (and in a live session as part of the document, within its message limit).
- **Cost:** a picture is decoded when it is first needed and a few are kept, so large pictures can
  make playback stutter on a slow machine (not measured on real HD files); a file of up to 100 MB is
  held in memory.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Video in prototypes (gap 15's media part, gap 12's playback): video fills (`crates/color` `VideoPaint`, stored by content hash with the images) and `crates/media`, a new crate that opens Motion-JPEG in AVI, MOV and MP4 and animated GIF, WebP and PNG, refuses every other codec by name, and holds a deterministic player clock (play, pause, mute, skip, loop, hit times, end). The "On media hit" and "On media end" triggers and the play/pause, mute/unmute, skip and skip-to actions exist in the model, the interaction editor, REST import (they were skipped), the commands (`image.place {video}`, `video.set`, `video.info`, `proto.interaction.*`) and the player, which plays by its own time, starts videos again with their screen, fires hits and ends once per crossing, asks for frames only while a video plays and renders again only the video's area. Breadth 92% → 94%, ready unchanged at 74% (most real video is refused until a decoder exists), 2–5 h unchanged (the media part is replaced by controllers off Windows and real prototype QA). |
| 2026-10-15 | minor | Open link in the same tab: the action takes "Open in a new tab" (on by default; files, commands and REST files without it mean a new tab, as before; REST `openInNewTab` read; the `.fig` writer has no field it can verify and warns). Off, the web version navigates the page itself; the desktop app opens the system browser either way (the editor's tooltip says so). Only web, mail, phone and text-message links open (`javascript:`, `file:`, `data:` and the like are refused with a note). Numbers unchanged. |
| 2026-10-13 | minor | Prototyping, delays on mouse triggers and manual overlay offsets (M5): mouse enter, leave, down and up wait `delayMs` before their actions run (once while waiting; dropped when the screen or overlay they were on goes away; a pointer that leaves early doesn't cancel, the documentation is silent), and a manual overlay sits at `offset: [x, y]` from the top-left of the layer that opens it (without one, right below it, as before). Delay and X/Y fields in the interaction editor, the player on its own clock, REST import (`delay`, `overlayRelativePosition`), `.fig` import and export (the export's field for mouse delays is unchecked against other software). Numbers unchanged. |
| 2026-10-12 | minor | Controller buttons on key/gamepad triggers (Controller menu, player polling on Windows and in the browser, REST import instead of skipping, control channel injection), widgets reacting to clicks in presented prototypes on the prototype's own copy, `proto.evaluate`; the mode lookup and set-variable-mode rules written as cases (they already held). Breadth 90% → 92%, ready unchanged at 74% (controllers on Windows and the browser only; no real prototype QA yet), 3–6 h → 2–5 h |
| 2026-10-11 | minor | Behaviour suite from the public documentation (7 model tests, 12 player tests with 21 gestures) and its fixes: on drag follows its animation's direction and the pointer, key combinations, overlay timers from when they open, interactive components play their variant's interactions and change the right instance, physical springs that play until they settle, smart animate overshoots with the easing, fixed layers above scrolling content; added scroll to offsets. Breadth 88% → 90%, ready 70% → 74%, 5–9 h → 3–6 h |
| 2026-10-10 | major | Created from the 2026-10-10 re-measure |
