use divan::black_box;
use wacore_libsignal::protocol::{IdentityKey, KeyPair, RootKey, SessionState};

#[path = "support/session_fixture.rs"]
mod session_fixture;

fn main() {
    divan::main();
}

// Retain the original single-call workload while testing process isolation.
#[divan::bench]
fn bench_session_root_key_update(bencher: divan::Bencher) {
    bencher
        .with_inputs(|| {
            let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(0x524f_4f54);
            let local = IdentityKey::new(KeyPair::generate(&mut rng).public_key);
            SessionState::new(
                3,
                &local,
                &local,
                &RootKey::new([0x11; 32]),
                local.public_key(),
            )
        })
        .bench_refs(|state| {
            state.set_root_key(black_box(&RootKey::new([0x22; 32])));
            black_box(&*state);
        });
}

// One pass preserves the independent-record experiment. Thirty-two passes
// separately measure successive updates on resident sessions; neither replaces
// the original cold-call case. Doubling passes is an explicit slowdown control.
#[divan::bench(args = [1, 2, 32, 64])]
fn successive_updates_32_sessions(bencher: divan::Bencher, passes: usize) {
    bencher
        .with_inputs(|| session_fixture::sessions(32, true))
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
