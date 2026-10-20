//! The LFO page's playhead from the audio (ADR 0177): the watched Part's
//! newest voice's phase, a word a slot, nothing unwatched. Its own binary:
//! it plays notes through the global `LFO_VIEW` and sounding meter.

mod common;

use chimera_core::instrument::{LFO_VIEW, LfoAt};

/// The audio publishes the watched Part's newest voice's phase, a word a
/// slot, and nothing unwatched.
#[test]
fn the_audio_publishes_the_watched_parts_playhead() {
    let mut rig =
        common::inst_rig::InstRig::with_budget(chimera_core::hw::SampleBudget::for_cpu(u32::MAX));
    LFO_VIEW.watch(None);
    rig.note_on(0, 60);
    rig.render(4);
    assert_eq!(LFO_VIEW.read(0).phase, None, "unwatched: nothing");
    LFO_VIEW.watch(Some(0));
    rig.render(4);
    let a = LFO_VIEW.read(0);
    assert_eq!(a.part, 0);
    assert!(a.phase.is_some(), "a voice sounds");
    LFO_VIEW.watch(Some(1));
    rig.render(1);
    assert_eq!(
        LFO_VIEW.read(0),
        LfoAt {
            part: 1,
            phase: None
        },
        "Part 2 has none"
    );
    LFO_VIEW.watch(None);
}
