//! The player's clock and state: pure, driven by a counter standing for the caller's time.

use crate::{MAX_STEP, Player, PlayerConfig};

const FRAME: f64 = 1.0 / 60.0;

fn player(duration: f64) -> Player {
    Player::new(duration, PlayerConfig::default())
}

/// Advance a 60 Hz clock `frames` times from `*now`, collecting what each step reports.
fn run(p: &mut Player, now: &mut f64, frames: usize, watch: &[f64]) -> (Vec<usize>, u32, u32) {
    let (mut hits, mut ended, mut looped) = (Vec::new(), 0, 0);
    for _ in 0..frames {
        *now += FRAME;
        let ev = p.advance(*now, watch);
        hits.extend(ev.hits);
        ended += u32::from(ev.ended);
        looped += u32::from(ev.looped);
    }
    (hits, ended, looped)
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn it_plays_by_the_callers_clock_and_pauses() {
    let mut p = player(10.0);
    assert!(p.playing() && !p.muted() && !p.looping() && !p.ended());
    let mut now = 100.0;
    // The first step only notes the clock.
    p.advance(now, &[]);
    assert_eq!(p.position(), 0.0);
    run(&mut p, &mut now, 60, &[]);
    assert!(near(p.position(), 1.0), "{}", p.position());
    p.pause();
    run(&mut p, &mut now, 60, &[]);
    assert!(near(p.position(), 1.0), "paused: it stays");
    // Playing again notes the clock first, so the time spent paused doesn't count.
    now += 5.0;
    p.play();
    p.advance(now, &[]);
    run(&mut p, &mut now, 30, &[]);
    assert!(near(p.position(), 1.5), "{}", p.position());
    // Toggling.
    assert!(!p.toggle_playing() && !p.playing());
    assert!(p.toggle_playing() && p.playing());
}

#[test]
fn a_stalled_clock_does_not_make_it_jump() {
    let mut p = player(100.0);
    p.advance(0.0, &[]);
    p.advance(60.0, &[]);
    assert!(near(p.position(), MAX_STEP), "a minute's stall plays one step: {}", p.position());
    // A clock that goes backwards or isn't a number moves nothing.
    p.advance(10.0, &[]);
    p.advance(f64::NAN, &[]);
    p.advance(f64::INFINITY, &[]);
    assert!(near(p.position(), MAX_STEP), "{}", p.position());
}

#[test]
fn skipping_and_seeking_stay_within_the_video() {
    let mut p = player(10.0);
    p.skip(4.0);
    assert_eq!(p.position(), 4.0);
    p.skip(100.0);
    assert_eq!(p.position(), 10.0);
    p.skip(-100.0);
    assert_eq!(p.position(), 0.0);
    p.seek(2.5);
    assert_eq!(p.position(), 2.5);
    p.skip(f64::NAN);
    p.seek(f64::NAN);
    assert_eq!(p.position(), 2.5, "not a number: nothing");
    p.skip(f64::INFINITY);
    assert_eq!(p.position(), 10.0);
    p.skip(f64::NEG_INFINITY);
    assert_eq!(p.position(), 0.0);
    p.seek(f64::MAX);
    assert_eq!(p.position(), 10.0);
}

#[test]
fn a_hit_time_fires_once_as_playing_crosses_it() {
    let mut p = player(10.0);
    let mut now = 0.0;
    // Indexes: 1 s, 2 s twice (two interactions at the same time), 0 s, 12 s (past the end), -1 s, NaN.
    let watch = [1.0, 2.0, 2.0, 0.0, 12.0, -1.0, f64::NAN];
    // (The first step only notes the clock, and reports time 0.)
    let mut hits = p.advance(now, &watch).hits;
    let (more, ended, _) = run(&mut p, &mut now, 60 * 11, &watch);
    hits.extend(more);
    // Time order; equal times in list order; the one past the end, the negative and the NaN never.
    assert_eq!(hits, [3, 0, 1, 2]);
    assert_eq!(ended, 1);
    assert_eq!(p.position(), 10.0);
}

#[test]
fn time_zero_fires_when_playing_starts_from_the_beginning() {
    let mut p = player(10.0);
    // The very first step reports it (it has "arrived" at 0).
    let ev = p.advance(0.0, &[0.0, 0.5]);
    assert_eq!(ev.hits, [0]);
    // Not again on the next steps.
    assert!(p.advance(0.1, &[0.0, 0.5]).hits.is_empty());
    // A skip back to the start and playing on fires it again.
    p.seek(0.0);
    assert_eq!(p.advance(0.2, &[0.0]).hits, [0]);
}

#[test]
fn skips_do_not_fire_what_they_jump_over_but_crossing_again_does() {
    let mut p = player(10.0);
    let mut now = 0.0;
    p.advance(now, &[]);
    p.skip(5.0);
    let (hits, ..) = run(&mut p, &mut now, 30, &[1.0, 5.25, 8.0]);
    assert_eq!(hits, [1], "1 s was jumped over; 5.25 s is crossed as playing goes on from 5 s");
    // Back before 5.25 s and across it again.
    p.seek(5.0);
    let (hits, ..) = run(&mut p, &mut now, 30, &[1.0, 5.25, 8.0]);
    assert_eq!(hits, [1], "crossed again: fires again");
    // Landing exactly on a time doesn't fire it; moving on doesn't either (it was reached by the jump).
    p.seek(8.0);
    let (hits, ..) = run(&mut p, &mut now, 10, &[8.0]);
    assert!(hits.is_empty(), "{hits:?}");
}

#[test]
fn it_ends_once_and_playing_again_starts_over() {
    let mut p = player(1.0);
    let mut now = 0.0;
    p.advance(now, &[]);
    let (hits, ended, looped) = run(&mut p, &mut now, 120, &[1.0, 0.5]);
    assert_eq!((hits, ended, looped), (vec![1, 0], 1, 0), "the end time itself fires, then it ends, once");
    assert!(p.ended() && !p.playing());
    assert_eq!(p.position(), 1.0);
    // More steps while stopped report nothing.
    assert_eq!(run(&mut p, &mut now, 30, &[1.0]), (vec![], 0, 0));
    // Play from the end: it starts over, and its times fire again.
    p.play();
    assert!(p.playing() && !p.ended() && p.position() == 0.0);
    p.advance(now, &[]);
    let (hits, ended, _) = run(&mut p, &mut now, 120, &[1.0, 0.5]);
    assert_eq!((hits, ended), (vec![1, 0], 1));
    // Seeking to the end of a playing video makes it end on the next step.
    p.play();
    p.seek(1.0);
    now += FRAME;
    let ev = p.advance(now, &[]);
    assert!(ev.ended, "{ev:?}");
}

#[test]
fn a_video_that_loops_goes_round_and_never_ends() {
    let mut p = Player::new(1.0, PlayerConfig { autoplay: true, looping: true, muted: false });
    let mut now = 0.0;
    p.advance(now, &[]);
    let watch = [0.0, 0.5, 1.0];
    // Three rounds and three quarters of a fourth.
    let (hits, ended, looped) = run(&mut p, &mut now, 60 * 3 + 45, &watch);
    assert_eq!(ended, 0, "looping videos don't end");
    assert_eq!(looped, 3);
    // Each time round: the end time, then 0 on the way round; 0.5 in each round (the first step,
    // which only noted the clock, reported nothing since it had no watch list).
    let count = |i| hits.iter().filter(|h| **h == i).count();
    assert_eq!((count(0), count(1), count(2)), (3, 4, 3), "{hits:?}");
    assert!(p.position() >= 0.0 && p.position() < 1.0);
    assert!(p.playing());
}

#[test]
fn a_step_longer_than_the_video_reports_each_time_once() {
    let mut p = Player::new(0.05, PlayerConfig { autoplay: true, looping: true, muted: false });
    p.advance(0.0, &[]);
    let ev = p.advance(0.25, &[0.01, 0.02, 0.01]);
    assert!(ev.looped);
    assert_eq!(ev.hits.len(), 3, "each index once even when the step went round five times: {ev:?}");
    assert!(p.position() >= 0.0 && p.position() < 0.05);
}

#[test]
fn an_empty_video_ends_the_moment_it_plays() {
    for d in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
        let mut p = player(d);
        let ev = p.advance(0.0, &[0.0]);
        assert!(ev.ended && ev.hits.is_empty(), "{d}: {ev:?}");
        assert_eq!((p.duration(), p.position()), (0.0, 0.0));
    }
    // A huge duration is capped.
    assert_eq!(player(f64::INFINITY).duration(), 0.0);
    assert_eq!(player(1e12).duration(), crate::MAX_DURATION);
}

#[test]
fn autoplay_off_waits_to_be_played() {
    let mut p = Player::new(5.0, PlayerConfig { autoplay: false, looping: false, muted: true });
    assert!(!p.playing() && p.muted());
    let mut now = 0.0;
    assert_eq!(run(&mut p, &mut now, 60, &[0.0]), (vec![], 0, 0));
    assert_eq!(p.position(), 0.0);
    p.play();
    p.advance(now, &[]);
    run(&mut p, &mut now, 60, &[]);
    assert!(near(p.position(), 1.0));
}

#[test]
fn mute_is_state_and_restart_goes_back_to_the_settings() {
    let mut p = Player::new(5.0, PlayerConfig { autoplay: true, looping: true, muted: false });
    assert!(p.toggle_muted() && p.muted());
    assert!(!p.toggle_muted());
    p.set_muted(true);
    p.set_looping(false);
    p.skip(3.0);
    p.pause();
    p.restart();
    // The settings it started with: playing, looping, sounding, at the beginning.
    assert!(p.playing() && p.looping() && !p.muted() && !p.ended());
    assert_eq!(p.position(), 0.0);
    let ev = p.advance(0.0, &[0.0]);
    assert_eq!(ev.hits, [0], "time 0 fires again after a restart");
}

#[test]
fn any_sequence_of_operations_keeps_the_invariants() {
    // A small deterministic generator: no operation order breaks the state.
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for round in 0..200 {
        let duration = [0.0, 0.01, 0.5, 3.0, 60.0][round % 5];
        let mut p = Player::new(duration, PlayerConfig { autoplay: next() % 2 == 0, looping: next() % 3 == 0, muted: next() % 2 == 0 });
        let watch = [0.0, 0.005, 0.25, 1.0, 59.0, 1e9, -3.0, f64::NAN];
        let mut now = 0.0;
        for _ in 0..200 {
            let r = next();
            let x = (r >> 8) as f64 / 1e12 - 5.0e3;
            match r % 11 {
                0 => p.play(),
                1 => p.pause(),
                2 => {
                    p.toggle_playing();
                }
                3 => p.seek(x % 70.0),
                4 => p.skip(x % 30.0),
                5 => p.restart(),
                6 => p.set_looping(r % 2 == 0),
                7 => p.set_muted(r % 2 == 0),
                8 => {
                    p.toggle_muted();
                }
                _ => {
                    now += (r % 100) as f64 / 400.0;
                    let ev = p.advance(now, &watch);
                    assert!(ev.hits.iter().all(|i| *i < watch.len()), "{ev:?}");
                    let mut seen = ev.hits.clone();
                    seen.sort_unstable();
                    seen.dedup();
                    assert_eq!(seen.len(), ev.hits.len(), "each index once per step: {ev:?}");
                    assert!(!(ev.ended && ev.looped), "{ev:?}");
                }
            }
            assert!(p.position().is_finite() && p.position() >= 0.0 && p.position() <= p.duration(), "{} of {}", p.position(), p.duration());
            assert!(!p.ended() || !p.playing(), "an ended video isn't playing");
        }
    }
}
