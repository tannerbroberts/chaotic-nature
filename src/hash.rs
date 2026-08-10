//! Deterministic state hashing — the instrument every other guarantee is read
//! through.
//!
//! FNV-1a 64. Chosen because it is order-sensitive (so it catches an iteration
//! order bug rather than hiding one), trivially portable, and has no
//! platform-dependent behaviour whatsoever. It is not cryptographic and does
//! not need to be — an adversary is not the threat model here, a mismatched
//! float is.

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Clone, Copy, Debug)]
pub struct Hasher(u64);

impl Default for Hasher {
    fn default() -> Self {
        Hasher::new()
    }
}

impl Hasher {
    #[inline]
    pub const fn new() -> Hasher {
        Hasher(FNV_OFFSET)
    }

    #[inline]
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0 ^= v as u64;
        self.0 = self.0.wrapping_mul(FNV_PRIME);
        self
    }

    #[inline]
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        for x in b {
            self.u8(*x);
        }
        self
    }

    #[inline]
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    #[inline]
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    #[inline]
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    #[inline]
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.bytes(&v.to_le_bytes())
    }

    #[inline]
    pub fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }

    #[inline]
    pub fn finish(&self) -> u64 {
        self.0
    }

    // ------------------------------------------------------------------
    // Bulk absorption.
    //
    // Byte-at-a-time FNV over a 92 KB terrain field costs ~100 µs per hash,
    // and the per-tick trace calls this every tick. The lane methods absorb
    // eight bytes per multiply instead — a different (but equally fixed)
    // mixing rule from the byte path, so a lane-hashed field is NOT
    // byte-compatible with `bytes()`. That is fine: the only contract is
    // that the rule never changes once replays exist.
    // ------------------------------------------------------------------

    #[inline]
    fn lane(&mut self, w: u64) {
        self.0 ^= w;
        self.0 = self.0.wrapping_mul(FNV_PRIME);
    }

    /// Absorb a `u16` slice as little-endian 4-lane words, then the length —
    /// so `[1, 2]` and `[1, 2, 0, 0]` cannot collide.
    pub fn u16_lanes(&mut self, v: &[u16]) -> &mut Self {
        let mut it = v.chunks_exact(4);
        for c in &mut it {
            self.lane(
                (c[0] as u64)
                    | (c[1] as u64) << 16
                    | (c[2] as u64) << 32
                    | (c[3] as u64) << 48,
            );
        }
        for r in it.remainder() {
            self.u16(*r);
        }
        self.u64(v.len() as u64)
    }

    /// Absorb a `u64` slice one lane per word, then the length.
    pub fn u64_lanes(&mut self, v: &[u64]) -> &mut Self {
        for w in v {
            self.lane(*w);
        }
        self.u64(v.len() as u64)
    }
}

/// Anything that contributes to the canonical state hash.
pub trait Hashable {
    fn hash_into(&self, h: &mut Hasher);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hash_is_the_offset_basis() {
        assert_eq!(Hasher::new().finish(), FNV_OFFSET);
    }

    #[test]
    fn is_order_sensitive() {
        // This is the property that makes it catch iteration-order bugs.
        let mut a = Hasher::new();
        a.u32(1).u32(2);
        let mut b = Hasher::new();
        b.u32(2).u32(1);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn distinguishes_adjacent_values() {
        let mut a = Hasher::new();
        a.i32(0);
        let mut b = Hasher::new();
        b.i32(1);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn lanes_are_order_sensitive_and_length_delimited() {
        let mut a = Hasher::new();
        a.u16_lanes(&[1, 2, 3, 4]);
        let mut b = Hasher::new();
        b.u16_lanes(&[4, 3, 2, 1]);
        assert_ne!(a.finish(), b.finish());

        // Trailing zeros must not collide with a shorter slice.
        let mut c = Hasher::new();
        c.u16_lanes(&[1, 2]);
        let mut d = Hasher::new();
        d.u16_lanes(&[1, 2, 0, 0]);
        assert_ne!(c.finish(), d.finish());
    }

    #[test]
    fn lanes_cover_the_remainder() {
        // A slice whose length is not a multiple of the lane width must still
        // feel a change in its tail element.
        let mut a = Hasher::new();
        a.u16_lanes(&[9, 9, 9, 9, 7]);
        let mut b = Hasher::new();
        b.u16_lanes(&[9, 9, 9, 9, 8]);
        assert_ne!(a.finish(), b.finish());
    }

    #[test]
    fn u64_lanes_notice_every_word() {
        let base: Vec<u64> = (0..64).collect();
        let mut h0 = Hasher::new();
        h0.u64_lanes(&base);
        for i in 0..64 {
            let mut v = base.clone();
            v[i] ^= 1;
            let mut h = Hasher::new();
            h.u64_lanes(&v);
            assert_ne!(h0.finish(), h.finish(), "word {i} was invisible");
        }
    }

    #[test]
    fn matches_published_fnv1a_vectors() {
        // Pinned against the reference implementation rather than against
        // ourselves. If this ever fails, the hashing scheme changed and every
        // stored replay hash is now meaningless.
        let mut a = Hasher::new();
        a.bytes(b"a");
        assert_eq!(a.finish(), 0xaf63_dc4c_8601_ec8c);

        let mut b = Hasher::new();
        b.bytes(b"foobar");
        assert_eq!(b.finish(), 0x85944171f73967e8);
    }
}
