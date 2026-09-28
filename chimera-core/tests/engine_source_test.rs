//! One source of truth for engine choice (spec §6).

use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::Sound;

#[test]
fn sound_init_plays_its_engine() {
    for e in EngineType::ALL {
        assert_eq!(Sound::init(e).engine(), e, "{e:?}");
    }
}

/// A Sound holds its engine once, in its params: replacing them replaces
/// the engine the UI reads too (#105).
#[test]
fn a_sounds_engine_is_its_params_engine() {
    let mut s = Sound::init(EngineType::Algo);
    s.params = ParamSnapshot::for_engine(EngineType::Modal);
    assert_eq!(s.engine(), EngineType::Modal);
}

#[test]
fn for_engine_is_the_defaults_with_that_engine() {
    for e in EngineType::ALL {
        let p = ParamSnapshot::for_engine(e);
        assert_eq!(p.engine(), e);
        assert_eq!(p.filter.cutoff, ParamSnapshot::default().filter.cutoff);
        assert_eq!(p.out.volume, ParamSnapshot::default().out.volume);
    }
}
