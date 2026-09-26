use chimera_core::triple::{Reader, TripleBuffer, Writer};

fn pair<T: Send + 'static>(a: T, b: T, c: T) -> (Writer<T>, Reader<T>) {
    Box::leak(Box::new(TripleBuffer::new(a, b, c))).split()
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[test]
fn reader_starts_on_the_first_value() {
    let (_w, mut r) = pair(1u32, 2, 3);
    assert_eq!(*r.read(), 1);
}

#[test]
fn reader_gets_the_newest_publish() {
    let (mut w, mut r) = pair(0u32, 0, 0);
    w.publish(|x| *x = 1);
    w.publish(|x| *x = 2);
    w.publish(|x| *x = 3);
    assert_eq!(*r.read(), 3);
    assert_eq!(*r.read(), 3, "nothing newer: the same value again");
}

#[test]
fn a_held_read_is_stable_across_publishes() {
    let (mut w, mut r) = pair([0u64; 8], [0; 8], [0; 8]);
    w.publish(|x| *x = [7; 8]);
    let held = r.read();
    for n in 8..20 {
        w.publish(|x| *x = [n; 8]);
    }
    assert_eq!(*held, [7; 8]);
    assert_eq!(*r.read(), [19; 8]);
}

#[test]
fn random_interleavings_match_the_model() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut w, mut r) = pair([0u64; 4], [0; 4], [0; 4]);
    let mut latest = 0u64;
    let mut next = 1u64;
    for step in 0..20_000 {
        let held = r.read();
        assert_eq!(
            *held, [latest; 4],
            "step {step}: read is the newest publish"
        );
        let held_addr = held as *const [u64; 4] as usize;
        let snapshot = *held;
        for _ in 0..rng.below(5) {
            let value = next;
            w.publish(|x| {
                assert_ne!(
                    x as *mut [u64; 4] as usize, held_addr,
                    "step {step}: the writer was handed the held slot"
                );
                *x = [value; 4];
            });
            latest = value;
            next += 1;
        }
        assert_eq!(*held, snapshot, "step {step}: the held read changed");
    }
}

#[test]
fn a_second_thread_never_sees_a_torn_or_older_value() {
    const N: u64 = 200_000;
    let (mut w, mut r) = pair([0u64; 16], [0; 16], [0; 16]);
    let writer = std::thread::spawn(move || {
        for n in 1..=N {
            w.publish(|x| *x = [n; 16]);
        }
    });
    let mut last = 0;
    while last < N {
        let done = writer.is_finished();
        let v = *r.read();
        assert!(v.iter().all(|&e| e == v[0]), "torn read {v:?}");
        assert!(v[0] >= last, "went back from {last} to {}", v[0]);
        last = v[0];
        if done {
            assert_eq!(
                last, N,
                "the last publish is visible once the writer is done"
            );
        }
    }
    writer.join().unwrap();
}

#[test]
fn init_in_place_starts_like_new() {
    let slot: &'static mut core::mem::MaybeUninit<TripleBuffer<u32>> =
        Box::leak(Box::new(core::mem::MaybeUninit::uninit()));
    let mut n = 0;
    let tb = TripleBuffer::init_in_place(slot, || {
        n += 1;
        n * 10
    });
    let (mut w, mut r) = tb.split();
    assert_eq!(*r.read(), 10, "the reader starts on the first value built");
    w.publish(|x| *x += 1);
    assert_eq!(*r.read(), 31, "the writer's first slot is the third value");
}
