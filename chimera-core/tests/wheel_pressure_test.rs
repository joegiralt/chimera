//! WHEEL and PRESSURE (#514): the mod wheel and aftertouch as matrix
//! sources, per Part's channel (and per key for poly pressure), glided.

mod common;
use common::{InstRig, tri};

use chimera_core::hw::{CPU_HZ_REV_V, SampleBudget};
use chimera_core::modulation::{CUTOFF, FixedCol, MAX_MOD_SOURCES, ModSource, ModState};
use chimera_core::note_queue::{ChannelMode, ChannelModeEvent, Expr, ExprEvent};
use chimera_core::{MidiChannel, MidiNote};
use chimera_midi::value::U7;

fn rig(src: ModSource) -> InstRig {
    let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
    *rig.shared.parts[0].params = tri();
    // Low, so the route has room to open it.
    rig.shared.parts[0].params.filter.cutoff = 200.0;
    let mut m = ModState::with_sources(MAX_MOD_SOURCES);
    m.set_fixed(src.index(), FixedCol::Freq, 60);
    *rig.shared.parts[0].mod_state = m;
    rig
}

fn send(rig: &mut InstRig, c: u8, expr: Expr) {
    rig.inst.expr(ExprEvent {
        channel: MidiChannel::new(c).unwrap(),
        expr,
    });
}

fn u7(v: u8) -> U7 {
    U7::new(v).unwrap()
}

fn key(n: u8) -> MidiNote {
    MidiNote::new(n).unwrap()
}

/// The voices holding a key, in voice order.
fn held(rig: &InstRig) -> Vec<usize> {
    let slots = rig.inst.allocator().slots();
    (0..slots.len()).filter(|&v| slots[v].held()).collect()
}

/// Voice `v`'s played CUTOFF each block for `blocks` blocks.
fn cutoffs(rig: &mut InstRig, v: usize, blocks: usize) -> Vec<f32> {
    (0..blocks)
        .map(|_| {
            rig.render(1);
            rig.inst.played(CUTOFF)[v]
        })
        .collect()
}

/// The cutoff climbs from `from` without a step: monotonic, its first
/// block and every block a small share of the way, and it arrives.
fn glides_up(from: f32, path: &[f32]) {
    let to = *path.last().unwrap();
    let way = to - from;
    assert!(way > 0.0, "{from} → {to}");
    let mut prev = from;
    for (i, &c) in path.iter().enumerate() {
        assert!(c >= prev, "block {i}: {prev} → {c}");
        assert!(
            c - prev < 0.15 * way,
            "block {i} steps {} of {way}",
            c - prev
        );
        prev = c;
    }
    let settled = path[path.len() - 2];
    assert!(
        (to - settled).abs() < 1e-3 * way,
        "arrived: {settled} → {to}"
    );
}

#[test]
fn the_wheel_moves_a_routed_cutoff_smoothly() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(0, 60);
    rig.render(4);
    let v = held(&rig)[0];
    let base = rig.inst.played(CUTOFF)[v];
    send(&mut rig, 0, Expr::Wheel(u7(127)));
    glides_up(base, &cutoffs(&mut rig, v, 400));
}

#[test]
fn channel_pressure_moves_a_routed_cutoff_smoothly() {
    let mut rig = rig(ModSource::Press);
    rig.note_on(0, 60);
    rig.render(4);
    let v = held(&rig)[0];
    let base = rig.inst.played(CUTOFF)[v];
    send(&mut rig, 0, Expr::Pressure(u7(127)));
    glides_up(base, &cutoffs(&mut rig, v, 400));
}

/// Poly pressure moves only its own key's voice; a voice reads its own or
/// the channel's, the higher.
#[test]
fn poly_pressure_is_per_key_and_the_channel_is_the_floor() {
    let mut rig = rig(ModSource::Press);
    rig.note_on(0, 60);
    rig.note_on(0, 64);
    rig.render(4);
    let vs = held(&rig);
    assert_eq!(vs.len(), 2);
    let base = rig.inst.played(CUTOFF);
    send(&mut rig, 0, Expr::Poly(key(64), u7(127)));
    rig.render(400);
    let full = rig.inst.played(CUTOFF);
    let (lo, hi) = if full[vs[0]] > full[vs[1]] {
        (vs[1], vs[0])
    } else {
        (vs[0], vs[1])
    };
    assert_eq!(full[lo], base[lo], "key 60 hears none of it");
    assert!(full[hi] > base[hi]);
    // Half the channel's: key 60 rises to it, key 64 keeps its own.
    send(&mut rig, 0, Expr::Pressure(u7(64)));
    rig.render(400);
    let both = rig.inst.played(CUTOFF);
    assert!(both[lo] > base[lo] && both[lo] < full[hi]);
    assert_eq!(both[hi], full[hi]);
}

#[test]
fn only_the_parts_on_its_channel_hear_it() {
    let mut rig = rig(ModSource::Wheel);
    rig.note_on(0, 60);
    rig.render(4);
    let v = held(&rig)[0];
    let base = rig.inst.played(CUTOFF)[v];
    send(&mut rig, 1, Expr::Wheel(u7(127)));
    send(&mut rig, 1, Expr::Pressure(u7(127)));
    send(&mut rig, 1, Expr::Poly(key(60), u7(127)));
    rig.render(100);
    assert_eq!(rig.inst.played(CUTOFF)[v], base);
}

/// CC 121 zeroes the wheel and pressure: the cutoff glides back down.
#[test]
fn reset_all_controllers_glides_them_home() {
    for src in [ModSource::Wheel, ModSource::Press] {
        let mut rig = rig(src);
        rig.note_on(0, 60);
        rig.render(4);
        let v = held(&rig)[0];
        let base = rig.inst.played(CUTOFF)[v];
        send(&mut rig, 0, Expr::Wheel(u7(127)));
        send(&mut rig, 0, Expr::Pressure(u7(127)));
        send(&mut rig, 0, Expr::Poly(key(60), u7(127)));
        rig.render(400);
        let up = rig.inst.played(CUTOFF)[v];
        assert!(up > base, "{src:?}");
        rig.inst.channel_mode(ChannelModeEvent {
            channel: MidiChannel::new(0).unwrap(),
            mode: ChannelMode::ResetControllers,
        });
        rig.render(1);
        let first = rig.inst.played(CUTOFF)[v];
        assert!(first < up && first > base, "{src:?} glides, {first}");
        rig.render(400);
        assert_eq!(rig.inst.played(CUTOFF)[v], base, "{src:?}");
    }
}

/// Unrouted, the wheel and pressure change nothing: the render is bit for
/// bit the one without them.
#[test]
fn unrouted_they_change_nothing() {
    let play = |moved: bool| {
        let mut rig = InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V));
        *rig.shared.parts[0].params = tri();
        rig.note_on(0, 60);
        let mut out = Vec::new();
        for i in 0..64 {
            if moved && i == 8 {
                send(&mut rig, 0, Expr::Wheel(u7(127)));
                send(&mut rig, 0, Expr::Pressure(u7(100)));
                send(&mut rig, 0, Expr::Poly(key(60), u7(90)));
            }
            rig.render(1);
            out.extend_from_slice(&rig.out()[0]);
        }
        out
    };
    let (still, moved) = (play(false), play(true));
    assert!(still.iter().any(|&x| x != 0.0));
    assert!(
        still
            .iter()
            .zip(&moved)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
}

/// A releasing voice keeps its last value: poly pressure on its key (a
/// re-strike's, a stray message) moves only held voices.
#[test]
fn poly_pressure_skips_a_releasing_voice() {
    let mut rig = rig(ModSource::Press);
    rig.shared.parts[0].params.envelopes[0].release = 2.0;
    rig.note_on(0, 60);
    rig.render(4);
    let v = held(&rig)[0];
    rig.note_off(0, 60);
    rig.render(2);
    assert!(
        !rig.inst.allocator().slots()[v].is_free(),
        "still releasing"
    );
    let base = rig.inst.played(CUTOFF)[v];
    send(&mut rig, 0, Expr::Poly(key(60), u7(127)));
    rig.render(40);
    assert_eq!(rig.inst.played(CUTOFF)[v], base);
}
