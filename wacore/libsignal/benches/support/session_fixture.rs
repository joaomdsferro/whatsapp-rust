use wacore_libsignal::protocol::{IdentityKey, KeyPair, RootKey, SessionState};

pub fn sessions(count: usize, is_self: bool) -> Vec<SessionState> {
    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(0x5345_4c46);
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
