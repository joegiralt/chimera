use chimera_core::preset::{ChainType, Patch, Project, SoundPool, Track};
use chimera_core::params::Param;

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

#[test]
fn track_starts_with_init_patch() {
    let track = Track::new(ChainType::PizzaPoly);
    assert_eq!(track.patch.chain_type, ChainType::PizzaPoly);
    assert!(track.loaded_from.is_none());
}

#[test]
fn track_load_from_pool_copies() {
    let mut pool = SoundPool::new();
    let mut patch = Patch::init(ChainType::PizzaPoly);
    patch.name = *b"Acid Bass\0\0\0\0\0\0\0";
    pool.store(3, patch);

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 3);

    assert_eq!(track.patch.name_str(), "Acid Bass");
    assert_eq!(track.loaded_from, Some(3));
}

#[test]
fn track_edit_does_not_modify_pool() {
    let mut pool = SoundPool::new();
    pool.store(0, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::PizzaPoly);
    track.load_from_pool(&pool, 0);
    track.patch.params.volume = Param::new(0.0, 1.0, 0.0); // mute

    // Pool slot unchanged
    assert!(pool.get(0).unwrap().params.volume.value() > 0.0);
}

#[test]
fn track_save_to_pool_overwrites() {
    let mut pool = SoundPool::new();
    pool.store(5, Patch::init(ChainType::PizzaPoly));

    let mut track = Track::new(ChainType::Modal);
    track.patch.name = *b"My Sound\0\0\0\0\0\0\0\0";
    track.save_to_pool(&mut pool, 5);

    assert_eq!(pool.get(5).unwrap().name_str(), "My Sound");
    assert_eq!(pool.get(5).unwrap().chain_type, ChainType::Modal);
}

#[test]
fn project_has_six_tracks() {
    let project = Project::new();
    assert_eq!(project.tracks.len(), 6);
}
