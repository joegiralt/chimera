use chimera_core::preset::{ChainType, Patch, SoundPool};

#[test]
fn patch_init_has_musically_useful_defaults() {
    let p = Patch::init(ChainType::PizzaPoly);
    assert_eq!(p.chain_type, ChainType::PizzaPoly);
    assert!(p.params.volume.value() > 0.0);
    assert!(p.params.filter.cutoff.value() > 1000.0);
    assert!(p.name_str().starts_with("(init)"));
}

#[test]
fn sound_pool_starts_empty() {
    let pool = SoundPool::new();
    assert!(pool.get(0).is_none());
    assert!(pool.get(31).is_none());
}

#[test]
fn sound_pool_store_and_retrieve() {
    let mut pool = SoundPool::new();
    let patch = Patch::init(ChainType::PizzaPoly);
    pool.store(0, patch);
    assert!(pool.get(0).is_some());
    assert_eq!(pool.get(0).unwrap().chain_type, ChainType::PizzaPoly);
}

#[test]
fn sound_pool_slot_count() {
    let pool = SoundPool::new();
    assert_eq!(pool.slot_count(), 32);
}
