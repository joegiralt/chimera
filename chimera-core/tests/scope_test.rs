use chimera_core::scope::{SCOPE_LEN, ScopeWriter, scope_buffer};

fn writer_and_reader() -> (ScopeWriter, chimera_core::triple::Reader<[f32; SCOPE_LEN]>) {
    let (w, r) = Box::leak(Box::new(scope_buffer())).split();
    (ScopeWriter::new(w), r)
}

#[test]
fn a_full_back_buffer_publishes_from_the_first_rising_zero_crossing() {
    let (mut sw, mut r) = writer_and_reader();
    let samples: Vec<f32> = (0..2 * SCOPE_LEN)
        .map(|i| {
            if i < 100 {
                -0.5
            } else {
                (i - 99) as f32 * 0.001
            }
        })
        .collect();
    for block in samples.chunks(64) {
        sw.write(block);
    }
    assert_eq!(&r.read()[..], &samples[100..100 + SCOPE_LEN]);
}

#[test]
fn nothing_is_published_until_the_back_buffer_fills() {
    let (mut sw, mut r) = writer_and_reader();
    sw.write(&[0.25; 400]);
    assert_eq!(*r.read(), [0.0; SCOPE_LEN]);
}

#[test]
fn without_a_zero_crossing_the_window_starts_at_the_beginning() {
    let (mut sw, mut r) = writer_and_reader();
    let samples: Vec<f32> = (0..2 * SCOPE_LEN).map(|i| 0.1 + i as f32 * 1e-4).collect();
    sw.write(&samples);
    assert_eq!(&r.read()[..], &samples[..SCOPE_LEN]);
}
