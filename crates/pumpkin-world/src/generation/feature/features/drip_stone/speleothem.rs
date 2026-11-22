//! Java's `SpeleothemUtils` and `Column`, shared by the configurable dripstone features.

use pumpkin_data::{
    Block, BlockDirection, BlockId, BlockState, BlockStateId, Property, PropertyKey,
    block_properties::{SpeleothemThickness, VerticalDirection},
};
use std::collections::HashSet;

use pumpkin_util::math::{cos, position::BlockPos, sin};

use crate::generation::proto_chunk::GenerationCache;

/// A vertical run of empty blocks bounded by an optional floor and ceiling (Java's `Column`).
#[derive(Clone, Copy)]
pub struct Column {
    pub floor: Option<i32>,
    pub ceiling: Option<i32>,
}

impl Column {
    /// `Column.getHeight`: only a bounded range has a height.
    pub fn height(self) -> Option<i32> {
        Some(self.ceiling? - self.floor? - 1)
    }

    /// `Column.scan`.
    pub fn scan<T: GenerationCache>(
        chunk: &T,
        pos: BlockPos,
        search_range: i32,
        inside: impl Fn(BlockStateId) -> bool,
        edge: impl Fn(BlockStateId) -> bool,
    ) -> Option<Self> {
        if !inside(state_at(chunk, pos)) {
            return None;
        }
        let scan = |step: i32| {
            let mut y = pos.0.y;
            let mut i = 1;
            while i < search_range && inside(state_at(chunk, BlockPos::new(pos.0.x, y, pos.0.z))) {
                y += step;
                i += 1;
            }
            edge(state_at(chunk, BlockPos::new(pos.0.x, y, pos.0.z))).then_some(y)
        };
        let ceiling = scan(1);
        let floor = scan(-1);
        Some(Self { floor, ceiling })
    }
}

/// `GenerationCache::get_block_state`, named to avoid the `BlockAccessor` method.
pub fn state_at<T: GenerationCache>(chunk: &T, pos: BlockPos) -> BlockStateId {
    GenerationCache::get_block_state(chunk, &pos.0)
}

pub fn is_water(state: BlockStateId) -> bool {
    let block = state.to_block_id();
    block == BlockId::WATER || block == BlockId::BUBBLE_COLUMN || state.is_waterlogged()
}

pub fn is_empty_or_water(state: BlockStateId) -> bool {
    state.to_state().is_air() || state.to_block_id() == BlockId::WATER
}

pub fn is_neither_empty_nor_water(state: BlockStateId) -> bool {
    !is_empty_or_water(state)
}

pub fn is_empty_or_water_or_lava(state: BlockStateId) -> bool {
    is_empty_or_water(state) || state.to_block_id() == BlockId::LAVA
}

pub fn is_base(state: BlockStateId, base: &Block, replaceable: &HashSet<BlockId>) -> bool {
    let block = state.to_block_id();
    block == base.id || replaceable.contains(&block)
}

pub fn is_base_or_lava(state: BlockStateId, base: &Block, replaceable: &HashSet<BlockId>) -> bool {
    is_base(state, base, replaceable) || state.to_block_id() == BlockId::LAVA
}

/// `SpeleothemUtils.getSpeleothemHeight`.
pub fn speleothem_height(distance: f64, radius: f64, scale: f64, bluntness: f64) -> f64 {
    let distance = distance.max(bluntness);
    let r = distance / radius * 0.384;
    let part1 = 0.75 * r.powf(1.333_333_333_333_333_3);
    let part2 = r.powf(0.666_666_666_666_666_6);
    let part3 = 0.333_333_333_333_333_3 * r.ln();
    let relative = (scale * (part1 - part2 - part3)).max(0.0);
    relative / 0.384 * radius
}

/// `SpeleothemUtils.isCircleMostlyEmbeddedInStone`.
pub fn is_circle_mostly_embedded_in_stone<T: GenerationCache>(
    chunk: &T,
    center: BlockPos,
    radius: i32,
) -> bool {
    if is_empty_or_water_or_lava(state_at(chunk, center)) {
        return false;
    }
    let increment = 6.0f32 / radius as f32;
    let mut angle = 0.0f32;
    while angle < std::f64::consts::TAU as f32 {
        let dx = (cos(angle) * radius as f32) as i32;
        let dz = (sin(angle) * radius as f32) as i32;
        if is_empty_or_water_or_lava(state_at(chunk, center.add(dx, 0, dz))) {
            return false;
        }
        angle += increment;
    }
    true
}

/// `SpeleothemUtils.placeBaseBlockIfPossible`.
pub fn place_base_block_if_possible<T: GenerationCache>(
    chunk: &mut T,
    pos: BlockPos,
    base: &Block,
    replaceable: &HashSet<BlockId>,
) -> bool {
    let block = state_at(chunk, pos).to_block_id();
    if replaceable.contains(&block) {
        chunk.set_block_state(&pos.0, base.default_state);
        true
    } else {
        false
    }
}

fn pointed_state(
    pointed: &Block,
    direction: BlockDirection,
    thickness: SpeleothemThickness,
    waterlogged: bool,
) -> &'static BlockState {
    let Some(properties) = pointed.properties(pointed.default_state.id) else {
        return pointed.default_state;
    };
    let vertical = match direction {
        BlockDirection::Down => VerticalDirection::Down,
        _ => VerticalDirection::Up,
    };
    let mut properties = properties.to_props();
    let values = [
        (PropertyKey::VerticalDirection, vertical.to_value()),
        (PropertyKey::Thickness, thickness.to_value()),
        (
            PropertyKey::Waterlogged,
            if waterlogged { "true" } else { "false" },
        ),
    ];
    for (key, value) in values {
        if let Some(property) = properties.iter_mut().find(|property| property.key() == key)
            && let Some(updated) = Property::from_key_value_str(key, value)
        {
            *property = updated;
        }
    }
    BlockState::from_id(pointed.from_properties(&properties).to_state_id(pointed))
}

/// `SpeleothemUtils.growSpeleothem`.
#[expect(clippy::too_many_arguments)]
pub fn grow_speleothem<T: GenerationCache>(
    chunk: &mut T,
    start: BlockPos,
    tip_direction: BlockDirection,
    height: i32,
    merged_tip: bool,
    base: &Block,
    pointed: &Block,
    replaceable: &HashSet<BlockId>,
) {
    let behind = start.offset(tip_direction.opposite().to_offset());
    if !is_base(state_at(chunk, behind), base, replaceable) {
        return;
    }
    let mut thicknesses = Vec::with_capacity(height.max(0) as usize);
    if height >= 3 {
        thicknesses.push(SpeleothemThickness::Base);
        thicknesses.extend((0..height - 3).map(|_| SpeleothemThickness::Middle));
    }
    if height >= 2 {
        thicknesses.push(SpeleothemThickness::Frustum);
    }
    if height >= 1 {
        thicknesses.push(if merged_tip {
            SpeleothemThickness::TipMerge
        } else {
            SpeleothemThickness::Tip
        });
    }
    let mut pos = start;
    for thickness in thicknesses {
        let waterlogged = is_water(state_at(chunk, pos));
        let state = pointed_state(pointed, tip_direction, thickness, waterlogged);
        chunk.set_block_state(&pos.0, state);
        pos = pos.offset(tip_direction.to_offset());
    }
}
