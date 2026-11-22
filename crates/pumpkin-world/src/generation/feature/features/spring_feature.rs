use std::collections::HashSet;

use pumpkin_data::{
    Block, BlockDirection, BlockId, BlockState,
    tag::{RegistryKey, get_registry_key_tags},
};
use pumpkin_util::version::JavaMinecraftVersion;
use pumpkin_util::{math::position::BlockPos, random::RandomGenerator};

use crate::generation::proto_chunk::GenerationCache;
use crate::world::WorldPortalExt;

pub struct SpringFeatureFeature {
    pub state: &'static BlockState,
    pub requires_block_below: bool,
    pub rock_count: i32,
    pub hole_count: i32,
    pub valid_blocks: BlockWrapper,
}

pub enum BlockWrapper {
    Single(String),
    Multi(Vec<String>),
}

impl BlockWrapper {
    fn entries(&self) -> &[String] {
        match self {
            Self::Single(entry) => std::slice::from_ref(entry),
            Self::Multi(entries) => entries,
        }
    }

    fn tag_ids(tag: &str) -> &'static [u16] {
        let tags = get_registry_key_tags(JavaMinecraftVersion::V_26_3, RegistryKey::Block);
        let found = if tag.contains(':') {
            tags.and_then(|tags| tags.get(tag))
        } else {
            tags.and_then(|tags| tags.get(format!("minecraft:{tag}").as_str()))
        };
        found.map_or(&[], |tag| tag.1)
    }

    /// Resolves block names and `#` block tags to block ids.
    #[must_use]
    pub fn block_ids(&self) -> HashSet<BlockId> {
        let mut ids = HashSet::new();
        for entry in self.entries() {
            if let Some(tag) = entry.strip_prefix('#') {
                ids.extend(Self::tag_ids(tag).iter().filter_map(|&id| BlockId::new(id)));
            } else if let Some(block) = Block::from_name(entry) {
                ids.insert(block.id);
            }
        }
        ids
    }

    fn contains(&self, block: &Block) -> bool {
        self.entries().iter().any(|entry| {
            entry.strip_prefix('#').map_or_else(
                || Block::from_name(entry).is_some_and(|entry| entry.id == block.id),
                |tag| Self::tag_ids(tag).contains(&block.id.as_u16()),
            )
        })
    }
}

impl SpringFeatureFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        _block_registry: &dyn WorldPortalExt,
        chunk: &mut T,
        _random: &mut RandomGenerator,
        pos: BlockPos,
    ) -> bool {
        let valid_blocks = &self.valid_blocks;

        if !valid_blocks.contains(GenerationCache::get_block_state(chunk, &pos.up().0).to_block()) {
            return false;
        }
        if self.requires_block_below
            && !valid_blocks.contains(
                GenerationCache::get_block_state(
                    chunk,
                    &pos.offset(BlockDirection::Down.to_offset()).0,
                )
                .to_block(),
            )
        {
            return false;
        }
        let state = GenerationCache::get_block_state(chunk, &pos.0);
        if !state.to_state().is_air() && !valid_blocks.contains(state.to_block()) {
            return false;
        }

        let mut valid = 0;
        if valid_blocks.contains(
            GenerationCache::get_block_state(
                chunk,
                &pos.offset(BlockDirection::West.to_offset()).0,
            )
            .to_block(),
        ) {
            valid += 1;
        }
        if valid_blocks.contains(
            GenerationCache::get_block_state(
                chunk,
                &pos.offset(BlockDirection::East.to_offset()).0,
            )
            .to_block(),
        ) {
            valid += 1;
        }
        if valid_blocks.contains(
            GenerationCache::get_block_state(
                chunk,
                &pos.offset(BlockDirection::North.to_offset()).0,
            )
            .to_block(),
        ) {
            valid += 1;
        }
        if valid_blocks.contains(
            GenerationCache::get_block_state(
                chunk,
                &pos.offset(BlockDirection::South.to_offset()).0,
            )
            .to_block(),
        ) {
            valid += 1;
        }
        if valid_blocks.contains(
            GenerationCache::get_block_state(
                chunk,
                &pos.offset(BlockDirection::Down.to_offset()).0,
            )
            .to_block(),
        ) {
            valid += 1;
        }
        let mut air = 0;
        if chunk.is_air(&pos.offset(BlockDirection::West.to_offset()).0) {
            air += 1;
        }
        if chunk.is_air(&pos.offset(BlockDirection::East.to_offset()).0) {
            air += 1;
        }
        if chunk.is_air(&pos.offset(BlockDirection::North.to_offset()).0) {
            air += 1;
        }
        if chunk.is_air(&pos.offset(BlockDirection::South.to_offset()).0) {
            air += 1;
        }
        if chunk.is_air(&pos.offset(BlockDirection::Down.to_offset()).0) {
            air += 1;
        }
        if valid == self.rock_count && air == self.hole_count {
            chunk.set_block_state(&pos.0, self.state);
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_data::Block;

    use super::BlockWrapper;

    #[test]
    fn block_tags_resolve_to_member_blocks() {
        let cannot_replace = BlockWrapper::Single("#minecraft:features_cannot_replace".into());
        assert!(cannot_replace.block_ids().contains(&Block::BEDROCK.id));
        assert!(cannot_replace.contains(&Block::BEDROCK));
        assert!(!cannot_replace.contains(&Block::STONE));
    }
}
