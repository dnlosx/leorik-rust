//! A `const` re-implementation of the seeded `System.Random` of .NET (the legacy
//! subtractive generator used whenever a seed is passed to the constructor).
//!
//! Leorik derives its Zobrist keys from `new Random(228126)`, so this is needed to
//! reproduce the exact same hash keys (and therefore the same transposition table behaviour).

const MBIG: i32 = i32::MAX;
const MSEED: i32 = 161803398;

#[derive(Clone)]
pub struct DotNetRandom {
    seed_array: [i32; 56],
    inext: usize,
    inextp: usize,
}

impl DotNetRandom {
    pub const fn new(seed: i32) -> DotNetRandom {
        let mut seed_array = [0i32; 56];
        let subtraction = if seed == i32::MIN { i32::MAX } else { seed.abs() };
        let mut mj = MSEED - subtraction;
        seed_array[55] = mj;
        let mut mk: i32 = 1;
        let mut i = 1;
        while i < 55 {
            let ii = (21 * i) % 55;
            seed_array[ii] = mk;
            mk = mj - mk;
            if mk < 0 {
                mk += MBIG;
            }
            mj = seed_array[ii];
            i += 1;
        }
        let mut k = 1;
        while k < 5 {
            let mut i = 1;
            while i < 56 {
                let mut n = i + 30;
                if n >= 55 {
                    n -= 55;
                }
                seed_array[i] = seed_array[i].wrapping_sub(seed_array[1 + n]);
                if seed_array[i] < 0 {
                    seed_array[i] = seed_array[i].wrapping_add(MBIG);
                }
                i += 1;
            }
            k += 1;
        }
        DotNetRandom { seed_array, inext: 0, inextp: 21 }
    }

    pub const fn internal_sample(&mut self) -> i32 {
        let mut loc_inext = self.inext + 1;
        if loc_inext >= 56 {
            loc_inext = 1;
        }
        let mut loc_inextp = self.inextp + 1;
        if loc_inextp >= 56 {
            loc_inextp = 1;
        }
        let mut ret_val = self.seed_array[loc_inext].wrapping_sub(self.seed_array[loc_inextp]);
        if ret_val == MBIG {
            ret_val -= 1;
        }
        if ret_val < 0 {
            ret_val = ret_val.wrapping_add(MBIG);
        }
        self.seed_array[loc_inext] = ret_val;
        self.inext = loc_inext;
        self.inextp = loc_inextp;
        ret_val
    }

    /// `Random.NextBytes` filling 8 bytes, interpreted as a little-endian `ulong`.
    pub const fn next_u64(&mut self) -> u64 {
        let mut result = 0u64;
        let mut i = 0;
        while i < 8 {
            let byte = self.internal_sample() as u8;
            result |= (byte as u64) << (8 * i);
            i += 1;
        }
        result
    }

    /// `Random.Next(maxValue)`
    pub fn next_max(&mut self, max_value: i32) -> i32 {
        let sample = self.internal_sample() as f64 * (1.0 / MBIG as f64);
        (sample * max_value as f64) as i32
    }
}
