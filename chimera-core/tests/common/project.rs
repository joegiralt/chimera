//! Project files as bytes: `encode` and the two-pass `decode`, and the
//! `full`/`same` fixtures.

#[allow(unused_imports)] // each test binary uses some of these
pub use super::codec_util::{decode_project as decode, encode_project as encode};
#[allow(unused_imports)]
pub use chimera_core::project::test_support::{full, same};

use chimera_core::project::{PartSource, Project, ReplaceError, ReplaceGuard, TemplateCrc};

/// A load into a Clean or Stale Part: the guard lets it through.
#[allow(dead_code)]
pub fn load(p: &mut Project, t: TemplateCrc, src: PartSource) -> Result<(), ReplaceError> {
    let c = ReplaceGuard::check(p, t, src).expect("a Clean or Stale Part");
    p.replace_part(c)
}

/// A load into an Edited Part, answered REPLACE.
#[allow(dead_code)]
pub fn load_anyway(p: &mut Project, t: TemplateCrc, src: PartSource) -> Result<(), ReplaceError> {
    let c = ReplaceGuard::check(p, t, src)
        .expect_err("an Edited Part")
        .into_pending()
        .anyway(p);
    p.replace_part(c)
}

/// A file load confirmed, REPLACE answered if it asks.
#[allow(dead_code)]
pub fn confirm_load(
    p: &Project,
    t: TemplateCrc,
    f: chimera_core::project::ProjectFile,
) -> chimera_core::project::Confirmed<chimera_core::project::ProjectSource> {
    let src = chimera_core::project::ProjectSource::File {
        id: f.id(),
        vol: f.vol(),
    };
    ReplaceGuard::check(p, t, src).unwrap_or_else(|n| n.into_pending().anyway(p))
}
