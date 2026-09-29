//! The 32 algorithms (spec § Algorithms), as data. Operators are 1–6 in the
//! tables; higher numbers modulate lower ones.

use crate::dsp::algo::plan::{EvalPlan, OPS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Algorithm {
    /// The label the UI shows.
    pub name: &'static str,
    /// The algorithm's frozen disk ident (ADR 0045): one token, a literal of
    /// its own so a relabel can't move it. Append-only, like the entry.
    pub ident: &'static str,
    /// `mods[i]` bit `j`: operator `i + 1` modulates operator `j + 1`.
    pub mods: [u8; OPS],
    pub carriers: u8,
}

const fn alg(
    name: &'static str,
    ident: &'static str,
    links: &[(u8, u8)],
    carriers: &[u8],
) -> Algorithm {
    let mut mods = [0u8; OPS];
    let mut i = 0;
    while i < links.len() {
        mods[links[i].0 as usize - 1] |= 1 << (links[i].1 - 1);
        i += 1;
    }
    let mut mask = 0u8;
    let mut i = 0;
    while i < carriers.len() {
        mask |= 1 << (carriers[i] - 1);
        i += 1;
    }
    Algorithm {
        name,
        ident,
        mods,
        carriers: mask,
    }
}

pub const ALGO_COUNT: usize = 32;

/// Append-only: an algorithm's index is its disk code (ADR 0045), frozen with
/// its name in `tests/fixtures/disk_codes_v1.txt`. Never reorder or remove.
pub static ALGORITHMS: [Algorithm; ALGO_COUNT] = [
    alg("T1", "T1", &[(4, 3), (3, 2), (2, 1), (6, 5)], &[1, 5]),
    alg("T2", "T2", &[(4, 2), (3, 2), (2, 1), (6, 5)], &[1, 5]),
    alg("T3", "T3", &[(3, 2), (2, 1), (4, 1), (6, 5)], &[1, 5]),
    alg("T4", "T4", &[(4, 3), (3, 1), (2, 1), (6, 5)], &[1, 5]),
    alg("T5", "T5", &[(4, 3), (2, 1), (6, 5)], &[1, 3, 5]),
    alg("T6", "T6", &[(4, 1), (4, 2), (4, 3), (6, 5)], &[1, 2, 3, 5]),
    alg("T7", "T7", &[(4, 3), (6, 5)], &[1, 2, 3, 5]),
    alg("T8", "T8", &[(6, 5)], &[1, 2, 3, 4, 5]),
    alg("A1", "A1", &[], &[1, 2, 3, 4, 5, 6]),
    alg("A2", "A2", &[(6, 1)], &[1, 2, 3, 4, 5]),
    alg("A3", "A3", &[(6, 1), (6, 2)], &[1, 2, 3, 4, 5]),
    alg("A4", "A4", &[(6, 1), (5, 2)], &[1, 2, 3, 4]),
    alg(
        "A5",
        "A5",
        &[(6, 1), (6, 2), (6, 3), (6, 4)],
        &[1, 2, 3, 4, 5],
    ),
    alg("A6", "A6", &[(6, 5), (5, 1)], &[1, 2, 3, 4]),
    alg("A7", "A7", &[(4, 1), (5, 2), (6, 3)], &[1, 2, 3]),
    alg("A8", "A8", &[(4, 1), (5, 1), (6, 1)], &[1, 2, 3]),
    alg("A9", "A9", &[(6, 5), (5, 4), (4, 1)], &[1, 2, 3]),
    alg("A10", "A10", &[(6, 4), (6, 5), (4, 1), (5, 2)], &[1, 2, 3]),
    alg(
        "A11",
        "A11",
        &[(5, 4), (6, 4), (4, 1), (4, 2), (4, 3)],
        &[1, 2, 3],
    ),
    alg("A12", "A12", &[(5, 3), (3, 1), (6, 4), (4, 2)], &[1, 2]),
    alg("A13", "A13", &[(3, 1), (4, 1), (5, 2), (6, 2)], &[1, 2]),
    alg(
        "A14",
        "A14",
        &[(3, 1), (3, 2), (4, 1), (4, 2), (5, 3), (6, 4)],
        &[1, 2],
    ),
    alg("A15", "A15", &[(6, 5), (5, 3), (3, 1), (4, 2)], &[1, 2]),
    alg(
        "A16",
        "A16",
        &[
            (3, 1),
            (3, 2),
            (4, 1),
            (4, 2),
            (5, 1),
            (5, 2),
            (6, 1),
            (6, 2),
        ],
        &[1, 2],
    ),
    alg(
        "A17",
        "A17",
        &[(6, 5), (5, 4), (4, 3), (3, 2), (2, 1)],
        &[1],
    ),
    alg(
        "A18",
        "A18",
        &[(2, 1), (3, 1), (4, 1), (5, 1), (6, 1)],
        &[1],
    ),
    alg(
        "A19",
        "A19",
        &[(4, 2), (4, 3), (2, 1), (3, 1), (6, 5), (5, 1)],
        &[1],
    ),
    alg(
        "A20",
        "A20",
        &[(3, 2), (2, 1), (5, 4), (4, 1), (6, 1)],
        &[1],
    ),
    alg(
        "A21",
        "A21",
        &[(6, 5), (5, 4), (4, 1), (3, 2), (2, 1)],
        &[1],
    ),
    alg(
        "A22",
        "A22",
        &[(6, 4), (6, 5), (4, 2), (5, 3), (2, 1), (3, 1)],
        &[1],
    ),
    alg(
        "A23",
        "A23",
        &[
            (6, 2),
            (6, 3),
            (6, 4),
            (6, 5),
            (2, 1),
            (3, 1),
            (4, 1),
            (5, 1),
        ],
        &[1],
    ),
    alg(
        "A24",
        "A24",
        &[(6, 5), (5, 2), (5, 3), (5, 4), (2, 1), (3, 1), (4, 1)],
        &[1],
    ),
];

pub static ALGO_NAMES: [&str; ALGO_COUNT] = {
    let mut n = [""; ALGO_COUNT];
    let mut i = 0;
    while i < ALGO_COUNT {
        n[i] = ALGORITHMS[i].name;
        i += 1;
    }
    n
};

pub static ALGO_IDENTS: [&str; ALGO_COUNT] = {
    let mut n = [""; ALGO_COUNT];
    let mut i = 0;
    while i < ALGO_COUNT {
        n[i] = ALGORITHMS[i].ident;
        i += 1;
    }
    n
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlgoId(u8);

impl AlgoId {
    pub const T1: AlgoId = AlgoId(0);
    pub const T2: AlgoId = AlgoId(1);
    pub const T3: AlgoId = AlgoId(2);
    pub const T4: AlgoId = AlgoId(3);
    pub const T5: AlgoId = AlgoId(4);
    pub const T6: AlgoId = AlgoId(5);
    pub const T7: AlgoId = AlgoId(6);
    pub const T8: AlgoId = AlgoId(7);
    pub const A1: AlgoId = AlgoId(8);
    pub const A2: AlgoId = AlgoId(9);
    pub const A3: AlgoId = AlgoId(10);
    pub const A4: AlgoId = AlgoId(11);
    pub const A5: AlgoId = AlgoId(12);
    pub const A6: AlgoId = AlgoId(13);
    pub const A7: AlgoId = AlgoId(14);
    pub const A8: AlgoId = AlgoId(15);
    pub const A9: AlgoId = AlgoId(16);
    pub const A10: AlgoId = AlgoId(17);
    pub const A11: AlgoId = AlgoId(18);
    pub const A12: AlgoId = AlgoId(19);
    pub const A13: AlgoId = AlgoId(20);
    pub const A14: AlgoId = AlgoId(21);
    pub const A15: AlgoId = AlgoId(22);
    pub const A16: AlgoId = AlgoId(23);
    pub const A17: AlgoId = AlgoId(24);
    pub const A18: AlgoId = AlgoId(25);
    pub const A19: AlgoId = AlgoId(26);
    pub const A20: AlgoId = AlgoId(27);
    pub const A21: AlgoId = AlgoId(28);
    pub const A22: AlgoId = AlgoId(29);
    pub const A23: AlgoId = AlgoId(30);
    pub const A24: AlgoId = AlgoId(31);

    /// `v` if it names an algorithm.
    pub const fn from_index(v: u8) -> Option<Self> {
        if (v as usize) < ALGO_COUNT {
            Some(AlgoId(v))
        } else {
            None
        }
    }

    pub const fn clamped(v: u8) -> Self {
        if (v as usize) < ALGO_COUNT {
            AlgoId(v)
        } else {
            AlgoId(ALGO_COUNT as u8 - 1)
        }
    }

    pub const fn get(self) -> u8 {
        self.0
    }

    pub fn algorithm(self) -> &'static Algorithm {
        &ALGORITHMS[self.0 as usize]
    }
}

pub fn plan(a: AlgoId, b: AlgoId) -> EvalPlan {
    let (a, b) = (a.algorithm(), b.algorithm());
    EvalPlan::build(&a.mods, a.carriers, &b.mods, b.carriers)
}
