pub use pumpkin_data::carver::{
    HeightProvider, TrapezoidHeightProvider, UniformHeightProvider,
    VeryBiasedToBottomHeightProvider,
};
use pumpkin_util::random::{RandomGenerator, RandomImpl};

pub fn sample(
    p: &HeightProvider,
    random: &mut RandomGenerator,
    min_y: i8,
    height: u16,
    sea_level: i32,
) -> i32 {
    let resolve = |anchor: &pumpkin_util::y_offset::YOffset| {
        anchor.resolve_y(i16::from(min_y), height, sea_level)
    };
    match p {
        HeightProvider::Constant(anchor) => resolve(anchor),
        HeightProvider::Uniform(p) => {
            let min = resolve(&p.min_inclusive);
            let max = resolve(&p.max_inclusive);
            if min > max {
                min
            } else {
                random.next_inbetween_i32(min, max)
            }
        }
        HeightProvider::Trapezoid(p) => {
            let i = resolve(&p.min_inclusive);
            let j = resolve(&p.max_inclusive);
            if i > j {
                return i;
            }
            let plateau = p.plateau.unwrap_or(0);
            let k = j - i;
            if plateau >= k {
                random.next_inbetween_i32(i, j)
            } else {
                let l = (k - plateau) / 2;
                let m = k - l;
                i + random.next_inbetween_i32(0, m) + random.next_inbetween_i32(0, l)
            }
        }
        HeightProvider::VeryBiasedToBottom(p) => {
            let min = resolve(&p.min_inclusive);
            let max = resolve(&p.max_inclusive);
            let inner = p.inner.map_or(1, std::num::NonZero::get) as i32;
            if max - min - inner < 0 {
                min
            } else {
                let upper_inclusive = random.next_inbetween_i32(min + inner, max);
                let biased_upper_inclusive = random.next_inbetween_i32(min, upper_inclusive - 1);
                random.next_inbetween_i32(min, biased_upper_inclusive - 1 + inner)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_util::random::xoroshiro128::Xoroshiro;
    use pumpkin_util::y_offset::{AboveBottom, BelowTop, YOffset};
    use std::num::NonZero;

    #[test]
    fn very_biased_to_bottom_parity() {
        let mut random = RandomGenerator::Xoroshiro(Xoroshiro::from_seed(12345));
        let provider = HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider {
            min_inclusive: YOffset::AboveBottom(AboveBottom { above_bottom: 10 }),
            max_inclusive: YOffset::BelowTop(BelowTop { below_top: 10 }),
            inner: NonZero::new(8),
        });

        // Nether height 128, min_y 0 -> min = 10, max = 118
        for _ in 0..1000 {
            let y = sample(&provider, &mut random, 0, 128, 32);
            assert!((10..=118).contains(&y), "y out of bounds: {y}");
        }

        // Empty height range
        let empty_provider = HeightProvider::VeryBiasedToBottom(VeryBiasedToBottomHeightProvider {
            min_inclusive: YOffset::AboveBottom(AboveBottom { above_bottom: 100 }),
            max_inclusive: YOffset::AboveBottom(AboveBottom { above_bottom: 10 }),
            inner: NonZero::new(8),
        });
        assert_eq!(sample(&empty_provider, &mut random, 0, 128, 32), 100);
    }
}
