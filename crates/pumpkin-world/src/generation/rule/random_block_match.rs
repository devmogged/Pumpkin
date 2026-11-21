use pumpkin_data::{BlockId, BlockStateId};
use pumpkin_util::random::{RandomGenerator, RandomImpl};

pub struct RandomBlockMatchRuleTest {
    pub block: BlockId,
    pub probability: f32,
}

impl RandomBlockMatchRuleTest {
    pub fn test(&self, state: BlockStateId, random: &mut RandomGenerator) -> bool {
        self.block == state.to_block_id() && random.next_f32() < self.probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::Block;
    use pumpkin_util::random::legacy_rand::LegacyRand;

    #[test]
    fn placement_rng_matches_block_identity() {
        let rule = RandomBlockMatchRuleTest {
            block: Block::OAK_LOG.id,
            probability: 1.0,
        };
        let mut random = RandomGenerator::Legacy(LegacyRand::from_seed(42));
        let mut control = RandomGenerator::Legacy(LegacyRand::from_seed(42));
        assert!(!rule.test(Block::STONE.default_state.id, &mut random));
        assert_eq!(random.next_i32(), control.next_i32());
        assert!(rule.test(Block::OAK_LOG.default_state.id, &mut random));
        control.next_f32();
        assert_eq!(random.next_i32(), control.next_i32());
    }
}
