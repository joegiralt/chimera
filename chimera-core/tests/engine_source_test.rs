//! One source of truth for engine choice (spec §6).

use chimera_core::params::{EngineType, ParamSnapshot};
use chimera_core::preset::{ChainType, Patch};

/// `ChainType::engine` is usable in const context.
const FM_ENGINE: EngineType = ChainType::Fm.engine();

#[test]
fn chain_type_names_its_engine() {
    assert_eq!(ChainType::PizzaPoly.engine(), EngineType::Pizza);
    assert_eq!(ChainType::Modal.engine(), EngineType::Modal);
    assert_eq!(FM_ENGINE, EngineType::Fm);
}

#[test]
fn patch_init_takes_its_engine_from_the_chain() {
    for ct in ChainType::ALL {
        assert_eq!(Patch::init(ct).params.engine(), ct.engine(), "{ct:?}");
    }
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
