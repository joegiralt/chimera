//! Project fixtures for tests: a project that fills every record kind, and
//! a comparison that says what differs.

use alloc::boxed::Box;
use alloc::format;

use crate::block::Block;
use crate::factory::{FACTORY_LEN, factory_sound};
use crate::name::{ProjectName, SoundName};
use crate::params::{EngineType, FilterParams, OutParams};
use crate::part::PartParams;
use crate::preset::Sound;

use super::{
    Origin, PartFrom, PartId, PartSource, Project, ReplaceGuard, SlotId, TemplateCrc, part_status,
    project_crc,
};

/// NEW with every slot filled: the factory Sounds and INIT, then edited
/// and renamed copies of them. Parts from slots 0, 3 and 9 and from INIT;
/// two Parts edited after loading, one of them renamed; non-default FX and
/// mix; the name `FULL`.
pub fn full() -> (Box<Project>, TemplateCrc) {
    let (mut p, t) = Project::boxed();
    for s in SlotId::ALL
        .into_iter()
        .skip(FACTORY_LEN + EngineType::ALL.len())
    {
        let i = s.index();
        let mut sound = match i % 3 {
            0 => Sound::init(EngineType::Modal),
            _ => factory_sound(i % FACTORY_LEN).expect("a factory Sound"),
        };
        sound.name = SoundName::new(&format!("COPY {i:02}")).expect("a valid name");
        sound
            .params
            .filter
            .set(FilterParams::CUTOFF, 200.0 + 100.0 * i as f32);
        sound.params.out.set(OutParams::VOLUME, i as f32 / 40.0);
        p.pool_store(s, sound);
    }
    let loads = [
        (0, PartFrom::Slot(SlotId::ALL[0])),
        (1, PartFrom::Slot(SlotId::ALL[3])),
        (2, PartFrom::Slot(SlotId::ALL[9])),
        (3, PartFrom::Init(EngineType::Modal)),
        (5, PartFrom::Slot(SlotId::ALL[3])),
    ];
    for (part, from) in loads {
        let src = PartSource {
            part: PartId::ALL[part],
            from,
        };
        let c = ReplaceGuard::check(&p, t, src).expect("a Clean Part");
        p.replace_part(c).expect("a filled slot");
    }
    let e = p.edit_part(PartId::ALL[1]);
    e.sound.params.filter.set(FilterParams::RESONANCE, 0.6);
    e.sound.name = SoundName::new("RENAMED").expect("a valid name");
    p.edit_part(PartId::ALL[3])
        .sound
        .params
        .out
        .set(OutParams::VOLUME, 0.25);
    let mix = p.edit_part(PartId::ALL[2]).mix;
    mix.set(PartParams::PAN, -0.5);
    mix.set(PartParams::SEND_DELAY, 0.4);
    mix.set(PartParams::LEVEL, 0.6);
    let fx = p.edit_fx();
    fx.delay.mix = 0.3;
    fx.reverb.mix = 0.45;
    fx.chorus.mode = 2;
    p.set_name(ProjectName::new("FULL").expect("a valid name"));
    (p, t)
}

/// Panics with the first difference: the names, each slot (`bits_eq`),
/// each Part's Sound (`bits_eq`), mix, Origin (slot and CRC, or engine;
/// never the generation) and `part_status`, the FX, then `project_crc`.
pub fn same(a: &Project, b: &Project) {
    assert_eq!(a.meta().name(), b.meta().name(), "project name");
    for s in SlotId::ALL {
        match (a.pool().get(s), b.pool().get(s)) {
            (None, None) => {}
            (Some(x), Some(y)) => assert!(
                x.bits_eq(y),
                "slot {}: {} vs {}",
                s.index(),
                x.name.as_str(),
                y.name.as_str()
            ),
            (x, y) => panic!(
                "slot {}: filled {} vs {}",
                s.index(),
                x.is_some(),
                y.is_some()
            ),
        }
    }
    for id in PartId::ALL {
        let (x, y) = (a.part(id), b.part(id));
        let n = id.index();
        assert!(x.sound.bits_eq(&y.sound), "Part {n} sound");
        assert_eq!(x.mix, y.mix, "Part {n} mix");
        match (x.origin(), y.origin()) {
            (
                Origin::Slot {
                    slot: s1, crc: c1, ..
                },
                Origin::Slot {
                    slot: s2, crc: c2, ..
                },
            ) => assert_eq!((s1, c1), (s2, c2), "Part {n} origin"),
            (o1, o2) => assert_eq!(o1, o2, "Part {n} origin"),
        }
        assert_eq!(
            part_status(x, a.pool()),
            part_status(y, b.pool()),
            "Part {n} status"
        );
    }
    assert_eq!(a.perf().fx, b.perf().fx, "FX");
    assert_eq!(project_crc(a), project_crc(b), "project_crc");
}
