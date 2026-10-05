//! Diagnostic candidates, not production performance improvements.
//!
//! Each sample visits independently constructed sessions once per pass. The
//! second pass is an intentional slowdown control, never a proposed workload.
//! Report batch totals and the explicit session count; CodSpeed's simulation
//! adapter does not implement Divan counters, so it cannot normalize them.

use divan::black_box;
use rand::{SeedableRng, rngs::StdRng};
use wacore_libsignal::protocol::{IdentityKey, KeyPair, RootKey, SessionState};

fn sessions(count: usize, is_self: bool) -> Vec<SessionState> {
    let mut rng = StdRng::seed_from_u64(0x5345_4c46);
    (0..count)
        .map(|_| {
            let local = IdentityKey::new(KeyPair::generate(&mut rng).public_key);
            let remote = if is_self {
                local
            } else {
                IdentityKey::new(KeyPair::generate(&mut rng).public_key)
            };
            SessionState::new(
                3,
                &local,
                &remote,
                &RootKey::new([0x11; 32]),
                local.public_key(),
            )
        })
        .collect()
}

#[divan::bench(args = [
    (false, 32, 1), (false, 32, 2), (true, 32, 1), (true, 32, 2),
    (false, 128, 1), (false, 128, 2), (true, 128, 1), (true, 128, 2),
])]
fn session_with_self(bencher: divan::Bencher, (is_self, count, passes): (bool, usize, usize)) {
    bencher
        .with_inputs(|| sessions(count, is_self))
        .bench_refs(|states| {
            for _ in 0..passes {
                for state in states.iter() {
                    black_box(black_box(state).session_with_self().expect("identities"));
                }
            }
        });
}

#[divan::bench(args = [(32, 1), (32, 2), (128, 1), (128, 2)])]
fn root_key_update(bencher: divan::Bencher, (count, passes): (usize, usize)) {
    bencher
        .with_inputs(|| sessions(count, true))
        .bench_refs(|states| {
            for pass in 0..passes {
                let key = RootKey::new([0x22 + pass as u8; 32]);
                for state in states.iter_mut() {
                    black_box(&mut *state).set_root_key(black_box(&key));
                    black_box(&*state);
                }
            }
        });
}
