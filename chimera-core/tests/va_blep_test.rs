//! VA 2-point polyBLEP/polyBLAMP kernels and the Corrector (Task 2).

use chimera_core::dsp::provenance::{Patent, Provenance};
use chimera_core::dsp::va::blep::{self, Corrector, blamp, blep};

fn close(a: f32, b: f32, tol: f32) {
    assert!((a - b).abs() <= tol, "{a} vs {b}");
}

#[test]
fn kernels_at_their_ends() {
    assert_eq!(blep(0.0), [0.0, -0.5]);
    assert_eq!(blep(1.0), [0.5, 0.0]);
    assert_eq!(blamp(0.0), [0.0, 1.0 / 6.0]);
    assert_eq!(blamp(1.0), [1.0 / 6.0, 0.0]);
    assert_eq!(blep(0.5), [0.125, -0.125]);
}

#[test]
fn blep_keeps_the_edge_time() {
    for i in 0..=64 {
        let t = i as f32 / 64.0;
        let k = blep(t);
        close(k[0] + k[1], t - 0.5, 1e-6);
    }
}

#[test]
fn blamp_integrates_blep() {
    const N: usize = 1024;
    for i in 0..=16 {
        let t = i as f32 / 16.0;
        let (mut before, mut after) = (0.0f64, 0.0f64);
        for k in 0..N {
            let u = (k as f64 + 0.5) / N as f64 * t as f64;
            before += 0.5 * u * u * (t as f64 / N as f64);
            let v = t as f64 + (k as f64 + 0.5) / N as f64 * (1.0 - t as f64);
            after += 0.5 * (1.0 - v) * (1.0 - v) * ((1.0 - t as f64) / N as f64);
        }
        let k = blamp(t);
        assert!((k[0] as f64 - before).abs() < 1e-5);
        assert!((k[1] as f64 - after).abs() < 1e-5);
    }
}

#[test]
fn corrector_is_one_sample_late() {
    let mut c = Corrector::NEW;
    assert_eq!(Corrector::LATENCY, 1);
    assert_eq!(c.push(1.0), 0.0);
    assert_eq!(c.push(2.0), 1.0);
    assert_eq!(c.push(3.0), 2.0);
}

#[test]
fn corrector_applies_both_halves() {
    let mut c = Corrector::NEW;
    c.push(-1.0);
    c.edge(0.25, 2.0, 0.0);
    let a = c.push(1.0);
    let b = c.push(0.0);
    let k = blep(0.25);
    close(a, -1.0 + 2.0 * k[0], 1e-6);
    close(b, 1.0 + 2.0 * k[1], 1e-6);
    // a third push shows the correction does not linger
    c.edge(0.5, 1.0, 0.5);
    c.push(5.0);
    assert_eq!(c.last_edges().len(), 0);
    close(c.push(0.0), 5.0 + blep(0.5)[1] + 0.5 * blamp(0.5)[1], 1e-6);
    assert_eq!(c.push(0.0), 0.0);
}

#[test]
fn corrections_superpose() {
    let mut c = Corrector::NEW;
    c.push(0.0);
    // two edges in the same interval
    c.edge(0.2, 1.0, 0.0);
    c.edge(0.7, -0.5, 0.25);
    assert_eq!(c.last_edges().len(), 2);
    let a = c.push(1.0);
    close(a, blep(0.2)[0] - 0.5 * blep(0.7)[0] + 0.25 * blamp(0.7)[0], 1e-6);
    // adjacent intervals: both reach the shared sample
    let (t1, s1, t2, s2) = (0.3, 2.0, 0.6, -1.0);
    let mut c = Corrector::NEW;
    c.push(0.0);
    c.edge(t1, s1, 0.0);
    c.push(4.0); // the shared sample
    c.edge(t2, s2, 0.0);
    let shared = c.push(0.0);
    close(shared, 4.0 + s1 * blep(t1)[1] + s2 * blep(t2)[0], 1e-6);
}

#[test]
fn provenance_is_declared() {
    assert!(!blep::PROVENANCE.is_empty());
    assert!(
        blep::PROVENANCE
            .iter()
            .all(|p| matches!(p, Provenance::Textbook { .. }))
    );
    assert_eq!(blep::PATENT.len(), 2);
    assert!(blep::PATENT.iter().all(|l| l.patent == Patent::NotChecked));
    assert!(!blep::GPL_CHECK.is_empty());
}
