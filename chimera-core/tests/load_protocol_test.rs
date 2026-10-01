//! Loading while playing (projects spec § Loading while playing, ADR
//! 0046): a load epoch fades every voice through the old snapshot, the
//! audio acks, and the gate reopens on the snapshot tagged with the epoch.

mod common;

use chimera_core::dsp::engines::SlotKind;
use chimera_core::dsp::fx_bus::FxBus;
use chimera_core::dsp::modal::ResonatorMode;
use chimera_core::dsp::voice::Voice;
use chimera_core::hw::{CPU_HZ_REV_V, MAX_PARTS, MAX_VOICES, SampleBudget};
use chimera_core::instrument::{AudioShared, DacOut};
use chimera_core::note_queue::{
    NoteDrain, NoteEvent, NoteKind, NoteProducer, NoteSources, SourceId,
};
use chimera_core::params::{EngineType, ParamSnapshot, Steal};
use chimera_core::part::DacPair;
use chimera_core::preset::{Performance, Sound};
use chimera_core::project::{GateStep, LoadGate, LoadLink, PartId, Settled};
use chimera_core::{MidiChannel, MidiNote, Velocity};
use common::{InstRig, peak};

fn rev_v() -> InstRig {
    InstRig::with_budget(SampleBudget::for_cpu(CPU_HZ_REV_V))
}

/// Every Part's bus silent this block.
fn silent(r: &InstRig) -> bool {
    (0..MAX_PARTS).all(|p| peak(r.inst.part_bus(p)) == 0.0)
}

#[test]
fn gate_table() {
    let mut g = LoadGate::new();
    let s = |kill, drain, ack| GateStep { kill, drain, ack };
    assert_eq!(g.step(0, 0, true), s(false, true, None));
    assert_eq!(g.step(1, 0, false), s(true, false, None));
    assert_eq!(g.step(1, 0, false), s(false, false, None));
    assert_eq!(g.step(1, 0, true), s(false, false, Some(1)));
    assert_eq!(g.step(1, 0, true), s(false, false, None)); // acked once
    assert_eq!(g.step(1, 1, true), s(false, true, None));
    assert_eq!(g.step(1, 1, true), s(false, true, None));
}

/// A snapshot that already carries the new epoch (the UI timed out, or
/// boot published with no fade) kills and reopens in one step.
#[test]
fn an_epoch_already_published_kills_and_drains() {
    let mut g = LoadGate::new();
    let s = |kill, drain, ack| GateStep { kill, drain, ack };
    assert_eq!(g.step(1, 1, true), s(true, true, None));
    assert_eq!(g.step(1, 1, true), s(false, true, None));
}

#[test]
fn second_epoch_restarts_the_fade() {
    // Review Focus 5
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert!(g.step(2, 0, false).kill);
    assert_eq!(g.step(2, 0, true).ack, Some(2));
    assert!(g.step(2, 2, true).drain);
}

/// A second epoch after the first was acked, before its publish.
#[test]
fn second_epoch_while_waiting_restarts_the_fade() {
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert_eq!(g.step(1, 0, true).ack, Some(1));
    let s = g.step(2, 0, true);
    assert!(s.kill && !s.drain && s.ack.is_none());
    assert!(!g.step(2, 1, true).drain, "epoch 1's snapshot is stale");
    assert_eq!(g.step(2, 1, true).ack, None, "acked once");
    assert!(g.step(2, 2, true).drain);
}

#[test]
fn timeout_publish_reopens() {
    // Review Focus 5
    let mut g = LoadGate::new();
    let _ = g.step(1, 0, false);
    assert!(g.step(1, 1, false).drain); // published before the ack
    assert!(g.step(1, 1, false).drain, "and stays open");
}

#[test]
fn settle_times_out() {
    let link = LoadLink::new();
    let swap = link.bump_for_test();
    let mut n = 0;
    assert_eq!(
        swap.settle(&link, || {
            n += 1;
            n < 5
        }),
        Settled::TimedOut
    );
    assert_eq!(n, 5, "it spun until `within` said stop");
    let swap = link.bump_for_test();
    link.ack(link.epoch());
    assert_eq!(swap.settle(&link, || true), Settled::Acked);
}

/// An ack for an epoch the UI has moved past settles nothing.
#[test]
fn a_late_ack_does_not_settle_the_next_epoch() {
    let link = LoadLink::new();
    let _ = link.bump_for_test();
    let first = link.epoch();
    let swap = link.bump_for_test();
    link.ack(first);
    assert!(link.acked(first) && !link.acked(link.epoch()));
    assert_eq!(swap.settle(&link, || false), Settled::TimedOut);
}

#[test]
fn update_from_tags_the_snapshot() {
    let mut r = rev_v();
    assert_eq!(r.shared.epoch, 0);
    r.shared.update_from(&Performance::new(), 7);
    assert_eq!(r.shared.epoch, 7);
}

#[test]
fn kill_all_fades_and_frees() {
    let mut r = rev_v();
    r.note_on(0, 60);
    r.note_on(0, 64);
    r.render(4);
    assert!(!r.inst.quiet() && !silent(&r));
    r.inst.kill_all();
    assert!(
        r.inst
            .allocator()
            .slots()
            .iter()
            .all(|s| s.is_free() || s.dying()),
        "every booked slot dying"
    );
    r.render(2); // FADE = 128 = 2 × 64
    assert!(r.inst.quiet());
    assert!(r.inst.allocator().slots().iter().all(|s| s.is_free()));
    r.note_off(0, 60);
    r.render(1);
    assert!(r.inst.quiet() && silent(&r));
}

/// The fade is the 128-sample one: the killed block falls, the next ends
/// at silence, with no step.
#[test]
fn the_kill_fades_over_two_blocks() {
    let mut r = rev_v();
    r.note_on(0, 60);
    r.render(8);
    let before = peak(r.inst.part_bus(0));
    r.inst.kill_all();
    r.render(1);
    let first = *r.inst.part_bus(0);
    assert!(peak(&first) > 0.0 && peak(&first) <= before);
    assert!(peak(&first[48..]) < peak(&first[..16]), "falling");
    r.render(1);
    assert!(r.inst.part_bus(0)[63].abs() < 1e-3 * before.max(1e-6) + 1e-6);
    r.render(1);
    assert!(silent(&r));
}

/// Two Modal Parts: Part 1 a released string ringing free (note-off never
/// damps it, ADR 0062), Part 2 at STEAL = GLIDE holding a note (ADR
/// 0065). Quiet within two blocks, and nothing glides or strikes after.
#[test]
fn kill_all_quiets_a_free_ringing_string_and_a_glide_part() {
    let mut r = rev_v();
    let string = ParamSnapshot::for_engine(EngineType::Modal);
    assert_eq!(string.modal.mode, ResonatorMode::String);
    let mut glide = string.clone();
    glide.pitch.steal = Steal::Glide;
    r.shared.parts[0].params = string;
    r.shared.parts[1].params = glide;
    r.note_on(0, 48);
    r.note_on(1, 55);
    r.render(10);
    r.note_off(0, 48);
    r.render(10);
    assert_eq!(r.inst.sounding(), 2, "the released string rings on");
    r.inst.kill_all();
    r.render(2);
    assert!(r.inst.quiet());
    assert!(r.inst.allocator().slots().iter().all(|s| s.is_free()));
    // The held key's note-off finds nothing, and nothing comes back.
    r.note_off(1, 55);
    for _ in 0..50 {
        r.render(1);
        assert!(silent(&r) && r.inst.quiet());
    }
    // A new note on the glide Part starts fresh: no glide from the killed
    // ring, and it sounds.
    r.note_on(1, 60);
    for _ in 0..20 {
        r.render(1);
        assert!(r.inst.slides().iter().all(|&s| s == 1.0), "no glide");
    }
    assert!(peak(r.inst.part_bus(1)) > 0.0);
}

/// Room for one voice of `params` beside the whole FX bus.
fn one_voice(params: &ParamSnapshot) -> InstRig {
    let r = rev_v();
    let cost = Voice::cost(params, &r.shared.parts[0].mod_state).0;
    let hz = ((FxBus::COST.0 + cost) as u64 * 480_000).div_ceil(7) as u32;
    InstRig::with_budget(SampleBudget::for_cpu(hz))
}

/// The UI timed out and published mid-fade: a note on the glide Part
/// takes the dying voice, waits out its fade and starts fresh, never
/// gliding from the killed note nor re-striking it.
#[test]
fn a_note_mid_fade_on_a_glide_part_starts_fresh() {
    let mut glide = ParamSnapshot::for_engine(EngineType::Modal);
    glide.pitch.steal = Steal::Glide;
    let mut r = one_voice(&glide);
    r.shared.parts[0].params = glide;
    r.note_on(0, 48);
    r.render(10);
    r.inst.kill_all();
    r.render(1);
    r.note_on(0, 55);
    assert_eq!(r.inst.allocator().refused(), 0, "it took the dying voice");
    for _ in 0..20 {
        r.render(1);
        assert!(r.inst.slides().iter().all(|&s| s == 1.0), "no glide");
    }
    assert_eq!(r.inst.sounding(), 1);
    let s = r.inst.allocator().slots().iter().find(|s| !s.is_free());
    assert_eq!(s.and_then(|s| s.note()).map(|n| n.get()), Some(55));
}

/// A note waiting on another Part's fade is dropped by the kill, and not
/// counted as refused: nothing was refused.
#[test]
fn kill_all_drops_a_waiting_note_uncounted() {
    let algo = ParamSnapshot::for_engine(EngineType::Algo);
    let mut r = one_voice(&algo);
    r.note_on(0, 60);
    r.render(4);
    r.note_on(1, 64); // steals Part 1's voice, waits for its fade
    r.inst.kill_all();
    assert_eq!(r.inst.allocator().refused(), 0);
    r.render(2);
    assert!(r.inst.quiet());
    for _ in 0..10 {
        r.render(1);
        assert!(silent(&r), "the waiting note never starts");
    }
    assert_eq!(r.inst.allocator().refused(), 0);
}

/// The project playing: Part 1 on ALGO INIT at LEVEL 0.8, out on pair 1.
fn old_project() -> Performance {
    let p = Performance::new();
    let part = &p.parts()[0];
    assert_eq!(part.sound.params.engine(), EngineType::Algo);
    assert_eq!((part.mix.level, part.mix.output), (0.8, DacPair::P1));
    p
}

/// The project loaded: Part 1 on MODAL INIT at LEVEL 0.2, out on pair 2.
/// The pair switches at once, so it tells which mix a block went through.
fn new_project() -> Performance {
    let mut p = Performance::new();
    let e = p.edit(PartId::ALL[0]);
    *e.sound = Sound::init(EngineType::Modal);
    e.mix.level = 0.2;
    e.mix.output = DacPair::P2;
    p
}

/// Each pair's summed |sample| over a block out.
fn energy(out: &DacOut) -> [f32; 3] {
    out.map(|pair| pair.iter().map(|x| x.abs()).sum())
}

/// A rig on the old project, fed through a real note queue, with the link
/// and the gate between them: a shell in miniature.
struct Load {
    link: LoadLink,
    gate: LoadGate,
    r: InstRig,
    keys: NoteProducer<'static>,
    drain: NoteDrain<'static, 1>,
}

impl Load {
    fn new() -> Self {
        let sources: &'static NoteSources<1> = Box::leak(Box::new(NoteSources::new()));
        let (mut producers, drain) = sources.split().unwrap();
        let mut r = rev_v();
        *r.shared = AudioShared::from_performance(&old_project());
        Self {
            link: LoadLink::new(),
            gate: LoadGate::new(),
            r,
            keys: producers.take(SourceId::new(0)).unwrap(),
            drain,
        }
    }

    fn key(&mut self, note: u8) {
        assert!(self.keys.push(NoteEvent {
            channel: MidiChannel::new(0).unwrap(),
            note: MidiNote::new(note).unwrap(),
            kind: NoteKind::On(Velocity::DEFAULT),
        }));
    }

    /// One callback: the gate, the drain only when it says so, a block.
    /// Returns whether it drained, and the block out.
    fn callback(&mut self) -> (bool, DacOut) {
        let Self { link, gate, r, .. } = self;
        let drain = gate.before_block(link, &mut r.inst, &r.shared);
        if drain {
            let (inst, shared) = (&mut r.inst, &r.shared);
            self.drain.drain(|ev| inst.handle(ev, shared));
        }
        r.render(1);
        (drain, *r.out())
    }

    fn publish(&mut self) {
        self.r.shared.update_from(&new_project(), self.link.epoch());
    }

    /// Part 1 sounds a note of the new project: a Modal voice, on pair 2.
    fn plays_new(&mut self) -> bool {
        let mut on_p2 = false;
        for _ in 0..3 {
            let (_, out) = self.callback();
            on_p2 |= energy(&out)[1] > 0.0;
        }
        let kinds = self.r.inst.slot_kinds();
        let modal = (0..MAX_VOICES).any(|v| {
            !self.r.inst.allocator().slots()[v].is_free()
                && kinds[v] == SlotKind::Modal(ResonatorMode::String)
        });
        modal && on_p2
    }
}

/// The old project's note faded by a bare `kill_all` after `held` blocks:
/// each block out, from the kill on.
fn reference_fade(held: usize, blocks: usize) -> Vec<DacOut> {
    let mut r = rev_v();
    *r.shared = AudioShared::from_performance(&old_project());
    r.note_on(0, 60);
    r.render(held);
    r.inst.kill_all();
    (0..blocks)
        .map(|_| {
            r.render(1);
            *r.out()
        })
        .collect()
}

/// The whole protocol, acked in time: the fade runs through the old
/// snapshot (its Sound, LEVEL and pair) bit for bit; a note played during
/// the load waits in the queue and plays on the new project.
#[test]
fn a_load_fades_through_the_old_mix_acks_and_reopens() {
    let mut l = Load::new();
    l.key(60);
    for _ in 0..4 {
        assert!(l.callback().0);
    }
    let swap = l.link.bump_for_test();
    let reference = reference_fade(4, 3);
    let (drained, out) = l.callback();
    assert!(!drained, "the kill");
    l.key(64); // queued behind the gate
    assert!(energy(&out)[0] > 0.0);
    assert_eq!(out, reference[0], "the old mix");
    let (drained, out) = l.callback();
    assert!(!drained, "fading");
    assert_eq!(out, reference[1], "the old mix");
    assert_eq!(energy(&out)[1], 0.0, "nothing on the new pair");
    assert!(l.r.inst.quiet());
    let (drained, out) = l.callback();
    assert!(!drained, "the ack");
    assert_eq!(out, reference[2]);
    assert!(l.link.acked(l.link.epoch()));
    assert_eq!(swap.settle(&l.link, || false), Settled::Acked);
    for _ in 0..3 {
        assert!(!l.callback().0, "held until the publish");
        assert!(l.r.inst.allocator().slots().iter().all(|s| s.is_free()));
    }
    l.publish();
    assert!(l.plays_new(), "the queued note plays on the new project");
}

/// Review Focus 5: the callback stalls past the timeout; the UI publishes
/// anyway. The gate reopens on that snapshot mid-fade, and the rest of the
/// fade goes through the new mix (pair 2), never leaving the synth muted.
#[test]
fn a_timed_out_load_still_reopens_the_audio() {
    let mut l = Load::new();
    l.key(60);
    for _ in 0..4 {
        assert!(l.callback().0);
    }
    let swap = l.link.bump_for_test();
    // The audio has not run since the bump: no ack.
    assert_eq!(swap.settle(&l.link, || false), Settled::TimedOut);
    l.publish();
    // The output stage runs a block behind: three blocks hold the fade.
    let (mut got, mut old) = ([0.0f32; 3], [0.0f32; 3]);
    for (i, reference) in reference_fade(4, 3).iter().enumerate() {
        let (drained, out) = l.callback();
        assert!(drained, "kill and drain in one, then open");
        if i == 0 {
            assert!(!l.r.inst.quiet(), "still fading");
        }
        for k in 0..3 {
            got[k] += energy(&out)[k];
            old[k] += energy(reference)[k];
        }
    }
    assert!(got[1] > 0.0, "the fade through the new mix");
    assert!(got[0] < old[0], "not wholly the old");
    for _ in 0..2 {
        assert!(l.callback().0);
    }
    assert!(l.r.inst.quiet());
    assert!(
        !l.link.acked(l.link.epoch()),
        "no ack is owed once published"
    );
    l.key(64);
    assert!(l.plays_new(), "never left muted");
}

/// Review Focus 5: a second epoch mid-fade, its predecessor never acked
/// nor published: the fade carries on, the second is acked, and its
/// publish reopens the gate.
#[test]
fn a_second_epoch_mid_fade_still_reopens_the_audio() {
    let mut l = Load::new();
    l.key(60);
    for _ in 0..4 {
        assert!(l.callback().0);
    }
    let first = l.link.bump_for_test();
    assert!(!l.callback().0, "the first kill");
    assert!(!l.r.inst.quiet(), "mid-fade");
    let second = l.link.bump_for_test();
    assert_eq!(first.settle(&l.link, || false), Settled::TimedOut);
    l.key(64);
    let mut acked = false;
    for _ in 0..4 {
        let (drained, out) = l.callback();
        assert!(!drained);
        assert_eq!(energy(&out)[1], 0.0, "the old mix to the end");
        acked |= l.link.acked(l.link.epoch());
    }
    assert!(acked && l.r.inst.quiet());
    assert_eq!(second.settle(&l.link, || false), Settled::Acked);
    l.publish();
    assert!(l.plays_new(), "never left muted");
}
