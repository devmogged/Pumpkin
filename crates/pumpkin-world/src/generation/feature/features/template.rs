use std::sync::Arc;

use pumpkin_data::{BlockState, BlockStateId, Rotation};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    random::{RandomGenerator, RandomImpl},
};

use crate::generation::{
    proto_chunk::GenerationCache,
    structure::template::{BlockPlacer, StructureProcessor, get_template, place_template},
};

/// Vanilla `TemplateFeature`: places a structure template centred on the origin.
pub struct TemplateFeature {
    pub templates: Vec<TemplateEntry>,
    pub total_weight: i32,
    pub processors: Arc<[StructureProcessor]>,
}

pub struct TemplateEntry {
    pub id: &'static str,
    pub rotations: Vec<Rotation>,
    pub weight: i32,
}

struct CachePlacer<'a, T>(&'a mut T);

impl<T: GenerationCache> BlockPlacer for CachePlacer<'_, T> {
    fn get_block_state(&self, pos: &Vector3<i32>) -> BlockStateId {
        GenerationCache::get_block_state(self.0, pos)
    }

    fn set_block_state(&mut self, pos: &Vector3<i32>, state: &BlockState) {
        GenerationCache::set_block_state(self.0, pos, state);
    }

    fn add_block_entity(&mut self, nbt: NbtCompound) {
        if let (Some(x), Some(y), Some(z)) = (nbt.get_int("x"), nbt.get_int("y"), nbt.get_int("z"))
        {
            GenerationCache::add_block_entity(self.0, &Vector3::new(x, y, z), nbt);
        }
    }
}

impl TemplateFeature {
    pub fn generate<T: GenerationCache>(
        &self,
        chunk: &mut T,
        random: &mut RandomGenerator,
        origin: BlockPos,
    ) -> bool {
        self.place(&mut CachePlacer(chunk), random, origin)
    }

    fn place(
        &self,
        placer: &mut impl BlockPlacer,
        random: &mut RandomGenerator,
        origin: BlockPos,
    ) -> bool {
        // WeightedList.getRandomOrThrow, then Util.getRandom over the rotations.
        let mut selection = random.next_bounded_i32(self.total_weight);
        let Some(entry) = self.templates.iter().find(|entry| {
            selection -= entry.weight;
            selection < 0
        }) else {
            return false;
        };
        let rotation =
            entry.rotations[random.next_bounded_i32(entry.rotations.len() as i32) as usize];
        let Some(template) = get_template(entry.id) else {
            return false;
        };
        // StructurePlaceSettings.getRandomPalette rolls the feature random even for one palette.
        random.next_bounded_i32(template.palettes.len().max(1) as i32);

        // Vanilla rotates around the corner it places at, after shifting that corner back by half
        // the template's X and Z size. Pumpkin rotates within the template's bounds, so cancel
        // that shift to land on the same blocks.
        let corner = rotation.transform_pos(Vector3::new(0, 0, 0), template.size);
        place_template(
            placer,
            &template,
            origin.0 - corner,
            (-(template.size.x / 2), -(template.size.z / 2)),
            rotation,
            false,
            true,
            &self.processors,
            None,
        );
        !template.blocks.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use pumpkin_util::random::legacy_rand::LegacyRand;

    use super::*;

    #[derive(Default)]
    struct Collect(BTreeSet<(i32, i32, i32)>);

    impl BlockPlacer for Collect {
        fn get_block_state(&self, _pos: &Vector3<i32>) -> BlockStateId {
            BlockStateId::AIR
        }
        fn set_block_state(&mut self, pos: &Vector3<i32>, _state: &BlockState) {
            self.0.insert((pos.x, pos.y, pos.z));
        }
        fn add_block_entity(&mut self, _nbt: NbtCompound) {}
    }

    /// Lands on the same blocks as vanilla, which offsets by half the size along the rotated
    /// west and north directions, then rotates each block around that corner.
    #[test]
    fn places_where_vanilla_does() {
        const ID: &str = "minecraft:desert_well/well";
        let template = get_template(ID).expect("the desert well template is embedded");
        let origin = Vector3::new(100, 64, -37);
        let (half_x, half_z) = (template.size.x / 2, template.size.z / 2);
        for (rotation, west, north) in [
            (Rotation::None, (-1, 0), (0, -1)),
            (Rotation::Clockwise90, (0, -1), (1, 0)),
            (Rotation::Rotate180, (1, 0), (0, 1)),
            (Rotation::CounterClockwise90, (0, 1), (-1, 0)),
        ] {
            let corner = (
                origin.x + west.0 * half_x + north.0 * half_z,
                origin.z + west.1 * half_x + north.1 * half_z,
            );
            let expected: BTreeSet<_> = template
                .blocks
                .iter()
                .filter(|b| {
                    let name = &template.palette[b.state as usize].name;
                    name != "minecraft:structure_void" && name != "minecraft:structure_block"
                })
                .map(|b| {
                    let (x, z) = match rotation {
                        Rotation::None => (b.pos.x, b.pos.z),
                        Rotation::Clockwise90 => (-b.pos.z, b.pos.x),
                        Rotation::Rotate180 => (-b.pos.x, -b.pos.z),
                        Rotation::CounterClockwise90 => (b.pos.z, -b.pos.x),
                    };
                    (corner.0 + x, origin.y + b.pos.y, corner.1 + z)
                })
                .collect();

            let feature = TemplateFeature {
                templates: vec![TemplateEntry {
                    id: ID,
                    rotations: vec![rotation],
                    weight: 1,
                }],
                total_weight: 1,
                processors: Arc::from([]),
            };
            let mut placed = Collect::default();
            let mut random = RandomGenerator::Legacy(LegacyRand::from_seed(0));
            assert!(feature.place(&mut placed, &mut random, BlockPos(origin)));
            assert_eq!(placed.0, expected, "{rotation:?}");
        }
    }
}
