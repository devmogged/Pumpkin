use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use pumpkin_data::item_stack::ItemStack;
use pumpkin_data::{
    entity::EntityType,
    item::Item,
    sound::{Sound, SoundCategory},
};
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_protocol::codec::var_int::VarInt;

use crate::entity::ai::behavior::hunting::{self, Hunt};
use crate::entity::area_effect_cloud::AreaEffectCloudEntity;
use crate::entity::{
    Entity, EntityBase,
    ai::behavior::fleeing::{self, Flee},
    ai::goal::{
        creeper_ignite::CreeperIgniteGoal, look_around::RandomLookAroundGoal,
        look_at_entity::LookAtEntityGoal, melee_attack::MeleeAttackGoal, revenge::RevengeGoal,
        swim::SwimGoal, wander_around::WanderAroundGoal,
    },
    ai::target_match::TargetMatch,
    mob::{Mob, MobEntity},
    player::Player,
};

const DEFAULT_FUSE_TIME: i32 = 30;
const DEFAULT_EXPLOSION_RADIUS: i32 = 3;

const CREEPER_FLEES: &[Flee] = &[
    Flee::new(3, TargetMatch::Type(&EntityType::OCELOT), 6.0, 1.0, 1.2),
    Flee::new(3, TargetMatch::Type(&EntityType::CAT), 6.0, 1.0, 1.2),
];

const CREEPER_HUNTS: &[Hunt] = &[Hunt::new(1, TargetMatch::Type(&EntityType::PLAYER), true)];

pub struct CreeperEntity {
    pub mob_entity: MobEntity,
    pub fuse_speed: AtomicI32,
    pub current_fuse_time: AtomicI32,
    pub last_fuse_time: AtomicI32,
    pub fuse_time: AtomicI32,
    pub explosion_radius: AtomicI32,
    pub ignited: AtomicBool,
    pub charged: AtomicBool,
}

impl CreeperEntity {
    pub fn new(entity: Entity) -> Arc<Self> {
        let mob_entity = MobEntity::new(entity);
        let entity = Self {
            mob_entity,
            fuse_speed: AtomicI32::new(-1),
            current_fuse_time: AtomicI32::new(0),
            last_fuse_time: AtomicI32::new(0),
            fuse_time: AtomicI32::new(DEFAULT_FUSE_TIME),
            explosion_radius: AtomicI32::new(DEFAULT_EXPLOSION_RADIUS),
            ignited: AtomicBool::new(false),
            charged: AtomicBool::new(false),
        };
        let mob_arc = Arc::new(entity);
        let mob_weak: Weak<dyn Mob> = {
            let mob_arc: Arc<dyn Mob> = mob_arc.clone();
            Arc::downgrade(&mob_arc)
        };

        {
            let goal_selector = &mob_arc.mob_entity.goals_selector;
            let target_selector = &mob_arc.mob_entity.target_selector;

            goal_selector.add_goal(1, Box::new(SwimGoal::default()));
            goal_selector.add_goal(
                2,
                Box::new(CreeperIgniteGoal::new(Arc::downgrade(&mob_arc))),
            );
            fleeing::apply(goal_selector, CREEPER_FLEES);
            goal_selector.add_goal(4, Box::new(MeleeAttackGoal::new(1.0, false)));
            goal_selector.add_goal(5, Box::new(WanderAroundGoal::new(0.8)));

            goal_selector.add_goal(
                6,
                LookAtEntityGoal::with_default(mob_weak, &EntityType::PLAYER, 8.0),
            );
            goal_selector.add_goal(6, Box::new(RandomLookAroundGoal::default()));

            hunting::apply(target_selector, &mob_arc.mob_entity, CREEPER_HUNTS);
            target_selector.add_goal(2, Box::new(RevengeGoal::new(true)));
        };

        mob_arc
    }

    pub fn set_fuse_speed(&self, speed: i32) {
        self.fuse_speed.store(speed, Ordering::Relaxed);
        self.mob_entity
            .living_entity
            .entity
            .set_synced_data(pumpkin_data::tracked_data::creeper::FUSE_ID, VarInt(speed));
    }

    pub fn explode(&self) {
        let entity = &self.mob_entity.living_entity.entity;
        let radius = self.explosion_radius.load(Ordering::Relaxed) as f32;
        let multiplier = if self.charged.load(Ordering::Relaxed) {
            2.0
        } else {
            1.0
        };
        self.mob_entity
            .living_entity
            .dead
            .store(true, Ordering::Relaxed);
        let world = entity.world.load();
        let pos = entity.pos.load();
        world.explode_from(
            self,
            pos,
            radius * multiplier,
            crate::world::ExplosionInteraction::Mob,
        );
        self.spawn_lingering_cloud();
        entity.remove();
    }

    // Vanilla Creeper.spawnLingeringCloud: preserve each active effect at 1/4 duration.
    fn spawn_lingering_cloud(&self) {
        let effects: Vec<_> = self
            .mob_entity
            .living_entity
            .active_effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|effect| {
                let duration = if effect.duration == -1 || effect.duration == 0 {
                    effect.duration
                } else {
                    ((effect.duration as f32) * 0.25).floor().max(1.0) as i32
                };
                (
                    effect.effect_type,
                    duration,
                    effect.amplifier,
                    effect.ambient,
                    effect.show_particles,
                    effect.show_icon,
                )
            })
            .collect();
        if effects.is_empty() {
            return;
        }

        let entity = &self.mob_entity.living_entity.entity;
        let world = entity.world.load();
        let cloud_entity = Entity::new(
            world.clone(),
            entity.pos.load(),
            &EntityType::AREA_EFFECT_CLOUD,
        );
        let cloud = AreaEffectCloudEntity::create(
            cloud_entity,
            ItemStack::new(0, &Item::GLASS_BOTTLE),
            effects,
            300,  // duration
            2.5,  // initial radius
            20,   // reapplication delay
            10,   // wait time
            -0.5, // radius on use
            0,    // duration on use
        );
        world.spawn_entity(cloud);
    }
}

impl Mob for CreeperEntity {
    // Vanilla Creeper.doHurtTarget acknowledges the attack without melee damage.
    fn do_hurt_target(&self, _target: &dyn EntityBase) -> bool {
        true
    }

    fn cause_fall_damage(
        &self,
        caller: &dyn EntityBase,
        fall_distance: f32,
        damage_modifier: f32,
    ) -> bool {
        let damaged =
            self.mob_entity
                .living_entity
                .cause_fall_damage(caller, fall_distance, damage_modifier);
        let max_swell = self.fuse_time.load(Ordering::Relaxed) - 5;
        let swell = self.current_fuse_time.load(Ordering::Relaxed) + (fall_distance * 1.5) as i32;
        self.current_fuse_time
            .store(swell.min(max_swell), Ordering::Relaxed);
        damaged
    }

    fn mob_write_nbt(&self, nbt: &mut NbtCompound) {
        nbt.put_bool("powered", self.charged.load(Ordering::Relaxed));
        nbt.put_short("Fuse", self.fuse_time.load(Ordering::Relaxed) as i16);
        nbt.put_byte(
            "ExplosionRadius",
            self.explosion_radius.load(Ordering::Relaxed) as i8,
        );
        nbt.put_bool("ignited", self.ignited.load(Ordering::Relaxed));
    }

    fn mob_read_nbt(&self, nbt: &NbtCompound) {
        if let Some(powered) = nbt.get_bool("powered") {
            self.charged.store(powered, Ordering::Relaxed);
        }
        if let Some(fuse) = nbt.get_short("Fuse") {
            self.fuse_time.store(i32::from(fuse), Ordering::Relaxed);
        }
        if let Some(radius) = nbt.get_byte("ExplosionRadius") {
            self.explosion_radius
                .store(i32::from(radius), Ordering::Relaxed);
        }
        if let Some(ignited) = nbt.get_bool("ignited") {
            self.ignited.store(ignited, Ordering::Relaxed);
        }
    }

    fn get_mob_entity(&self) -> &MobEntity {
        &self.mob_entity
    }

    fn mob_metadata(&self) -> Option<crate::entity::mob::MobMetadata> {
        let mut meta = crate::entity::mob::MobMetadata::new();
        if self.is_charged() {
            meta = meta.charged(true);
        }
        if self.is_ignited() {
            meta = meta.ignited(true);
        }
        let fuse = self.fuse_speed.load(Ordering::Relaxed);
        if fuse != -1 {
            meta = meta.fuse_speed(fuse);
        }
        Some(meta)
    }

    fn mob_on_lightning_strike(
        &self,
        caller: &dyn EntityBase,
        lightning: &crate::entity::lightning::LightningBoltEntity,
    ) {
        self.charged.store(true, Ordering::Relaxed);
        self.mob_entity
            .living_entity
            .entity
            .set_synced_data(pumpkin_data::tracked_data::creeper::CHARGED, true);
        self.mob_entity
            .living_entity
            .on_lightning_strike(caller, lightning);
    }

    fn mob_tick(&self, _caller: &dyn EntityBase) {
        let entity = &self.mob_entity.living_entity.entity;
        if !entity.is_alive() {
            return;
        }

        self.last_fuse_time.store(
            self.current_fuse_time.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );

        if self.ignited.load(Ordering::Relaxed) {
            self.set_fuse_speed(1);
        }

        let fuse_speed = self.fuse_speed.load(Ordering::Relaxed);
        let current = self.current_fuse_time.load(Ordering::Relaxed);

        if fuse_speed > 0 && current == 0 {
            let world = entity.world.load();
            world.play_sound_fine(
                Sound::EntityCreeperPrimed,
                SoundCategory::Hostile,
                &entity.pos.load(),
                1.0,
                0.5,
            );
        }

        let fuse_time = self.fuse_time.load(Ordering::Relaxed);
        let new_fuse = (current + fuse_speed).max(0);
        self.current_fuse_time.store(new_fuse, Ordering::Relaxed);

        if new_fuse >= fuse_time {
            self.current_fuse_time.store(fuse_time, Ordering::Relaxed);
            self.explode();
        }
    }

    fn mob_interact(&self, player: &Arc<Player>, item_stack: &mut ItemStack) -> bool {
        if item_stack.item.id != Item::FLINT_AND_STEEL.id {
            return self
                .mob_entity
                .mob_interact(player, item_stack, self.can_be_leashed());
        }

        let entity = &self.mob_entity.living_entity.entity;
        let world = entity.world.load();
        let pos = entity.pos.load();

        world.play_sound_fine(
            Sound::ItemFlintandsteelUse,
            SoundCategory::Hostile,
            &pos,
            1.0,
            rand::random::<f32>() * 0.4 + 0.8,
        );

        self.ignited.store(true, Ordering::Relaxed);
        entity.set_synced_data(pumpkin_data::tracked_data::creeper::IS_IGNITED, true);

        if player.gamemode.load() != pumpkin_util::GameMode::Creative {
            let _ = item_stack.damage_item(1);
        }

        true
    }
}

impl CreeperEntity {
    pub fn is_charged(&self) -> bool {
        self.charged.load(Ordering::Relaxed)
    }

    pub fn set_charged(&self, charged: bool) {
        self.charged.store(charged, Ordering::Relaxed);
        let entity = &self.mob_entity.living_entity.entity;
        entity.set_synced_data(pumpkin_data::tracked_data::creeper::CHARGED, charged);
        if let Some(meta) = self.mob_metadata() {
            entity.send_bedrock_actor_data(&meta.to_bedrock_actor_data());
        }
    }

    pub fn is_ignited(&self) -> bool {
        self.ignited.load(Ordering::Relaxed)
    }

    pub fn set_ignited(&self, ignited: bool) {
        self.ignited.store(ignited, Ordering::Relaxed);
        let entity = &self.mob_entity.living_entity.entity;
        entity.set_synced_data(pumpkin_data::tracked_data::creeper::IS_IGNITED, ignited);
        if let Some(meta) = self.mob_metadata() {
            entity.send_bedrock_actor_data(&meta.to_bedrock_actor_data());
        }
    }

    pub fn get_fuse(&self) -> i32 {
        self.fuse_time.load(Ordering::Relaxed)
    }

    pub fn set_fuse(&self, fuse: i32) {
        self.fuse_time.store(fuse, Ordering::Relaxed);
    }

    pub fn get_explosion_radius(&self) -> i32 {
        self.explosion_radius.load(Ordering::Relaxed)
    }

    pub fn set_explosion_radius(&self, radius: i32) {
        self.explosion_radius.store(radius, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entity::{ai::test_support::world, r#type::from_type};
    use pumpkin_data::{effect::StatusEffect, potion::Effect};
    use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};
    use uuid::Uuid;

    #[tokio::test]
    async fn creeper_cloud_only_spawns_with_active_effects_and_uses_vanilla_settings()
    -> Result<(), Box<dyn std::error::Error>> {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let creeper = CreeperEntity::new(Entity::new(
            world.clone(),
            Vector3::new(2.5, 151.0, 2.5),
            &EntityType::CREEPER,
        ));
        let bounds = BoundingBox::new(Vector3::new(0.0, 149.0, 0.0), Vector3::new(5.0, 155.0, 5.0));
        creeper.spawn_lingering_cloud();
        assert!(world.get_entities_at_box(&bounds).is_empty());

        creeper.mob_entity.living_entity.add_effect(Effect {
            effect_type: &StatusEffect::SPEED,
            duration: 200,
            amplifier: 1,
            ambient: false,
            show_particles: true,
            show_icon: true,
            blend: false,
        });
        creeper.spawn_lingering_cloud();
        let spawned = world.get_entities_at_box(&bounds);
        assert_eq!(spawned.len(), 1);
        let Some(cloud) = spawned[0]
            .cast_any()
            .downcast_ref::<AreaEffectCloudEntity>()
        else {
            panic!("an effect-bearing creeper should spawn an area effect cloud");
        };
        assert_eq!(
            *cloud
                .duration
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            300
        );
        assert_eq!(
            *cloud
                .wait_time
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            10
        );
        assert_eq!(
            *cloud
                .radius
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            2.5
        );
        assert_eq!(
            *cloud
                .radius_on_use
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            -0.5
        );
        let effects = cloud
            .effects
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].0.id, StatusEffect::SPEED.id);
        assert_eq!(
            effects[0].1, 50,
            "Java's potion-duration scale is one quarter"
        );
        assert_eq!(effects[0].2, 1);
        drop(effects);
        world.level.shutdown().await;
        Ok(())
    }

    #[tokio::test]
    async fn creeper_melee_attempt_cannot_damage_a_target() -> Result<(), Box<dyn std::error::Error>>
    {
        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        let creeper = from_type(
            &EntityType::CREEPER,
            Vector3::new(0.5, 64.0, 0.5),
            &world,
            Uuid::new_v4(),
        );
        let pig = from_type(
            &EntityType::PIG,
            Vector3::new(1.5, 64.0, 0.5),
            &world,
            Uuid::new_v4(),
        );
        let Some(creeper_mob) = creeper.get_mob() else {
            panic!("creeper entity should have Mob behavior");
        };
        let Some(pig_living) = pig.get_living_entity() else {
            panic!("pig entity should be LivingEntity");
        };
        let health_before = pig_living.health.load();
        assert!(creeper_mob.do_hurt_target(pig.as_ref()));
        assert_eq!(
            pig_living.health.load(),
            health_before,
            "vanilla Creeper.doHurtTarget succeeds without direct melee damage"
        );
        world.level.shutdown().await;
        Ok(())
    }
}
