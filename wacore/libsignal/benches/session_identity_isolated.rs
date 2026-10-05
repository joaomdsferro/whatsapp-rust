use divan::black_box;
use wacore_libsignal::protocol::{IdentityKey, KeyPair, RootKey, SessionState};

#[path = "support/session_fixture.rs"]
mod session_fixture;

fn main() {
    divan::main();
}

// Retain the original single-call workload while testing process isolation.
#[divan::bench(args = [false, true])]
fn bench_session_with_self(bencher: divan::Bencher, is_self: bool) {
    bencher
        .with_inputs(|| {
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(0x5345_4c46);
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
        .bench_refs(|state| black_box(state.session_with_self().expect("identities")));
}

// A separate binary removes unrelated benchmark code and allocator history.
// The original cold-call cases remain in libsignal_benchmark.
#[divan::bench(args = [(false, 1), (true, 1), (false, 2), (true, 2)])]
fn independent_sessions_32(bencher: divan::Bencher, (is_self, passes): (bool, usize)) {
    bencher
        .with_inputs(|| session_fixture::sessions(32, is_self))
        .bench_refs(|states| {
            for _ in 0..passes {
                for state in states.iter() {
                    black_box(black_box(state).session_with_self().expect("identities"));
                }
            }
        });
}
