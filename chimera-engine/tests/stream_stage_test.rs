//! Where a streamed source's heads go (slice 1 § 5.6): from START − 1,
//! made even; to just past the chunk boundary after `HEAD_MS` of reach
//! (#583); the loop's from XFADE before it, holding XFADE + `HEAD_MS`
//! (#584).

use chimera_core::dsp::tape_engine::{Way, Direction, Tron};
use chimera_engine::dsp::tape_engine::heads::{Bounds, Sizing, window_end, window_start};
use chimera_engine::dsp::tape_engine::params::TapeEngineParams;
use chimera_engine::dsp::tape_engine::{Direction, TapeMachine, TapeModel};
use chimera_engine::storage::SourceRef;
use chimera_engine::stream::admit::head_frames;
use chimera_engine::stream::layout::CHUNK_FRAMES;
use chimera_engine::stream::stage::{XFADE_MAX_MS, heads_for, xfade_max};
use chimera_engine::stream::units::SourceRate;

const CF: u32 = CHUNK_FRAMES.get();
const RATE: u16 = 24_000;
const HZ: SourceRate = SourceRate::new(RATE).unwrap();
const FRAMES: u32 = 30 * RATE as u32;
const FILE: core::num::NonZeroU32 = core::num::NonZeroU32::new(FRAMES).unwrap();

fn r() -> SourceRef {
    SourceRef {
        crc: 1,
        frames: FRAMES,
        rate: RATE,
    }
}

fn play(mode: Direction, start: f32) -> TapeEngineParams {
    TapeEngineParams {
        machine: TapeMachine::play(mode),
        start,
        end: 1.0,
        ..TapeEngineParams::default()
    }
}

/// A head's last read (`end − 3`, its taps to `end − 1`) in a chunk's
/// first frames: the slot after it holds most of a chunk past the head.
fn ends_past_a_boundary(end: u32) -> bool {
    end % CF == 4
}

#[test]
fn start_head_starts_before_start_made_even() {
    for start in [0.0, 0.1, 0.25, 0.333_33, 0.5, 0.9] {
        let p = play(Direction::Forward, start);
        let h = heads_for(r(), &p).expect("ONE streams");
        assert_eq!(h.as_slice().len(), 1, "ONE: START's head alone");
        let s = window_start(start, FILE);
        let want = if s == 0 { 0 } else { (s - 1) & !1 };
        assert_eq!(h.as_slice()[0].first(), want, "START {start}");
    }
}

#[test]
fn every_head_ends_just_past_the_next_chunk_boundary() {
    let reach = head_frames(HZ);
    // Every alignment of START against the chunks, the worst included:
    // `HEAD_MS` of reach ending a frame short of a boundary.
    for k in 0..CF / 64 {
        let s = 3 * CF + 64 * k;
        let start = s as f32 / FRAMES as f32;
        let p = play(Direction::Forward, start);
        let h = heads_for(r(), &p).unwrap().as_slice()[0];
        let at = window_start(start, FILE);
        assert!(
            ends_past_a_boundary(h.end()),
            "START at {at}: ends {}",
            h.end()
        );
        assert!(h.end() >= at + reach, "START at {at}: holds HEAD_MS");
        assert!(
            h.end() < at + reach + CF + 4,
            "START at {at}: no chunk more"
        );
    }
}

#[test]
fn loop_head_holds_xfade_and_head_ms() {
    let reach = head_frames(HZ);
    for (size, place, xfade) in [
        (0.6, 0.5, 0.0),
        (0.6, 0.5, 0.05),
        (0.8, 0.7, 0.5),
        (0.3, 0.0, 0.2),
    ] {
        let mut p = play(Direction::Loop, 0.1);
        (p.size, p.position, p.xfade) = (size, place, xfade);
        let h = heads_for(r(), &p).expect("LOOP streams");
        assert_eq!(h.as_slice().len(), 2, "LOOP: START's and the loop's");
        let ws = window_start(p.start, FILE);
        let b = Bounds::of(
            ws,
            window_end(ws, p.end, FRAMES, HZ),
            Sizing { size, place, xfade },
            HZ,
        );
        let lx = b.lx().min(xfade_max(HZ));
        let early = if b.ls() - ws >= lx { lx } else { 0 };
        let lh = h.as_slice()[1];
        assert!(
            lh.first() < b.ls() - early,
            "{size} {place} {xfade}: from before the fade"
        );
        assert!(
            lh.first() + 2 >= b.ls() - early,
            "{size} {place} {xfade}: no frame more"
        );
        assert!(
            lh.end() >= b.ls() - early + lx + reach,
            "{size} {place} {xfade}"
        );
        assert!(ends_past_a_boundary(lh.end()), "{size} {place} {xfade}");
    }
    assert_eq!(xfade_max(HZ), RATE as u32 * XFADE_MAX_MS / 1_000);
}

#[test]
fn a_head_stops_at_end_or_the_file() {
    // END a little past START: the head reaches END's taps, no further.
    let mut p = play(Direction::Forward, 0.5);
    p.end = 0.5 + 500.0 / FRAMES as f32;
    let h = heads_for(r(), &p).unwrap().as_slice()[0];
    let we = window_end(window_start(p.start, FILE), p.end, FRAMES, HZ);
    assert!(
        h.end() >= we + 2 && h.end() <= we + 3,
        "to END: {} for {we}",
        h.end()
    );
    // At the file's end, the file's last frame (even).
    let h = heads_for(r(), &play(Direction::Forward, 0.999_9))
        .unwrap()
        .as_slice()[0];
    assert_eq!(h.end(), FRAMES);
}

#[test]
fn what_needs_ram_has_no_heads() {
    let mut ping = play(Direction::Ping, 0.2);
    assert!(heads_for(r(), &ping).is_err(), "PING");
    ping.set_way(Way::Loop);
    (ping.start, ping.end) = (0.6, 0.2);
    assert!(heads_for(r(), &ping).is_err(), "END before START");
    for m in [TapeModel::Scrub, TapeModel::Stretch, TapeModel::Transwave] {
        let p = TapeEngineParams {
            machine: TapeMachine::of(m),
            ..play(Direction::Forward, 0.2)
        };
        assert!(heads_for(r(), &p).is_err(), "{m:?}");
    }
}

/// TRON streams as ONE does (#698): one START head. A MODE off PLAY can't
/// be held (`TapeEngineParams`' doctest).
#[test]
fn tron_streams_on_one_start_head() {
    let p = TapeEngineParams {
        machine: TapeMachine::Play,
        direction: Direction::Forward,
        tron: Tron::On,
        ..play(Direction::Loop, 0.2)
    };
    let h = heads_for(r(), &p).expect("TRON's heads");
    let one = heads_for(r(), &play(Direction::Forward, 0.2)).unwrap();
    assert_eq!(h, one);
}
