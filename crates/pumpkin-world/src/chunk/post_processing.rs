use pumpkin_data::{BlockId, BlockState};
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::position::BlockPos;

/// Generation updates in section order, with packed local positions in insertion order.
#[derive(Clone, Default)]
pub struct PostProcessing(Vec<Vec<i16>>);

impl PostProcessing {
    pub(crate) fn mark(&mut self, pos: BlockPos, min_y: i32, height: u16) {
        let y = pos.0.y - min_y;
        if !(0..i32::from(height)).contains(&y) {
            return;
        }
        let section = (y >> 4) as usize;
        if section >= self.0.len() {
            self.0.resize_with(section + 1, Vec::new);
        }
        self.0[section]
            .push(((pos.0.x & 15) | ((pos.0.y & 15) << 4) | ((pos.0.z & 15) << 8)) as i16);
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn positions(
        self,
        chunk_x: i32,
        chunk_z: i32,
        min_y: i32,
    ) -> impl Iterator<Item = BlockPos> {
        self.0
            .into_iter()
            .enumerate()
            .flat_map(move |(section, offsets)| {
                offsets.into_iter().map(move |offset| {
                    let offset = i32::from(offset);
                    BlockPos::new(
                        (chunk_x << 4) + (offset & 15),
                        min_y + (section as i32 * 16) + ((offset >> 4) & 15),
                        (chunk_z << 4) + ((offset >> 8) & 15),
                    )
                })
            })
    }

    pub(crate) fn from_nbt(tag: Option<&[NbtTag]>, section_count: usize) -> Self {
        let mut sections = tag
            .unwrap_or_default()
            .iter()
            .take(section_count)
            .map(|tag| {
                if let NbtTag::List(offsets) = tag {
                    offsets
                        .iter()
                        .filter_map(|tag| match tag {
                            NbtTag::Short(offset) if (0..4096).contains(offset) => Some(*offset),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                } else {
                    Vec::new()
                }
            })
            .collect::<Vec<_>>();
        while sections.last().is_some_and(Vec::is_empty) {
            sections.pop();
        }
        Self(sections)
    }

    pub(crate) fn to_nbt(&self, section_count: usize) -> Vec<NbtTag> {
        (0..section_count)
            .map(|section| {
                NbtTag::List(
                    self.0
                        .get(section)
                        .into_iter()
                        .flatten()
                        .copied()
                        .map(NbtTag::Short)
                        .collect(),
                )
            })
            .collect()
    }
}

pub(crate) fn post_process_pos(state: &BlockState, pos: BlockPos) -> Option<BlockPos> {
    match state.id.to_block_id() {
        BlockId::BROWN_MUSHROOM | BlockId::RED_MUSHROOM => Some(pos),
        BlockId::SOUL_SAND | BlockId::MAGMA_BLOCK => Some(pos.up()),
        _ => None,
    }
}
