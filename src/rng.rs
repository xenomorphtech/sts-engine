//! Exact ports of libGDX `RandomXS128` (2016) and STS `com.megacrit.cardcrawl.random.Random`.

use serde::{Deserialize, Serialize};

const MURMUR_C1: u64 = 0xff51afd7ed558ccd;
const MURMUR_C2: u64 = 0xc4ceb9fe1a85ec53;
const NORM_DOUBLE: f64 = 1.110_223_024_625_156_5e-16; // 0x1p-53
const NORM_FLOAT: f64 = 5.960_464_477_539_063e-8; // 0x1p-24

#[inline]
const fn murmur_hash3(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(MURMUR_C1);
    x ^= x >> 33;
    x = x.wrapping_mul(MURMUR_C2);
    x ^= x >> 33;
    x
}

/// libGDX `RandomXS128` xorshift128+ used by the desktop 2016 game JAR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RandomXs128 {
    pub seed0: i64,
    pub seed1: i64,
}

impl RandomXs128 {
    pub fn from_seed(seed: i64) -> Self {
        let mixed = if seed == 0 { i64::MIN } else { seed };
        let seed0 = murmur_hash3(mixed as u64) as i64;
        let seed1 = murmur_hash3(seed0 as u64) as i64;
        Self { seed0, seed1 }
    }

    pub fn from_state(seed0: i64, seed1: i64) -> Self {
        Self { seed0, seed1 }
    }

    pub fn next_long(&mut self) -> i64 {
        let s0 = self.seed0 as u64;
        let s1 = self.seed1 as u64;
        self.seed0 = s1 as i64;
        let s0 = s0 ^ (s0 << 23);
        let next = s0 ^ s1 ^ (s0 >> 17) ^ (s1 >> 26);
        self.seed1 = next as i64;
        s1.wrapping_add(next) as i64
    }

    pub fn next_long_bound(&mut self, n: i64) -> i64 {
        assert!(n > 0, "n must be positive");
        loop {
            let bits = (self.next_long() as u64) >> 1;
            let val = (bits % n as u64) as i64;
            if (bits as i64).wrapping_sub(val).wrapping_add(n - 1) >= 0 {
                return val;
            }
        }
    }

    pub fn next_int(&mut self) -> i32 {
        self.next_long() as i32
    }

    pub fn next_int_bound(&mut self, n: i32) -> i32 {
        self.next_long_bound(n as i64) as i32
    }

    pub fn next_double(&mut self) -> f64 {
        ((self.next_long() as u64) >> 11) as f64 * NORM_DOUBLE
    }

    pub fn next_float(&mut self) -> f32 {
        (((self.next_long() as u64) >> 40) as f64 * NORM_FLOAT) as f32
    }

    pub fn next_boolean(&mut self) -> bool {
        self.next_long() & 1 != 0
    }
}

/// STS named-stream wrapper. `random(range)` calls `nextInt(range + 1)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StsRandom {
    pub random: RandomXs128,
    pub counter: i32,
}

impl StsRandom {
    pub fn from_seed(seed: i64) -> Self {
        Self {
            random: RandomXs128::from_seed(seed),
            counter: 0,
        }
    }

    /// `new Random(seed, counter)` burns `counter` calls of `random(999)`.
    pub fn from_seed_counter(seed: i64, counter: i32) -> Self {
        let mut rng = Self::from_seed(seed);
        for _ in 0..counter {
            rng.random_int(999);
        }
        rng
    }

    pub fn snapshot(&self) -> RngSnapshot {
        RngSnapshot {
            counter: self.counter,
            state0: self.random.seed0,
            state1: self.random.seed1,
        }
    }

    pub fn random_int(&mut self, range: i32) -> i32 {
        self.counter += 1;
        self.random.next_int_bound(range + 1)
    }

    pub fn random_range(&mut self, start: i32, end: i32) -> i32 {
        self.counter += 1;
        start + self.random.next_int_bound(end - start + 1)
    }

    pub fn random_long(&mut self) -> i64 {
        self.counter += 1;
        self.random.next_long()
    }

    pub fn random_boolean(&mut self) -> bool {
        self.counter += 1;
        self.random.next_boolean()
    }

    pub fn random_boolean_chance(&mut self, chance: f32) -> bool {
        self.counter += 1;
        self.random.next_float() < chance
    }

    pub fn random_float(&mut self) -> f32 {
        self.counter += 1;
        self.random.next_float()
    }

    pub fn random_float_range(&mut self, start: f32, end: f32) -> f32 {
        self.counter += 1;
        start + self.random.next_float() * (end - start)
    }

    /// `setCounter` burns `randomBoolean()` until the counter matches.
    pub fn set_counter(&mut self, target: i32) {
        while self.counter < target {
            self.random_boolean();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RngSnapshot {
    pub counter: i32,
    pub state0: i64,
    pub state1: i64,
}

/// The 13 named STS gameplay streams.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RngSet {
    pub monster: StsRandom,
    pub map: StsRandom,
    pub event: StsRandom,
    pub merchant: StsRandom,
    pub card: StsRandom,
    pub treasure: StsRandom,
    pub relic: StsRandom,
    pub potion: StsRandom,
    pub monster_hp: StsRandom,
    pub ai: StsRandom,
    pub shuffle: StsRandom,
    pub card_random: StsRandom,
    pub misc: StsRandom,
}

impl RngSet {
    /// `AbstractDungeon.generateSeeds()`. `map` is created later per act.
    pub fn generate_seeds(seed: i64) -> Self {
        Self {
            monster: StsRandom::from_seed(seed),
            map: StsRandom::from_seed(seed),
            event: StsRandom::from_seed(seed),
            merchant: StsRandom::from_seed(seed),
            card: StsRandom::from_seed(seed),
            treasure: StsRandom::from_seed(seed),
            relic: StsRandom::from_seed(seed),
            potion: StsRandom::from_seed(seed),
            monster_hp: StsRandom::from_seed(seed),
            ai: StsRandom::from_seed(seed),
            shuffle: StsRandom::from_seed(seed),
            card_random: StsRandom::from_seed(seed),
            misc: StsRandom::from_seed(seed),
        }
    }

    /// Floor transition resets the in-combat streams to `seed + floor`.
    pub fn reset_floor_streams(&mut self, seed: i64, floor: i32) {
        let mixed = seed.wrapping_add(floor as i64);
        self.monster_hp = StsRandom::from_seed(mixed);
        self.ai = StsRandom::from_seed(mixed);
        self.shuffle = StsRandom::from_seed(mixed);
        self.card_random = StsRandom::from_seed(mixed);
        self.misc = StsRandom::from_seed(mixed);
    }

    pub fn snapshot(&self) -> RngSetSnapshot {
        RngSetSnapshot {
            monster: self.monster.snapshot(),
            map: self.map.snapshot(),
            event: self.event.snapshot(),
            merchant: self.merchant.snapshot(),
            card: self.card.snapshot(),
            treasure: self.treasure.snapshot(),
            relic: self.relic.snapshot(),
            potion: self.potion.snapshot(),
            monster_hp: self.monster_hp.snapshot(),
            ai: self.ai.snapshot(),
            shuffle: self.shuffle.snapshot(),
            card_random: self.card_random.snapshot(),
            misc: self.misc.snapshot(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngSetSnapshot {
    pub monster: RngSnapshot,
    pub map: RngSnapshot,
    pub event: RngSnapshot,
    pub merchant: RngSnapshot,
    pub card: RngSnapshot,
    pub treasure: RngSnapshot,
    pub relic: RngSnapshot,
    pub potion: RngSnapshot,
    pub monster_hp: RngSnapshot,
    pub ai: RngSnapshot,
    pub shuffle: RngSnapshot,
    pub card_random: RngSnapshot,
    pub misc: RngSnapshot,
}

/// SeedHelper alphanumeric codec (`0123456789ABCDEFGHIJKLMNPQRSTUVWXYZ`).
pub fn seed_from_string(raw: &str) -> i64 {
    const CHARS: &[u8] = b"0123456789ABCDEFGHIJKLMNPQRSTUVWXYZ";
    let cleaned = raw.trim().to_ascii_uppercase().replace('O', "0");
    let mut total: i64 = 0;
    for b in cleaned.bytes() {
        let idx = CHARS.iter().position(|&c| c == b).unwrap_or(0) as i64;
        total = total.wrapping_mul(CHARS.len() as i64).wrapping_add(idx);
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_two_matches_java_initial_stream() {
        let rng = StsRandom::from_seed(2);
        assert_eq!(rng.random.seed0, 4233148493373801447);
        assert_eq!(rng.random.seed1, 3386738095288643496);
    }

    #[test]
    fn seed_helper_numeric_string() {
        assert_eq!(seed_from_string("2"), 2);
        assert_eq!(seed_from_string("1"), 1);
    }
}
