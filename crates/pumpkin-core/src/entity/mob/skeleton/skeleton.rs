use pumpkin_data::BlockState;
use pumpkin_data::entity::EntityType;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::sound::Sound;
use pumpkin_data::tracked_data;
use pumpkin_data::world::WorldEvent;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

use crate::entity::{
    Entity, EntityBase,
    mob::{
        Mob, MobEntity,
        conversion::{ConversionParams, ConversionTracker, convert_to},
        equipment::RegionalDifficulty,
        skeleton::SkeletonEntityBase,
    },
};
use crate::world::World;

pub struct SkeletonEntity {
    entity: Arc<SkeletonEntityBase>,
    freezing: ConversionTracker,
}

impl SkeletonEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let entity = SkeletonEntityBase::new(entity);
        Arc::new(Self {
            entity,
            freezing: ConversionTracker::new(
                tracked_data::skeleton::DATA_STRAY_CONVERSION_ID,
                "FreezingTime",
                140,
                "StrayConversionTime",
                300,
            ),
        })
    }
}

impl Mob for SkeletonEntity {
    fn wants_to_pick_up(&self, stack: &ItemStack) -> bool {
        super::wants_to_pick_up(self, stack)
    }

    fn mob_play_step_sound(&self, _pos: &BlockPos, _state: &BlockState) {
        self.play_entity_sound(Sound::EntitySkeletonStep, 0.15, 1.0);
    }

    fn get_mob_entity(&self) -> &MobEntity {
        &self.entity.mob_entity
    }

    fn post_tick(&self) {
        let entity = self.get_entity();
        let afflicted = entity.is_in_powder_snow.load(Relaxed);
        self.freezing.tick(self, afflicted, || {
            let world = entity.world.load_full();
            let _ = convert_to(
                self,
                &EntityType::STRAY,
                ConversionParams::single(true, true),
                |converted| {
                    let converted_entity = converted.get_entity();
                    if !converted_entity.is_silent() {
                        world.sync_world_event(
                            WorldEvent::SoundSkeletonToStray,
                            converted_entity.block_pos.load(),
                            0,
                        );
                    }
                },
            );
        });
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        self.freezing
            .write_nbt(nbt, self.get_entity().is_in_powder_snow.load(Relaxed));
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        self.freezing.read_nbt(self.get_entity(), nbt);
    }

    fn populate_default_equipment_slots(
        &self,
        world: &Arc<World>,
        difficulty: &RegionalDifficulty,
    ) {
        self.entity
            .populate_default_equipment_slots(world, difficulty);
    }

    fn populate_default_equipment_enchantments(&self, difficulty: &RegionalDifficulty) {
        self.entity
            .populate_default_equipment_enchantments(difficulty);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::ai::test_support::world;
    use pumpkin_util::math::vector3::Vector3;

    #[tokio::test]
    async fn vanilla_skeleton_freezing_conversion_nbt() -> Result<(), Box<dyn std::error::Error>> {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let skeleton = SkeletonEntity::new(Entity::new(
            world.clone(),
            Vector3::new(8.0, 64.0, 8.0),
            &EntityType::SKELETON,
        ));
        let mut nbt = NbtCompound::new();
        skeleton.mob_write_nbt(&mut nbt);
        assert_eq!(nbt.get_int("FreezingTime"), Some(-1));
        assert_eq!(nbt.get_int("StrayConversionTime"), Some(-1));

        nbt.put_int("FreezingTime", 140);
        nbt.put_int("StrayConversionTime", 300);
        skeleton.mob_read_nbt(&nbt);
        let mut restored_nbt = NbtCompound::new();
        skeleton.mob_write_nbt(&mut restored_nbt);
        assert_eq!(restored_nbt.get_int("StrayConversionTime"), Some(300));
        world.level.shutdown().await;
        Ok(())
    }
}
