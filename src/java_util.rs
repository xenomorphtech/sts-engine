//! Exact ports of `java.util.Random` (Java 8) and `Collections.shuffle`.

use crate::rng::RandomXs128;

const MULTIPLIER: i64 = 0x5DEECE66D;
const ADDEND: i64 = 0xB;
const MASK: i64 = (1 << 48) - 1;

/// `java.util.Random` LCG used for relic/boss/card-pile shuffles.
#[derive(Clone, Copy, Debug)]
pub struct JavaRandom {
    seed: i64,
}

impl JavaRandom {
    pub fn from_seed(seed: i64) -> Self {
        Self {
            seed: (seed ^ MULTIPLIER) & MASK,
        }
    }

    fn next_bits(&mut self, bits: i32) -> i32 {
        self.seed = self.seed.wrapping_mul(MULTIPLIER).wrapping_add(ADDEND) & MASK;
        (self.seed >> (48 - bits)) as i32
    }

    pub fn next_int(&mut self, n: i32) -> i32 {
        assert!(n > 0);
        if n & -n == n {
            return ((n as i64 * self.next_bits(31) as i64) >> 31) as i32;
        }
        loop {
            let bits = self.next_bits(31);
            let val = bits % n;
            if bits - val + (n - 1) >= 0 {
                return val;
            }
        }
    }
}

/// `Collections.shuffle(list, new java.util.Random(seed))`.
pub fn shuffle_java<T>(items: &mut [T], seed: i64) {
    let mut rng = JavaRandom::from_seed(seed);
    for i in (2..=items.len()).rev() {
        let j = rng.next_int(i as i32) as usize;
        items.swap(i - 1, j);
    }
}

/// `Collections.shuffle(list, randomXs128)` — used by map room assignment.
pub fn shuffle_xs128<T>(items: &mut [T], rng: &mut RandomXs128) {
    for i in (2..=items.len()).rev() {
        let j = rng.next_int_bound(i as i32) as usize;
        items.swap(i - 1, j);
    }
}

/// Fisher-Yates via STS `shuffleRng.randomLong()` then `java.util.Random`.
pub fn shuffle_with_sts_long<T>(items: &mut [T], seed: i64) {
    shuffle_java(items, seed);
}
