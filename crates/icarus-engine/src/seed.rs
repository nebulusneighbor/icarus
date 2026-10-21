/// FNV-1a hash. Used instead of `std`'s hasher, whose output may change between Rust releases,
/// so that per-node seeds (and therefore renders) stay stable forever.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
