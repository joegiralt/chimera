// Copyright (C) 2026 Joe Giralt
// SPDX-License-Identifier: GPL-3.0-or-later

//! The SOURCE cell's card work and its turns (ADR 0175): bank 2's with
//! `bank2` (`bank2.x`'s `source_cell4card` glob), reached through the UI's
//! table (`bank2::UiDoor`); `SourceCell`'s wrappers are bank 1's.

use super::*;

/// The rows SOURCE stops on, in order: `rows` less the divider.
#[inline(never)]
fn stops(pool: &Pool, card: &[(Stem, u32)]) -> ([SourceRow; MAX_SOURCES + MAX_TAPES], usize) {
    let mut out = [SourceRow::Divider; MAX_SOURCES + MAX_TAPES];
    let mut n = 0;
    for r in rows(pool, card).filter(|r| *r != SourceRow::Divider) {
        out[n] = r;
        n += 1;
    }
    (out, n)
}

/// `heads`, folded to a word (FNV-1a).
#[cfg(feature = "tape")]
fn heads_key(heads: &Heads) -> HeadsKey {
    let words = heads.as_slice().iter().flat_map(|h| [h.first(), h.end()]);
    let k = words.fold(0x811C_9DC5, |k, w| (k ^ w).wrapping_mul(0x0100_0193));
    HeadsKey(NonZeroU32::MIN | k)
}

/// The first streamed source a Part's START, loop or XFADE moved off its
/// heads that `ready` lets re-stage, and the heads it wants. A source two
/// Parts play takes its first Part's heads (#599); SOURCE names that Part
/// on the other's band (`view::heads_of`).
#[cfg(feature = "tape")]
fn moved_heads(p: &Project, ready: impl Fn(SourceId, &Heads) -> bool) -> Option<(SourceId, Heads)> {
    let pool = p.pool();
    let mut seen = crate::project::SourceSet::EMPTY;
    PartId::ALL.into_iter().find_map(|part| {
        let sound = &p.part(part).sound;
        let r = sound
            .source_ref()
            .filter(|_| sound.engine() == crate::params::EngineType::Tape)?;
        let id = pool.find_source(r)?;
        let first = !seen.contains(id);
        seen = seen.with(id);
        let staged = head_table(pool.held(r)?)?;
        let want = heads_for(r, &sound.params.tape).ok()?;
        let moved =
            first && !staged.is_empty() && !staged.iter().eq(want.as_slice().iter().copied());
        (moved && ready(id, &want)).then_some((id, want))
    })
}

impl SourceCell {
    fn size(&self, stem: Stem) -> Option<u32> {
        let files = self.card.as_ref()?.files();
        files.iter().find(|f| f.0 == stem).map(|f| f.1)
    }

    /// SOURCE turned `delta` detents on `part`: a pool row plays at once;
    /// a card row is only shown, dimmed, until MIX + PLUS loads it.
    pub(crate) fn turn_in(&mut self, p: &mut Project, part: PartId, delta: i8) {
        let Some(card) = self.card else {
            self.job = Some(SourceJob::List(part, delta));
            return;
        };
        let playing = p.part(part).sound.source_ref();
        let on = |row: &SourceRow| match (*row, self.at) {
            (SourceRow::Card { stem, .. }, Some((q, at, _))) => q == part && stem == at,
            (SourceRow::Pool(s), _) => {
                self.at.is_none_or(|a| a.0 != part) && held(p.pool(), s) == playing
            }
            _ => false,
        };
        let (steps, n) = stops(p.pool(), card.files());
        let steps = &steps[..n];
        let n = n as i32;
        let from = steps
            .iter()
            .position(on)
            .map_or(if delta > 0 { -1 } else { n }, |i| i as i32);
        let to = (from + i32::from(delta)).min(n - 1).max(0);
        let Some(&row) = steps.get(to as usize) else {
            return;
        };
        match row {
            SourceRow::Pool(s) => {
                self.at = None;
                if let Some(r) = held(p.pool(), s) {
                    self.pick(p, part, r);
                }
            }
            SourceRow::Card { stem, .. } => {
                let size = self.size(stem).unwrap_or_default();
                self.at = Some((part, stem, size));
                if self.heads.get(stem).is_none() {
                    self.job = Some(SourceJob::Head {
                        part,
                        stem,
                        size,
                        to: HeadFor::Name,
                    });
                }
            }
            SourceRow::Divider => {}
        }
    }

    /// `part` plays pool source `r`: the memory turned, the head's ROOT
    /// and PERIOD copied, read first if it isn't cached.
    fn pick(&mut self, p: &mut Project, part: PartId, r: SourceRef) {
        if p.part(part).sound.source_ref() == Some(r) {
            return;
        }
        let head = self.heads.get(r.stem());
        self.memo.turn(p.edit_part(part).sound, r, head.as_ref());
        if let (None, Some(size)) = (head, self.size(r.stem())) {
            self.job = Some(SourceJob::Head {
                part,
                stem: r.stem(),
                size,
                to: HeadFor::Pick,
            });
        }
    }

    /// MIX + PLUS on SOURCE: loads the card row it's on, if it fits:
    /// whole, or too big for the arena as its heads (spec § 5.6), or says
    /// why the Part needs it whole.
    pub(crate) fn load_in(&mut self, p: &mut Project, part: PartId) {
        let Some((_, stem, size)) = self.at.filter(|a| a.0 == part) else {
            return;
        };
        match self.kind_in(p, part, stem) {
            #[cfg(feature = "tape")]
            Some((head, LoadKind::Streamed(heads))) => {
                if let Some(l) = StreamLoad::start(p, head, heads) {
                    self.job = Some(SourceJob::Stream(part, l));
                }
            }
            Some((_, LoadKind::Refused(why))) => self.note = Some(SourceNote::Refused(why)),
            _ => {
                if let Some(l) = SourceLoad::start(p, stem, size) {
                    self.job = Some(SourceJob::Load(part, l));
                }
            }
        }
    }

    /// How card row `stem` loads for `part`, its head once read, with the
    /// START and END the pick will give it (`SourceMemo::turn`). Whole
    /// only without TAPE.
    pub(crate) fn kind_in(
        &self,
        p: &Project,
        part: PartId,
        stem: Stem,
    ) -> Option<(SourceHead, LoadKind)> {
        if !cfg!(feature = "tape") {
            return None;
        }
        let head = self.heads.get(stem)?;
        let sound = &p.part(part).sound;
        let mut t: TapeEngineParams = sound.params.tape;
        if sound.source_ref() != Some(head.sref) {
            (t.start, t.end) = self.memo.get(stem).map_or((0.0, 1.0), |m| (m.start, m.end));
        }
        Some((head, load_kind(&head, p.pool().free(), &t)))
    }

    #[cfg(feature = "tape")]
    pub(crate) fn rehead_in(&mut self, p: &mut Project) {
        if self.job.is_some() {
            return;
        }
        // Neither a source that can't re-stage yet (a note holds it, and
        // nothing is free beside it: #598) nor one stuck on these heads
        // keeps the others waiting.
        let pool = p.pool();
        let ready = |id: SourceId, h: &Heads| {
            pool.can_rehead(id) && self.stuck[id.index()] != Some(heads_key(h))
        };
        let Some((id, heads)) = moved_heads(p, ready) else {
            self.wait = None;
            return;
        };
        let w = match self.wait {
            Some(w) if w.id == id && w.heads == heads => w,
            _ => Wait {
                id,
                heads,
                frames: UiFrames(0),
            },
        };
        let frames = w.frames.next();
        self.wait = Some(Wait { frames, ..w });
        if frames.at_least(REHEAD) {
            self.job = StreamLoad::rehead(p, id, heads).map(SourceJob::Rehead);
        }
    }

    pub(crate) fn run_in(&mut self, job: SourceJob, s: &mut dyn Store, r: &Ready, p: &mut Project) {
        match job {
            SourceJob::List(part, delta) => {
                let listed = TapeList::list(s, r);
                self.card = Some(listed.unwrap_or(TapeList::EMPTY));
                self.note = listed.err().map(said);
                self.turn_in(p, part, delta);
            }
            SourceJob::Head {
                part,
                stem,
                size,
                to,
            } => match read_head(s, r, stem, size) {
                Ok(h) => {
                    self.heads.put(h);
                    let sound = &p.part(part).sound;
                    match to {
                        HeadFor::Pick if sound.source_ref() == Some(h.sref) => {
                            p.edit_part(part).sound.pick_source(&h, None);
                        }
                        // The library's Sound still on it: loaded as on
                        // its card row (#600).
                        // The cell's own row on another Part is kept.
                        HeadFor::Load
                            if sound.source_ref() == Some(h.sref)
                                && self.at.is_none_or(|a| a.0 == part) =>
                        {
                            self.at = Some((part, stem, size));
                            self.load_in(p, part);
                        }
                        HeadFor::Pick | HeadFor::Load | HeadFor::Name => {}
                    }
                }
                Err(w) => self.note = Some(w),
            },
            SourceJob::Load(part, mut l) => match l.step(s, r, p) {
                LoadStep::More => self.job = Some(SourceJob::Load(part, l)),
                // Its head was read when the cell came onto its row.
                LoadStep::Done(_, h) => {
                    self.at = None;
                    self.pick(p, part, h.sref);
                }
                LoadStep::Failed(w) => self.note = Some(w),
            },
            #[cfg(feature = "tape")]
            SourceJob::Stream(part, mut l) => match l.step(s, r, p, STEP_BYTES as usize) {
                StreamStep::More => self.job = Some(SourceJob::Stream(part, l)),
                StreamStep::Done(_) => {
                    self.at = None;
                    self.pick(p, part, l.sref());
                }
                StreamStep::Failed(f) => self.note = Some(stream_note(f)),
            },
            #[cfg(feature = "tape")]
            SourceJob::Rehead(mut l) => match l.step(s, r, p, STEP_BYTES as usize) {
                StreamStep::More => self.job = Some(SourceJob::Rehead(l)),
                StreamStep::Done(_) => {}
                // A card fault, or a note that started during the head
                // read with nothing free beside it (#761 m8): passing, so
                // tried again after another `REHEAD` once `can_rehead`
                // lets it.
                StreamStep::Failed(StreamFail::Sounding) => self.wait = None,
                StreamStep::Failed(StreamFail::Read(e))
                    if crate::storage::CardFault::store_error(&e).is_some() =>
                {
                    self.wait = None
                }
                // Not again until the heads it wants change, or the card.
                StreamStep::Failed(_) => {
                    if let Some(w) = self.wait {
                        self.stuck[w.id.index()] = Some(heads_key(&w.heads));
                    }
                }
            },
            // `UiState::lib_step`'s, not this.
            SourceJob::Stage(l) => self.job = Some(SourceJob::Stage(l)),
        }
    }
}
