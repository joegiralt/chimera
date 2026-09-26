//! The union of two algorithms' links, each weighted at MORPH 0 and 1, in
//! an order where every link runs forward when it can (spec § Plan and morph).

pub const OPS: usize = 6;
pub const MAX_EDGES: usize = 15;

/// `src` modulates `dst` (0-based) with weight `a` in ALG A and `b` in ALG B.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub src: u8,
    pub dst: u8,
    pub a: f32,
    pub b: f32,
}

impl Edge {
    const NONE: Edge = Edge {
        src: 0,
        dst: 0,
        a: 0.0,
        b: 0.0,
    };
}

/// The one blend of a link weight or carrier gain at MORPH `m`: exact at
/// `m == 0` and `m == 1` for weights of 0 and 1.
#[inline(always)]
pub fn blend(a: f32, b: f32, m: f32) -> f32 {
    a + m * (b - a)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EvalPlan {
    pub order: [u8; OPS],
    /// Grouped by target in `order`; ascending source within a target.
    pub edges: [Edge; MAX_EDGES],
    /// Links into `order[k]` are `edges[starts[k]..starts[k + 1]]`.
    pub starts: [u8; OPS + 1],
    /// Bit `e`: link `e`'s source runs later, so it reads the previous sample.
    pub delayed: u16,
    pub carrier_a: [f32; OPS],
    pub carrier_b: [f32; OPS],
}

impl EvalPlan {
    /// `mods[i]` bit `j`: operator `i` modulates operator `j`; `carriers` bit `i`: heard.
    pub fn build(a_mods: &[u8; OPS], a_carriers: u8, b_mods: &[u8; OPS], b_carriers: u8) -> Self {
        let union: [u8; OPS] = core::array::from_fn(|i| a_mods[i] | b_mods[i]);
        let order = topo_order(&union);
        let mut pos = [0usize; OPS];
        for (k, &op) in order.iter().enumerate() {
            pos[op as usize] = k;
        }
        let mut edges = [Edge::NONE; MAX_EDGES];
        let mut starts = [0u8; OPS + 1];
        let mut delayed = 0u16;
        let mut n = 0;
        for (k, &dst) in order.iter().enumerate() {
            starts[k] = n as u8;
            let bit = 1u8 << dst;
            for src in 0..OPS {
                let (in_a, in_b) = (a_mods[src] & bit != 0, b_mods[src] & bit != 0);
                if !(in_a || in_b) {
                    continue;
                }
                if pos[src] > k {
                    delayed |= 1 << n;
                }
                edges[n] = Edge {
                    src: src as u8,
                    dst,
                    a: weight(in_a),
                    b: weight(in_b),
                };
                n += 1;
            }
        }
        starts[OPS] = n as u8;
        Self {
            order,
            edges,
            starts,
            delayed,
            carrier_a: core::array::from_fn(|i| weight(a_carriers & (1 << i) != 0)),
            carrier_b: core::array::from_fn(|i| weight(b_carriers & (1 << i) != 0)),
        }
    }

    pub fn edge_count(&self) -> usize {
        self.starts[OPS] as usize
    }
}

fn weight(on: bool) -> f32 {
    if on { 1.0 } else { 0.0 }
}

/// Kahn's algorithm, highest-numbered ready operator first; a cycle is
/// broken at its highest operator.
fn topo_order(mods: &[u8; OPS]) -> [u8; OPS] {
    let mut indegree = [0u8; OPS];
    for m in mods {
        for (dst, d) in indegree.iter_mut().enumerate() {
            *d += (m >> dst) & 1;
        }
    }
    let mut order = [0u8; OPS];
    let mut placed = 0u8;
    for slot in order.iter_mut() {
        let free = |i: &usize| placed & (1 << i) == 0;
        let Some(i) = (0..OPS)
            .rev()
            .filter(free)
            .find(|&i| indegree[i] == 0)
            .or_else(|| (0..OPS).rev().find(free))
        else {
            break;
        };
        *slot = i as u8;
        placed |= 1 << i;
        for (dst, d) in indegree.iter_mut().enumerate() {
            if mods[i] & (1 << dst) != 0 {
                *d = d.saturating_sub(1);
            }
        }
    }
    order
}
