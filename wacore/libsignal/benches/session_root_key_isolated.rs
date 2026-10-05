use divan::black_box;
use wacore_libsignal::protocol::RootKey;

#[path = "support/session_fixture.rs"]
mod session_fixture;

fn main() {
    divan::main();
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
