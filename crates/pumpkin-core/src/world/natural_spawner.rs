use crate::block::PathComputationType;
use crate::entity::EntityBase;
use crate::entity::mob::spawn::{EntitySpawnReason, finalize_spawn};
use crate::entity::r#type::{check_spawn_rules, from_type};
use crate::world::World;
use arc_swap::ArcSwap;
use dashmap::DashMap;
use pumpkin_data::biome::Spawner;
use pumpkin_data::chunk::Biome;
use pumpkin_data::dimension::Dimension;
use pumpkin_data::entity::{EntityType, MobCategory, SpawnLocation};
use pumpkin_data::tag::Block::MINECRAFT_PREVENT_MOB_SPAWNING_INSIDE;
use pumpkin_data::tag::Fluid::{MINECRAFT_LAVA, MINECRAFT_WATER};
use pumpkin_data::tag::Taggable;
use pumpkin_data::tag::WorldgenBiome::MINECRAFT_REDUCE_WATER_AMBIENT_SPAWNS;
use pumpkin_data::{Block, BlockDirection, BlockState, SpawnFloorPredicate};
use pumpkin_util::GameMode;
use pumpkin_util::math::get_section_cord;
use pumpkin_util::math::position::BlockPos;
use pumpkin_util::math::vector2::Vector2;
use pumpkin_util::math::vector3::Vector3;
use pumpkin_util::random::xoroshiro128::Xoroshiro;
use pumpkin_util::random::{RandomImpl, get_seed};
use pumpkin_world::chunk::{ChunkData, ChunkHeightmapType};
use pumpkin_world::generation::proto_chunk::GenerationCache;
use rand::{RngExt, rng};
use std::fmt;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use uuid::Uuid;

const MAGIC_NUMBER: i32 = 17 * 17;

/// A natural spawn entry an embedder adds to biome spawn pools, for a mob Pumpkin doesn't define.
#[derive(Debug)]
pub struct EmbedderSpawn {
    /// The embedder's id for the mob, handed back to `check` and `create`.
    pub id: &'static str,
    /// The vanilla type whose size, spawn costs, caps and despawn rules the mob uses.
    pub carrier: &'static EntityType,
    /// Native dimensions for the registered type when they differ from the carrier.
    pub dimensions: Option<pumpkin_util::math::boundingbox::EntityDimensions>,
    /// The pool it is in.
    pub category: &'static MobCategory,
    pub location: SpawnLocation,
    pub weight: i32,
    pub min_count: i32,
    pub max_count: i32,
    /// Also apply the carrier's vanilla spawn rules before `check`.
    pub carrier_rules: bool,
}

/// Hooks that let an embedder add mobs to the natural spawner.
pub struct EmbedderSpawns {
    /// Extra entries for a biome's pool of one category.
    pub entries: fn(&'static Biome, &'static MobCategory) -> &'static [EmbedderSpawn],
    /// The mob's own spawn rule at a position (vanilla `SpawnPlacements` predicate).
    pub check: fn(&Arc<World>, &'static EmbedderSpawn, &BlockPos, EntitySpawnReason) -> bool,
    /// Create the mob at a position without adding it to the world.
    pub create:
        fn(&Arc<World>, &'static EmbedderSpawn, Vector3<f64>) -> Option<Arc<dyn EntityBase>>,
    /// Called after insertion; returning true marks the group as claimed for later members.
    pub spawned: fn(
        &Arc<World>,
        &'static EmbedderSpawn,
        &dyn EntityBase,
        EntitySpawnReason,
        u64,
        bool,
    ) -> bool,
}

static NEXT_SPAWN_GROUP: AtomicU64 = AtomicU64::new(1);

static EMBEDDER_SPAWNS: OnceLock<EmbedderSpawns> = OnceLock::new();

/// Installs the embedder spawn hooks before worlds tick. Only the first call succeeds.
pub fn install_embedder_spawns(spawns: EmbedderSpawns) -> bool {
    EMBEDDER_SPAWNS.set(spawns).is_ok()
}

fn embedder_entries(
    biome: &'static Biome,
    category: &'static MobCategory,
) -> &'static [EmbedderSpawn] {
    EMBEDDER_SPAWNS
        .get()
        .map_or(&[], |spawns| (spawns.entries)(biome, category))
}

/// One entry of a biome's spawn pool: vanilla, or an embedder's.
#[derive(Clone, Copy)]
pub enum SpawnEntry {
    Vanilla(&'static Spawner, &'static EntityType),
    Embedder(&'static EmbedderSpawn),
}

impl SpawnEntry {
    fn spawn_bounds(self, position: Vector3<f64>) -> pumpkin_util::math::boundingbox::BoundingBox {
        if let Self::Embedder(spawn) = self
            && let Some(dimensions) = spawn.dimensions
        {
            return pumpkin_util::math::boundingbox::BoundingBox::new_from_pos(
                position.x,
                position.y,
                position.z,
                &dimensions,
            );
        }
        self.entity_type()
            .get_spawn_bounding_box(position.x, position.y, position.z)
    }

    #[must_use]
    pub const fn entity_type(self) -> &'static EntityType {
        match self {
            Self::Vanilla(_, entity_type) => entity_type,
            Self::Embedder(spawn) => spawn.carrier,
        }
    }

    const fn weight(self) -> i32 {
        match self {
            Self::Vanilla(spawner, _) => spawner.weight,
            Self::Embedder(spawn) => spawn.weight,
        }
    }

    const fn counts(self) -> (i32, i32) {
        match self {
            Self::Vanilla(spawner, _) => (spawner.min_count, spawner.max_count),
            Self::Embedder(spawn) => (spawn.min_count, spawn.max_count),
        }
    }

    const fn location(self) -> &'static SpawnLocation {
        match self {
            Self::Vanilla(_, entity_type) => &entity_type.spawn_restriction.location,
            Self::Embedder(spawn) => &spawn.location,
        }
    }

    fn create(self, world: &Arc<World>, position: Vector3<f64>) -> Option<Arc<dyn EntityBase>> {
        match self {
            Self::Vanilla(_, entity_type) => {
                Some(from_type(entity_type, position, world, Uuid::new_v4()))
            }
            Self::Embedder(spawn) => EMBEDDER_SPAWNS
                .get()
                .and_then(|spawns| (spawns.create)(world, spawn, position)),
        }
    }

    fn spawned(
        self,
        world: &Arc<World>,
        entity: &dyn EntityBase,
        reason: EntitySpawnReason,
        group: u64,
        claimed: &mut bool,
    ) {
        if let Self::Embedder(spawn) = self
            && let Some(hooks) = EMBEDDER_SPAWNS.get()
        {
            *claimed |= (hooks.spawned)(world, spawn, entity, reason, group, *claimed);
        }
    }

    /// Vanilla `SpawnPlacements.checkSpawnRules`, or the carrier's and the embedder's.
    fn check_rules(
        self,
        world: &Arc<World>,
        pos: &BlockPos,
        is_thundering: bool,
        reason: EntitySpawnReason,
    ) -> bool {
        match self {
            Self::Vanilla(_, entity_type) => {
                check_spawn_rules(entity_type, world, pos, is_thundering, reason)
            }
            Self::Embedder(spawn) => {
                (!spawn.carrier_rules
                    || check_spawn_rules(spawn.carrier, world, pos, is_thundering, reason))
                    && EMBEDDER_SPAWNS
                        .get()
                        .is_some_and(|spawns| (spawns.check)(world, spawn, pos, reason))
            }
        }
    }
}

/// A chunk generation spawn group member waiting for its chunk to join the world, so its spawn
/// rules read the generated blocks instead of an unloaded chunk.
struct PendingGenerationSpawn {
    world: Weak<World>,
    entry: SpawnEntry,
    group: u64,
    chunk: Vector2<i32>,
    /// The positions vanilla would try in order; the first that passes spawns.
    candidates: Vec<(Vector3<f64>, BlockPos)>,
    queued: i64,
}

static PENDING_GENERATION: Mutex<Vec<PendingGenerationSpawn>> = Mutex::new(Vec::new());

/// Give up on a chunk that never joins the world.
const PENDING_GENERATION_TICKS: i64 = 1200;

/// Finish chunk generation spawns whose chunks have joined `world`.
pub fn spawn_pending_generation(world: &Arc<World>) {
    let now = world.get_world_age();
    let ready = {
        let mut pending = PENDING_GENERATION
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if pending.is_empty() {
            return;
        }
        let mut ready = Vec::new();
        pending.retain_mut(|spawn| {
            let Some(owner) = spawn.world.upgrade() else {
                return false;
            };
            if !Arc::ptr_eq(&owner, world) {
                return true;
            }
            if world.level.is_chunk_loaded(&spawn.chunk) {
                ready.push((
                    spawn.entry,
                    spawn.group,
                    std::mem::take(&mut spawn.candidates),
                ));
                return false;
            }
            now - spawn.queued < PENDING_GENERATION_TICKS
        });
        ready
    };
    let mut groups = std::collections::HashMap::<u64, bool>::new();
    for (entry, group, candidates) in ready {
        for (position, check_pos) in candidates {
            if world.is_space_empty(entry.spawn_bounds(position))
                && entry.check_rules(world, &check_pos, false, EntitySpawnReason::ChunkGeneration)
                && let Some(entity) = entry.create(world, position)
            {
                entity
                    .get_entity()
                    .set_rotation(rand::random::<f32>() * 360.0, 0.0);
                if matches!(entry, SpawnEntry::Vanilla(..)) {
                    finalize_spawn(&entity, world, EntitySpawnReason::ChunkGeneration, None);
                }
                world.spawn_entity_non_save(entity.clone());
                entry.spawned(
                    world,
                    entity.as_ref(),
                    EntitySpawnReason::ChunkGeneration,
                    group,
                    groups.entry(group).or_default(),
                );
                break;
            }
        }
    }
}

/// Vanilla `WeightedRandomList.getRandom` over the biome's entries and the embedder's.
fn choose_weighted(
    vanilla: &'static [Spawner],
    embedder: &'static [EmbedderSpawn],
) -> Option<SpawnEntry> {
    let entries = vanilla
        .iter()
        .filter_map(|spawner| {
            let name = spawner
                .r#type
                .strip_prefix("minecraft:")
                .unwrap_or(spawner.r#type);
            EntityType::from_name(name).map(|entity_type| SpawnEntry::Vanilla(spawner, entity_type))
        })
        .chain(embedder.iter().map(SpawnEntry::Embedder))
        .filter(|entry| entry.weight() > 0);
    let total: i32 = entries.clone().map(SpawnEntry::weight).sum();
    if total <= 0 {
        return None;
    }
    let mut roll = rng().random_range(0..total);
    for entry in entries {
        roll -= entry.weight();
        if roll < 0 {
            return Some(entry);
        }
    }
    None
}

/// A biome's vanilla spawn pool of one category.
#[must_use]
pub fn pool(biome: &'static Biome, category: &'static MobCategory) -> &'static [Spawner] {
    match category.id {
        id if id == MobCategory::MONSTER.id => biome.spawners.monster,
        id if id == MobCategory::CREATURE.id => biome.spawners.creature,
        id if id == MobCategory::AMBIENT.id => biome.spawners.ambient,
        id if id == MobCategory::AXOLOTLS.id => biome.spawners.axolotls,
        id if id == MobCategory::UNDERGROUND_WATER_CREATURE.id => {
            biome.spawners.underground_water_creature
        }
        id if id == MobCategory::WATER_CREATURE.id => biome.spawners.water_creature,
        id if id == MobCategory::WATER_AMBIENT.id => biome.spawners.water_ambient,
        _ => biome.spawners.misc,
    }
}

pub(crate) const SPAWN_DISTANCE_CHUNK: u8 = 8;

pub struct MobCounts([AtomicI32; 8]);

impl Default for MobCounts {
    fn default() -> Self {
        Self(std::array::from_fn(|_| AtomicI32::new(0)))
    }
}

impl fmt::Debug for MobCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|a| a.load(Relaxed)))
            .finish()
    }
}

impl Clone for MobCounts {
    fn clone(&self) -> Self {
        Self(std::array::from_fn(|i| {
            AtomicI32::new(self.0[i].load(Relaxed))
        }))
    }
}

impl MobCounts {
    #[inline]
    pub fn add(&self, category: &'static MobCategory) {
        self.0[category.id].fetch_add(1, Relaxed);
    }

    #[inline]
    pub fn remove(&self, category: &'static MobCategory) {
        self.0[category.id].fetch_sub(1, Relaxed);
    }

    #[inline]
    pub fn can_spawn(&self, category: &'static MobCategory) -> bool {
        self.0[category.id].load(Relaxed) < category.max
    }
}

#[derive(Clone, Copy)]
struct ActiveSpawnPlayer {
    entity_id: i32,
    chunk_pos: Vector2<i32>,
    pos: Vector3<f64>,
}

pub struct LocalMobCapCalculator {
    player_mob_counts: DashMap<i32, MobCounts>,
    players_near_chunk: DashMap<Vector2<i32>, Arc<[i32]>>,
    active_players: Option<Arc<[ActiveSpawnPlayer]>>,
}

impl Clone for LocalMobCapCalculator {
    fn clone(&self) -> Self {
        let player_mob_counts = DashMap::new();
        for r in &self.player_mob_counts {
            player_mob_counts.insert(*r.key(), r.value().clone());
        }
        let players_near_chunk = DashMap::new();
        for r in &self.players_near_chunk {
            players_near_chunk.insert(*r.key(), r.value().clone());
        }
        Self {
            player_mob_counts,
            players_near_chunk,
            active_players: self.active_players.clone(),
        }
    }
}

impl Default for LocalMobCapCalculator {
    fn default() -> Self {
        Self {
            player_mob_counts: DashMap::new(),
            players_near_chunk: DashMap::new(),
            active_players: None,
        }
    }
}

impl fmt::Debug for LocalMobCapCalculator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalMobCapCalculator")
            .field("world", &"<skipped>")
            .finish()
    }
}

impl LocalMobCapCalculator {
    fn for_world(world: &World) -> Self {
        let players = world.players.load();
        let mut active = Vec::with_capacity(players.len());
        for player in players.iter() {
            if player.gamemode.load() == GameMode::Spectator {
                continue;
            }
            let entity = player.get_entity();
            active.push(ActiveSpawnPlayer {
                entity_id: player.entity_id(),
                chunk_pos: entity.chunk_pos.load(),
                pos: entity.pos.load(),
            });
        }
        Self {
            player_mob_counts: DashMap::new(),
            players_near_chunk: DashMap::new(),
            active_players: Some(active.into()),
        }
    }

    const fn calc_distance(chunk_pos: Vector2<i32>, player_pos: &Vector3<f64>) -> f64 {
        let dx = ((chunk_pos.x << 4) + 8) as f64 - player_pos.x;
        let dy = ((chunk_pos.y << 4) + 8) as f64 - player_pos.z;
        dx * dx + dy * dy
    }

    fn nearest_player_distance_sq(&self, world: &World, x: f64, y: f64, z: f64) -> Option<f64> {
        let mut min_dst_sq = f64::MAX;
        let mut found = false;
        if let Some(active_players) = &self.active_players {
            for p in active_players.iter() {
                let dx = x - p.pos.x;
                let dy = y - p.pos.y;
                let dz = z - p.pos.z;
                let dist_sq = dx * dx + dy * dy + dz * dz;
                if dist_sq < min_dst_sq {
                    min_dst_sq = dist_sq;
                    found = true;
                }
            }
        } else {
            let players = world.players.load();
            for player in players.iter() {
                if player.gamemode.load() != GameMode::Spectator {
                    let ppos = player.position();
                    let dx = x - ppos.x;
                    let dy = y - ppos.y;
                    let dz = z - ppos.z;
                    let dist_sq = dx * dx + dy * dy + dz * dz;
                    if dist_sq < min_dst_sq {
                        min_dst_sq = dist_sq;
                        found = true;
                    }
                }
            }
        }
        found.then_some(min_dst_sq)
    }

    fn get_players_near(&self, world: &World, chunk_pos: Vector2<i32>) -> Arc<[i32]> {
        if let Some(players) = self.players_near_chunk.get(&chunk_pos) {
            return players.value().clone();
        }

        let mut players = Vec::new();
        if let Some(active_players) = &self.active_players {
            for p in active_players.iter() {
                if (chunk_pos.x - p.chunk_pos.x).abs() <= 9
                    && (chunk_pos.y - p.chunk_pos.y).abs() <= 9
                    && Self::calc_distance(chunk_pos, &p.pos) < 16384.0
                {
                    players.push(p.entity_id);
                }
            }
        } else {
            for player in world.players.load().iter() {
                if player.gamemode.load() == GameMode::Spectator {
                    continue;
                }
                if Self::calc_distance(chunk_pos, &player.position()) < 16384.0 {
                    players.push(player.entity_id());
                }
            }
        }
        let players: Arc<[i32]> = players.into();
        self.players_near_chunk.insert(chunk_pos, players.clone());
        players
    }

    pub fn add_mob(&self, chunk_pos: Vector2<i32>, world: &World, category: &'static MobCategory) {
        let players = self.get_players_near(world, chunk_pos);
        for &player in players.iter() {
            self.player_mob_counts
                .entry(player)
                .or_default()
                .add(category);
        }
    }

    pub fn remove_mob(
        &self,
        chunk_pos: Vector2<i32>,
        world: &World,
        category: &'static MobCategory,
    ) {
        let players = self.get_players_near(world, chunk_pos);
        for &player in players.iter() {
            if let Some(count) = self.player_mob_counts.get(&player) {
                count.remove(category);
            }
        }
    }

    pub fn can_spawn(
        &self,
        category: &'static MobCategory,
        world: &World,
        chunk_pos: Vector2<i32>,
    ) -> bool {
        let players = self.get_players_near(world, chunk_pos);
        for &player in players.iter() {
            if let Some(count) = self.player_mob_counts.get(&player) {
                if count.can_spawn(category) {
                    return true;
                }
            } else {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone)]
struct PointCharge(BlockPos, f64);

impl PointCharge {
    /// Change in spawn potential this charge adds at `pos`.
    ///
    /// Widen before subtracting, like vanilla's `Vec3i.distSqr` does with `i2d`;
    /// multiplying in `i32` overflows past ~46341 blocks before any cast runs.
    fn get_potential_change(&self, pos: &BlockPos) -> f64 {
        let dx = self.0.0.x as f64 - pos.0.x as f64;
        let dy = self.0.0.y as f64 - pos.0.y as f64;
        let dz = self.0.0.z as f64 - pos.0.z as f64;
        let dist_sq = dx * dx + dy * dy + dz * dz;
        if dist_sq == 0.0 {
            f64::INFINITY
        } else {
            self.1 / dist_sq.sqrt()
        }
    }
}

#[derive(Default, Debug)]
struct PotentialCalculator(std::sync::Mutex<Vec<PointCharge>>);

impl Clone for PotentialCalculator {
    fn clone(&self) -> Self {
        Self(std::sync::Mutex::new(
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        ))
    }
}

impl PotentialCalculator {
    pub fn add_charge(&self, pos: &BlockPos, charge: f64) {
        if charge != 0.0 {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(PointCharge(*pos, charge));
        }
    }

    pub fn remove_charge(&self, pos: &BlockPos, charge: f64) {
        if charge != 0.0 {
            let mut charges = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(idx) = charges.iter().position(|c| c.0 == *pos && c.1 == charge) {
                charges.swap_remove(idx);
            }
        }
    }

    pub fn get_potential_energy_change(&self, pos: &BlockPos, charge: f64) -> f64 {
        if charge == 0.0 {
            return 0.0;
        }
        let mut sum: f64 = 0.0;
        let charges = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for i in charges.iter() {
            sum += i.get_potential_change(pos);
        }
        sum * charge
    }
}

pub struct SpawnState {
    spawnable_chunk_count: i32,
    pub mob_category_counts: MobCounts,
    spawn_potential: PotentialCalculator,
    local_mob_cap_calculator: LocalMobCapCalculator,
}

impl Clone for SpawnState {
    fn clone(&self) -> Self {
        Self {
            spawnable_chunk_count: self.spawnable_chunk_count,
            mob_category_counts: self.mob_category_counts.clone(),
            spawn_potential: self.spawn_potential.clone(),
            local_mob_cap_calculator: self.local_mob_cap_calculator.clone(),
        }
    }
}

impl fmt::Debug for SpawnState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpawnState")
            .field("spawnable_chunk_count", &self.spawnable_chunk_count)
            .field("mob_category_counts", &self.mob_category_counts)
            .field("spawn_potential", &self.spawn_potential)
            .field("local_mob_cap_calculator", &self.local_mob_cap_calculator)
            .finish()
    }
}

fn counts_toward_spawn_limit(entity: &dyn EntityBase) -> bool {
    entity.counts_toward_mob_cap()
        && entity.get_mob().is_none_or(|mob| {
            !mob.get_mob_entity().persistence_required.load(Relaxed)
                && !mob.requires_custom_persistence()
        })
}

impl SpawnState {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            spawnable_chunk_count: 0,
            mob_category_counts: MobCounts::default(),
            spawn_potential: PotentialCalculator::default(),
            local_mob_cap_calculator: LocalMobCapCalculator::default(),
        }
    }

    pub const fn set_spawnable_chunk_count(&mut self, count: i32) {
        self.spawnable_chunk_count = count;
    }

    pub fn add_entity(&self, world: &World, entity: &dyn EntityBase) {
        if !counts_toward_spawn_limit(entity) {
            return;
        }
        let base_entity = entity.get_entity();
        let entity_type = base_entity.entity_type;
        if entity_type.category == &MobCategory::MISC {
            return;
        }
        let entity_pos = base_entity.block_pos.load();
        let biome = base_entity.current_biome.load();
        if let Some(cost) = biome.spawn_costs.get(entity_type.resource_name) {
            self.spawn_potential.add_charge(&entity_pos, cost.charge);
        }
        if entity.get_mob().is_some() || entity_type.mob {
            self.local_mob_cap_calculator.add_mob(
                base_entity.chunk_pos.load(),
                world,
                entity_type.category,
            );
        }
        self.mob_category_counts.add(entity_type.category);
    }

    pub fn remove_entity(&self, world: &World, entity: &dyn EntityBase) {
        if !counts_toward_spawn_limit(entity) {
            return;
        }
        let base_entity = entity.get_entity();
        let entity_type = base_entity.entity_type;
        if entity_type.category == &MobCategory::MISC {
            return;
        }
        let entity_pos = base_entity.block_pos.load();
        let biome = base_entity.current_biome.load();
        if let Some(cost) = biome.spawn_costs.get(entity_type.resource_name) {
            self.spawn_potential.remove_charge(&entity_pos, cost.charge);
        }
        if entity.get_mob().is_some() || entity_type.mob {
            self.local_mob_cap_calculator.remove_mob(
                base_entity.chunk_pos.load(),
                world,
                entity_type.category,
            );
        }
        self.mob_category_counts.remove(entity_type.category);
    }

    pub fn new(
        chunk_count: i32,
        entities: &ArcSwap<Vec<Arc<dyn EntityBase>>>,
        world: &World,
    ) -> Self {
        let potential = PotentialCalculator::default();
        let local_mob_cap = LocalMobCapCalculator::for_world(world);
        let counter = MobCounts::default();
        for entity in entities.load().iter() {
            if !counts_toward_spawn_limit(entity.as_ref()) {
                continue;
            }
            let base_entity = entity.get_entity();
            let entity_type = base_entity.entity_type;
            if entity_type.category == &MobCategory::MISC {
                continue;
            }
            let chunk_pos = base_entity.chunk_pos.load();
            let entity_pos = base_entity.block_pos.load();
            let biome = base_entity.current_biome.load();
            if let Some(cost) = biome.spawn_costs.get(entity_type.resource_name) {
                potential.add_charge(&entity_pos, cost.charge);
            }
            if entity.get_mob().is_some() || entity_type.mob {
                local_mob_cap.add_mob(chunk_pos, world, entity_type.category);
            }
            counter.add(entity_type.category);
        }
        Self {
            spawnable_chunk_count: chunk_count,
            mob_category_counts: counter,
            spawn_potential: potential,
            local_mob_cap_calculator: local_mob_cap,
        }
    }

    #[inline]
    pub fn can_spawn_for_category_global(&self, category: &'static MobCategory) -> bool {
        self.mob_category_counts.0[category.id].load(Relaxed)
            < category.max * self.spawnable_chunk_count / MAGIC_NUMBER
    }

    pub fn can_spawn_for_category_local(
        &self,
        world: &Arc<World>,
        category: &'static MobCategory,
        chunk_pos: Vector2<i32>,
    ) -> bool {
        self.local_mob_cap_calculator
            .can_spawn(category, world, chunk_pos)
    }

    pub fn can_spawn(
        &self,
        entity_type: &'static EntityType,
        pos: &BlockPos,
        world: &Arc<World>,
    ) -> bool {
        let biome = world.level.get_rough_biome(pos);
        biome
            .spawn_costs
            .get(entity_type.resource_name)
            .is_none_or(|cost| {
                self.spawn_potential
                    .get_potential_energy_change(pos, cost.charge)
                    <= cost.energy_budget
            })
    }
}

#[must_use]
pub fn get_filtered_spawning_categories(
    state: &SpawnState,
    spawn_friendlies: bool,
    spawn_enemies: bool,
    spawn_passives: bool,
) -> Vec<&'static MobCategory> {
    let mut ret = Vec::with_capacity(MobCategory::SPAWNING_CATEGORIES.len());
    for category in MobCategory::SPAWNING_CATEGORIES {
        let is_type_allowed = if category.is_friendly {
            spawn_friendlies
        } else {
            spawn_enemies
        };

        if !is_type_allowed {
            continue;
        }

        if category.is_persistent && !spawn_passives {
            continue;
        }

        if state.can_spawn_for_category_global(category) {
            ret.push(category);
        }
    }
    ret
}

pub fn spawn_for_chunk(
    world: &Arc<World>,
    chunk_pos: Vector2<i32>,
    chunk: &Arc<ChunkData>,
    spawn_state: &SpawnState,
    spawn_list: &Vec<&'static MobCategory>,
    is_thundering: bool,
) {
    for category in spawn_list {
        if spawn_state.can_spawn_for_category_local(world, category, chunk_pos) {
            let random_pos = get_random_pos_within(world.min_y, &chunk_pos, chunk);
            if random_pos.0.y > world.min_y {
                spawn_category_for_position(
                    category,
                    world,
                    random_pos,
                    &chunk_pos,
                    spawn_state,
                    is_thundering,
                );
            }
        }
    }
}

pub fn get_random_pos_within(
    min_y: i32,
    chunk_pos: &Vector2<i32>,
    chunk: &Arc<ChunkData>,
) -> BlockPos {
    let mut rng = Xoroshiro::from_seed(get_seed());

    let x = (chunk_pos.x << 4) + rng.next_bounded_i32(16);
    let z = (chunk_pos.y << 4) + rng.next_bounded_i32(16);
    let temp_y = chunk
        .heightmap
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(ChunkHeightmapType::WorldSurface, x, z, chunk.section.min_y)
        + 1;
    let y = rng.next_inbetween_i32(min_y, temp_y);
    BlockPos::new(x, y, z)
}

pub fn spawn_mobs_for_chunk_generation(
    world: &Arc<World>,
    cache: &mut dyn GenerationCache,
    biome: &'static Biome,
    chunk_x: i32,
    chunk_z: i32,
) {
    let creatures = biome.spawners.creature;
    let embedder = embedder_entries(biome, &MobCategory::CREATURE);

    if (creatures.is_empty() && embedder.is_empty())
        || !world.level_info.load().game_rules.spawn_mobs
    {
        return;
    }

    let xo = chunk_x << 4;
    let zo = chunk_z << 4;

    while rand::random::<f32>() < biome.creature_spawn_probability {
        let Some(entry) = choose_weighted(creatures, embedder) else {
            return;
        };

        let (min_count, max_count) = entry.counts();
        let count = min_count + rand::random_range(0..=(max_count - min_count).max(0));
        let entity_type = entry.entity_type();

        let mut x = xo + rand::random_range(0..16);
        let mut z = zo + rand::random_range(0..16);
        let start_x = x;
        let start_z = z;
        let group = NEXT_SPAWN_GROUP.fetch_add(1, Relaxed);

        for _ in 0..count {
            let mut candidates = Vec::new();

            for _ in 0..4 {
                let pos = get_top_non_colliding_pos(world, cache, entity_type, x, z);

                let summonable = matches!(entry, SpawnEntry::Embedder(_)) || entity_type.summonable;
                if summonable
                    && is_spawn_position_ok_cache_at(cache, &pos, entity_type, entry.location())
                {
                    let width = f64::from(entity_type.dimension[0]);
                    let fx =
                        (f64::from(x)).clamp(f64::from(xo) + width, f64::from(xo) + 16.0 - width);
                    let fz =
                        (f64::from(z)).clamp(f64::from(zo) + width, f64::from(zo) + 16.0 - width);
                    candidates.push((
                        Vector3::new(fx, f64::from(pos.0.y), fz),
                        BlockPos::new(fx.floor() as i32, pos.0.y, fz.floor() as i32),
                    ));
                }

                x += rand::random_range(0..5) - rand::random_range(0..5);
                z += rand::random_range(0..5) - rand::random_range(0..5);
                while x < xo || x >= xo + 16 || z < zo || z >= zo + 16 {
                    x = start_x + rand::random_range(0..5) - rand::random_range(0..5);
                    z = start_z + rand::random_range(0..5) - rand::random_range(0..5);
                }
            }
            if !candidates.is_empty() {
                PENDING_GENERATION
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(PendingGenerationSpawn {
                        world: Arc::downgrade(world),
                        entry,
                        group,
                        chunk: Vector2::new(chunk_x, chunk_z),
                        candidates,
                        queued: world.get_world_age(),
                    });
            }
        }
    }
}

pub fn get_top_non_colliding_pos(
    world: &World,
    cache: &dyn GenerationCache,
    entity_type: &'static EntityType,
    x: i32,
    z: i32,
) -> BlockPos {
    let mut y = cache.get_top_y(&entity_type.spawn_restriction.heightmap, x, z);
    let mut pos_vec = Vector3::new(x, y, z);
    let min_y = world.min_y;

    if world.dimension.has_ceiling {
        loop {
            y -= 1;
            pos_vec.y = y;
            if GenerationCache::get_block_state(cache, &pos_vec)
                .to_state()
                .is_air()
                || y <= min_y
            {
                break;
            }
        }

        loop {
            y -= 1;
            pos_vec.y = y;
            if !GenerationCache::get_block_state(cache, &pos_vec)
                .to_state()
                .is_air()
                || y <= min_y
            {
                break;
            }
        }
    }

    let pos = BlockPos::new(x, y, z);

    adjust_spawn_position_cache(cache, pos, entity_type)
}

pub fn adjust_spawn_position_cache(
    cache: &dyn GenerationCache,
    pos: BlockPos,
    entity_type: &'static EntityType,
) -> BlockPos {
    if matches!(
        entity_type.spawn_restriction.location,
        SpawnLocation::OnGround
    ) {
        let below = pos.down();
        let state = GenerationCache::get_block_state(cache, &below.0).to_state();

        if !state.is_full_cube() && !state.is_liquid() {
            return below;
        }
    }
    pos
}

pub fn adjust_spawn_position(
    world: &World,
    pos: BlockPos,
    entity_type: &'static EntityType,
) -> BlockPos {
    if matches!(
        entity_type.spawn_restriction.location,
        SpawnLocation::OnGround
    ) {
        let below = pos.down();
        let (block, state) = world.get_block_and_state(&below);
        if world
            .block_registry
            .is_pathfindable(block, state, PathComputationType::Land)
        {
            return below;
        }
    }
    pos
}

#[allow(clippy::too_many_lines)]
pub fn spawn_category_for_position(
    category: &'static MobCategory,
    world: &Arc<World>,
    pos: BlockPos,
    chunk_pos: &Vector2<i32>,
    spawn_state: &SpawnState,
    is_thundering: bool,
) {
    let y_start = pos.0.y;
    let state = world.get_block_state(&pos);
    if state.is_solid_block() || state.is_full_cube() {
        return;
    }

    let mut cluster_size = 0;

    for _ in 0..3 {
        let mut x = pos.0.x;
        let mut z = pos.0.z;
        let mut current_spawner: Option<SpawnEntry> = None;
        let mut max = (rng().random::<f32>() * 4.0).ceil() as i32;
        let mut group_size = 0;
        let mut group_data = None;
        let group = NEXT_SPAWN_GROUP.fetch_add(1, Relaxed);
        let mut group_claimed = false;
        let mut ll = 0;

        while ll < max {
            x += rng().random_range(0..6) - rng().random_range(0..6);
            z += rng().random_range(0..6) - rng().random_range(0..6);
            let check_pos = BlockPos::new(x, y_start, z);
            let xx = f64::from(x) + 0.5;
            let zz = f64::from(z) + 0.5;

            let nearest_player_distance_sq = spawn_state
                .local_mob_cap_calculator
                .nearest_player_distance_sq(world, xx, f64::from(y_start), zz);

            if let Some(player_distance_sq) = nearest_player_distance_sq
                && is_right_distance_to_player_and_spawn_point(
                    world,
                    &check_pos,
                    chunk_pos,
                    player_distance_sq,
                )
            {
                if current_spawner.is_none() {
                    let Some(entry) = choose_spawn_entry(world, category, &check_pos) else {
                        break;
                    };
                    current_spawner = Some(entry);
                    let (min_count, max_count) = entry.counts();
                    max = min_count + rng().random_range(0..=(max_count - min_count).max(0));
                }

                let Some(entry) = current_spawner else {
                    break;
                };
                let entity_type = entry.entity_type();

                if is_valid_spawn_position_for_entry(
                    world,
                    &check_pos,
                    category,
                    entry,
                    player_distance_sq,
                    is_thundering,
                ) && spawn_state.can_spawn(entity_type, &check_pos, world)
                    && let Some(entity) =
                        entry.create(world, Vector3::new(xx, f64::from(y_start), zz))
                {
                    entity
                        .get_entity()
                        .set_rotation(rng().random::<f32>() * 360.0, 0.0);

                    let is_valid_for_mob = entity.get_mob().is_none_or(|mob| {
                        let despawn_dist = f64::from(entity_type.category.despawn_distance);
                        !(player_distance_sq > despawn_dist * despawn_dist
                            && mob.remove_when_far_away(player_distance_sq))
                    });

                    if is_valid_for_mob {
                        if matches!(entry, SpawnEntry::Vanilla(..)) {
                            group_data = finalize_spawn(
                                &entity,
                                world,
                                EntitySpawnReason::Natural,
                                group_data,
                            );
                        }
                        cluster_size += 1;
                        group_size += 1;
                        let max_group_size_reached = entity
                            .get_mob()
                            .is_some_and(|mob| mob.is_max_group_size_reached(group_size));
                        world.spawn_entity_non_save(entity.clone());
                        entry.spawned(
                            world,
                            entity.as_ref(),
                            EntitySpawnReason::Natural,
                            group,
                            &mut group_claimed,
                        );
                        if cluster_size >= entity_type.limit_per_chunk {
                            return;
                        }

                        if max_group_size_reached {
                            break;
                        }
                    }
                }
            }

            ll += 1;
        }
    }
}

#[must_use]
pub fn get_nearest_player(pos: &Vector3<f64>, player_positions: &[Vector3<f64>]) -> f64 {
    let mut min_dst_sq = f64::MAX;

    for player_pos in player_positions {
        let cur_dst_sq = player_pos.squared_distance_to_vec(pos);
        if cur_dst_sq < min_dst_sq {
            min_dst_sq = cur_dst_sq;
        }
    }
    min_dst_sq
}

#[must_use]
pub fn is_right_distance_to_player_and_spawn_point(
    world: &World,
    pos: &BlockPos,
    chunk_pos: &Vector2<i32>,
    nearest_player_distance_sq: f64,
) -> bool {
    if nearest_player_distance_sq <= 576.0 {
        return false;
    }

    if world.dimension == Dimension::OVERWORLD {
        let level_info = world.level_info.load();
        let dx = f64::from(level_info.spawn_x - pos.0.x);
        let dy = (f64::from(level_info.spawn_y) + 0.5) - f64::from(pos.0.y);
        let dz = f64::from(level_info.spawn_z - pos.0.z);
        if dx * dx + dy * dy + dz * dz < 576.0 {
            return false;
        }
    }

    let chunk_x = get_section_cord(pos.0.x);
    let chunk_z = get_section_cord(pos.0.z);
    let target_chunk = Vector2::new(chunk_x, chunk_z);
    target_chunk == *chunk_pos
        || (world
            .active_chunks
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(&target_chunk)
            && world
                .worldborder
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_block(pos.0.x, pos.0.z))
}

#[must_use]
pub fn can_spawn_mob_at(
    world: &World,
    category: &'static MobCategory,
    spawner_type: &'static str,
    pos: &BlockPos,
) -> bool {
    let biome = world.level.get_rough_biome(pos);
    let spawners = pool(biome, category);
    let target = spawner_type
        .strip_prefix("minecraft:")
        .unwrap_or(spawner_type);
    spawners.iter().any(|s| {
        let name = s.r#type.strip_prefix("minecraft:").unwrap_or(s.r#type);
        name == target
    })
}

#[must_use]
pub fn get_random_spawn_mob_at(
    world: &Arc<World>,
    category: &'static MobCategory,
    block_pos: &BlockPos,
) -> Option<&'static Spawner> {
    match choose_spawn_entry(world, category, block_pos)? {
        SpawnEntry::Vanilla(spawner, _) => Some(spawner),
        SpawnEntry::Embedder(_) => None,
    }
}

/// Vanilla `NaturalSpawner.getRandomSpawnMobAt`, including embedder entries.
#[must_use]
pub fn choose_spawn_entry(
    world: &Arc<World>,
    category: &'static MobCategory,
    block_pos: &BlockPos,
) -> Option<SpawnEntry> {
    let biome = world.level.get_rough_biome(block_pos);
    if category == &MobCategory::WATER_AMBIENT
        && biome.has_tag(&MINECRAFT_REDUCE_WATER_AMBIENT_SPAWNS)
        && rng().random::<f32>() < 0.98f32
    {
        None
    } else {
        choose_weighted(pool(biome, category), embedder_entries(biome, category))
    }
}

#[must_use]
pub fn is_valid_spawn_position_for_type(
    world: &Arc<World>,
    block_pos: &BlockPos,
    category: &'static MobCategory,
    entity_type: &'static EntityType,
    spawner_type: &'static str,
    distance: f64,
    is_thundering: bool,
) -> bool {
    pool(world.level.get_rough_biome(block_pos), category)
        .iter()
        .find(|spawner| spawner.r#type == spawner_type)
        .is_some_and(|spawner| {
            is_valid_spawn_position_for_entry(
                world,
                block_pos,
                category,
                SpawnEntry::Vanilla(spawner, entity_type),
                distance,
                is_thundering,
            )
        })
}

/// Vanilla `NaturalSpawner.isValidSpawnPostitionForType` for a vanilla or embedder entry.
#[must_use]
pub fn is_valid_spawn_position_for_entry(
    world: &Arc<World>,
    block_pos: &BlockPos,
    category: &'static MobCategory,
    entry: SpawnEntry,
    distance: f64,
    is_thundering: bool,
) -> bool {
    let entity_type = entry.entity_type();
    if category == &MobCategory::MISC || entity_type.category == &MobCategory::MISC {
        return false;
    }
    if !entity_type.can_spawn_far_from_player
        && distance
            > f64::from(entity_type.category.despawn_distance)
                * f64::from(entity_type.category.despawn_distance)
    {
        return false;
    }
    let in_pool = match entry {
        SpawnEntry::Vanilla(spawner, _) => {
            entity_type.summonable && can_spawn_mob_at(world, category, spawner.r#type, block_pos)
        }
        SpawnEntry::Embedder(spawn) => {
            embedder_entries(world.level.get_rough_biome(block_pos), category)
                .iter()
                .any(|other| other.id == spawn.id)
        }
    };
    if !in_pool {
        return false;
    }
    if !is_spawn_position_ok_at(world, block_pos, entity_type, entry.location()) {
        return false;
    }
    if !entry.check_rules(world, block_pos, is_thundering, EntitySpawnReason::Natural) {
        return false;
    }
    if !world.is_space_empty(entry.spawn_bounds(Vector3::new(
        f64::from(block_pos.0.x) + 0.5,
        f64::from(block_pos.0.y),
        f64::from(block_pos.0.z) + 0.5,
    ))) {
        return false;
    }
    true
}

#[must_use]
pub fn is_spawn_position_ok(
    world: &Arc<World>,
    block_pos: &BlockPos,
    entity_type: &'static EntityType,
) -> bool {
    is_spawn_position_ok_at(
        world,
        block_pos,
        entity_type,
        &entity_type.spawn_restriction.location,
    )
}

/// `is_spawn_position_ok` with the placement location given separately from the type.
#[must_use]
pub fn is_spawn_position_ok_at(
    world: &Arc<World>,
    block_pos: &BlockPos,
    entity_type: &'static EntityType,
    location: &SpawnLocation,
) -> bool {
    let within_bounds = world
        .worldborder
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains_block(block_pos.0.x, block_pos.0.z);

    match location {
        SpawnLocation::InLava => {
            within_bounds && world.get_fluid(block_pos).has_tag(&MINECRAFT_LAVA)
        }
        SpawnLocation::InWater => {
            if !within_bounds {
                return false;
            }
            let above_state = world.get_block_state(&block_pos.up());
            world.get_fluid(block_pos).has_tag(&MINECRAFT_WATER) && !above_state.is_solid_block()
        }
        SpawnLocation::OnGround => {
            if !within_bounds {
                return false;
            }
            let down = world.get_block_state(&block_pos.down());
            let up = world.get_block_state(&block_pos.up());
            let cur = world.get_block_state(block_pos);
            let is_valid_spawn_below = is_valid_spawn_floor(down, entity_type);

            if is_valid_spawn_below {
                is_valid_empty_spawn_block(cur, entity_type)
                    && is_valid_empty_spawn_block(up, entity_type)
            } else {
                false
            }
        }
        SpawnLocation::Unrestricted => true,
    }
}

#[must_use]
pub fn is_spawn_position_ok_cache(
    cache: &dyn GenerationCache,
    block_pos: &BlockPos,
    entity_type: &'static EntityType,
) -> bool {
    is_spawn_position_ok_cache_at(
        cache,
        block_pos,
        entity_type,
        &entity_type.spawn_restriction.location,
    )
}

fn is_spawn_position_ok_cache_at(
    cache: &dyn GenerationCache,
    block_pos: &BlockPos,
    entity_type: &'static EntityType,
    location: &SpawnLocation,
) -> bool {
    let pos_vec = block_pos.0;
    let state = GenerationCache::get_block_state(cache, &pos_vec).to_state();

    match location {
        SpawnLocation::InLava => {
            state.is_liquid() && Block::from_state_id(state.id).has_tag(&MINECRAFT_LAVA)
        }
        SpawnLocation::InWater => {
            let above_pos = block_pos.up().0;
            let above_state = GenerationCache::get_block_state(cache, &above_pos).to_state();

            state.is_liquid()
                && Block::from_state_id(state.id).has_tag(&MINECRAFT_WATER)
                && !above_state.is_solid_block()
        }
        SpawnLocation::OnGround => {
            let down_pos = block_pos.down().0;
            let up_pos = block_pos.up().0;

            let down = GenerationCache::get_block_state(cache, &down_pos).to_state();
            let up = GenerationCache::get_block_state(cache, &up_pos).to_state();

            let is_valid_spawn_below = is_valid_spawn_floor(down, entity_type);

            if is_valid_spawn_below {
                is_valid_empty_spawn_block(state, entity_type)
                    && is_valid_empty_spawn_block(up, entity_type)
            } else {
                false
            }
        }
        SpawnLocation::Unrestricted => true,
    }
}

#[must_use]
pub fn is_valid_empty_spawn_block(
    state: &'static BlockState,
    entity_type: &'static EntityType,
) -> bool {
    if state.is_full_cube() {
        return false;
    }
    if state.is_liquid() {
        return false;
    }
    if Block::from_state_id(state.id).has_tag(&MINECRAFT_PREVENT_MOB_SPAWNING_INSIDE) {
        return false;
    }
    !is_block_dangerous(entity_type, state)
}

#[must_use]
pub(crate) fn is_valid_spawn_floor(
    state: &'static BlockState,
    entity_type: &'static EntityType,
) -> bool {
    match Block::from_state_id(state.id).spawn_floor_predicate() {
        SpawnFloorPredicate::Default => {
            state.is_side_solid(BlockDirection::Up) && state.luminance < 14
        }
        SpawnFloorPredicate::Never => false,
        SpawnFloorPredicate::Always => true,
        SpawnFloorPredicate::OcelotOrParrot => {
            entity_type == &EntityType::OCELOT || entity_type == &EntityType::PARROT
        }
        SpawnFloorPredicate::PolarBear => entity_type == &EntityType::POLAR_BEAR,
        SpawnFloorPredicate::FireImmune => entity_type.fire_immune,
    }
}

fn is_block_dangerous(entity_type: &'static EntityType, state: &'static BlockState) -> bool {
    let block = Block::from_state_id(state.id);
    if !entity_type.fire_immune && is_burning_block(block) {
        return true;
    }
    block.id == Block::WITHER_ROSE.id
        || block.id == Block::SWEET_BERRY_BUSH.id
        || block.id == Block::CACTUS.id
        || block.id == Block::POWDER_SNOW.id
}

fn is_burning_block(block: &Block) -> bool {
    block.has_tag(&pumpkin_data::tag::Block::MINECRAFT_FIRE)
        || block.id == Block::LAVA.id
        || block.id == Block::MAGMA_BLOCK.id
        || block.id == Block::LAVA_CAULDRON.id
        || block.id == Block::CAMPFIRE.id
        || block.id == Block::SOUL_CAMPFIRE.id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spawn_state_counts_entities_outside_active_chunks()
    -> Result<(), Box<dyn std::error::Error>> {
        use crate::entity::{ai::test_support::world, r#type::from_type};

        let folder = tempfile::tempdir()?;
        let world = world(&folder);
        // Nothing has been marked active, so the entity's chunk is outside the
        // simulation-distance set. Vanilla still counts it through `getFullChunk`.
        assert!(
            world
                .active_chunks
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        );
        let entity: Arc<dyn EntityBase> = from_type(
            &EntityType::ZOMBIE,
            Vector3::new(8.0, 64.0, 8.0),
            &world,
            Uuid::new_v4(),
        );
        let entities = ArcSwap::from_pointee(vec![entity]);

        let state = SpawnState::new(289, &entities, &world);

        assert_eq!(
            state.mob_category_counts.0[MobCategory::MONSTER.id].load(Relaxed),
            1
        );
        world.level.shutdown().await;
        Ok(())
    }

    #[test]
    fn potential_change_is_finite_beyond_i32_range() {
        let origin = BlockPos::new(0, 64, 0);
        for (far, distance) in [
            (BlockPos::new(46_342, 64, 0), 46_342.0),
            (
                BlockPos::new(40_000, 64, 40_000),
                40_000f64.hypot(40_000f64),
            ),
        ] {
            let change = PointCharge(far, 1.0).get_potential_change(&origin);

            assert_eq!(change, 1.0 / distance, "wrong potential for {far:?}");
        }
    }

    #[test]
    fn vanilla_spawn_floor_predicates_are_preserved() {
        for (block, entity_type, expected) in [
            (&Block::BEDROCK, &EntityType::CREEPER, false),
            (&Block::STONE, &EntityType::CREEPER, true),
            (&Block::GLASS, &EntityType::CREEPER, false),
            (&Block::OAK_LEAVES, &EntityType::OCELOT, true),
            (&Block::OAK_LEAVES, &EntityType::CREEPER, false),
            (&Block::ICE, &EntityType::POLAR_BEAR, true),
            (&Block::ICE, &EntityType::CREEPER, false),
            (&Block::MAGMA_BLOCK, &EntityType::BLAZE, true),
            (&Block::MAGMA_BLOCK, &EntityType::CREEPER, false),
            (&Block::JACK_O_LANTERN, &EntityType::CREEPER, true),
        ] {
            assert_eq!(
                is_valid_spawn_floor(block.default_state, entity_type),
                expected,
                "{}",
                block.name
            );
        }
    }
}
