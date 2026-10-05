use divan::black_box;

#[path = "support/session_fixture.rs"]
mod session_fixture;

fn main() {
    divan::main();
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
