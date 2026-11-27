//! Vanilla block transformers and copper families registered by content mods.
pub use crate::generated_block_transformer::*;

use crate::{Block, BlockId, BlockStateId, content, world::WorldEvent};

#[must_use]
pub fn get_next_weathering(id: BlockId) -> Option<BlockId> {
    crate::generated_block_transformer::get_next_weathering(id).or_else(|| {
        let (family, stage) = content::copper_family(id)?;
        (stage < 3).then(|| family[stage + 1])
    })
}

#[must_use]
pub fn get_previous_weathering(id: BlockId) -> Option<BlockId> {
    crate::generated_block_transformer::get_previous_weathering(id).or_else(|| {
        let (family, stage) = content::copper_family(id)?;
        (1..4).contains(&stage).then(|| family[stage - 1])
    })
}

#[must_use]
pub fn get_waxed(id: BlockId) -> Option<BlockId> {
    crate::generated_block_transformer::get_waxed(id).or_else(|| {
        let (family, stage) = content::copper_family(id)?;
        (stage < 4).then(|| family[stage + 4])
    })
}

#[must_use]
pub fn get_unwaxed(id: BlockId) -> Option<BlockId> {
    crate::generated_block_transformer::get_unwaxed(id).or_else(|| {
        let (family, stage) = content::copper_family(id)?;
        (stage >= 4).then(|| family[stage - 4])
    })
}

/// Extend the vanilla axe's scraping and unwaxing rules to registered copper families.
#[must_use]
pub fn content_axe_transform(
    transformer: &BlockTransformer,
    block: &Block,
    state: BlockStateId,
) -> Option<TransformResult> {
    if !std::ptr::eq(transformer, &AXE) {
        return None;
    }
    let (family, stage) = content::copper_family(block.id)?;
    let (target, particle) = match stage {
        1..4 => (family[stage - 1], WorldEvent::ParticlesScrape),
        4..8 => (family[stage - 4], WorldEvent::ParticlesWaxOff),
        _ => return None,
    };
    let entry = AXE
        .entries
        .iter()
        .find(|entry| entry.particle == Some(particle))?;
    let target_block = target.to_block();
    Some(TransformResult {
        new_state_id: target_block.with_properties_of(block, state),
        target_block,
        entry,
    })
}
