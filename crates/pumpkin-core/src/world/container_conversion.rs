//! Deferred-container proposals run outside world and inventory locks.
//! The initial transaction supports untouched, single vanilla chests.
use super::{Arc, Block, BlockFlags, BlockPos, BlockStateId, NbtCompound, World};
use crate::block::entities::{BlockEntity, chest::ChestBlockEntity, content::ContentBlockEntity};
use pumpkin_data::block_properties::{ChestLikeProperties, ChestType};
use pumpkin_world::chunk::io::Dirtiable;
use std::{
    cell::Cell,
    sync::{OnceLock, atomic::Ordering},
};

pub enum Proposal {
    Pass,
    Defer,
    Replace(BlockStateId, NbtCompound),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Deferred,
    Converted,
}

type Hook = fn(&World, BlockPos, BlockStateId, &NbtCompound) -> Proposal;
static HOOK: OnceLock<Hook> = OnceLock::new();
thread_local! { static IN_PROPOSAL: Cell<bool> = const { Cell::new(false) }; }

pub fn install(hook: Hook) -> bool {
    HOOK.set(hook).is_ok()
}

fn supported(state: BlockStateId, nbt: &NbtCompound) -> bool {
    state.to_block() == &Block::CHEST
        && ChestLikeProperties::from_state_id(state).r#type == ChestType::Single
        && nbt.get_string("id") == Some("minecraft:chest")
        && nbt
            .get_string("LootTable")
            .is_some_and(|key| !key.is_empty())
        && nbt.get("Lock").is_none()
        && nbt.get("lock").is_none()
        && (nbt.get("Items").is_none()
            || nbt
                .get_list("Items")
                .is_some_and(<[pumpkin_nbt::tag::NbtTag]>::is_empty))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_nbt::tag::NbtTag;

    fn unopened() -> NbtCompound {
        let mut nbt = NbtCompound::new();
        nbt.put_string("id", "minecraft:chest");
        nbt.put_string("LootTable", "minecraft:chests/desert_pyramid");
        nbt.put_long("LootTableSeed", 8451212618467398019);
        nbt
    }

    #[test]
    fn accepts_only_unopened_vanilla_single_chests() {
        let single = Block::CHEST.default_state.id;
        let base = unopened();
        assert_eq!(
            ChestLikeProperties::from_state_id(single).r#type,
            ChestType::Single
        );
        assert!(supported(single, &base));
        assert!(!supported(Block::TRAPPED_CHEST.default_state.id, &base));
        assert!(!supported(Block::BARREL.default_state.id, &base));
        let other_half = Block::CHEST
            .states
            .iter()
            .find(|candidate| {
                ChestLikeProperties::from_state_id(candidate.id).r#type != ChestType::Single
            })
            .expect("vanilla double chest state");
        assert!(!supported(other_half.id, &base));

        let mut already_opened = base.clone();
        already_opened.put_list("Items", vec![NbtTag::Compound(NbtCompound::new())]);
        assert!(!supported(single, &already_opened));

        let mut explicitly_empty = base.clone();
        explicitly_empty.put_list("Items", Vec::<NbtTag>::new());
        assert!(supported(single, &explicitly_empty));

        let mut invalid_item_encoding = base.clone();
        invalid_item_encoding.put_int("Items", 4);
        assert!(!supported(single, &invalid_item_encoding));

        for key in ["Lock", "lock"] {
            let mut locked = base.clone();
            locked.put_string(key, "secret");
            assert!(!supported(single, &locked));
        }
        let mut no_table = base.clone();
        no_table.child_tags.remove("LootTable");
        assert!(!supported(single, &no_table));
        let mut missing_id = base;
        missing_id.put_string("id", "minecraft:barrel");
        assert!(!supported(single, &missing_id));
    }
}

/// Propose only for the pending record; live conversion has a separate guard.
/// A successful conversion commits before publishing any entity to a caller.
pub(super) fn pending(world: &World, pos: BlockPos, commit: bool) -> Outcome {
    let Some(hook) = HOOK.get() else {
        return Outcome::Pass;
    };
    let chunk_pos = pos.chunk_position();
    let relative = pos.chunk_relative_position();
    let Some((state, source)) = world
        .level
        .read_chunk_sync(&chunk_pos, |chunk| {
            let pending = chunk
                .pending_block_entities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if world
                .block_entities
                .get(&chunk_pos)
                .is_some_and(|entities| entities.contains_key(&pos))
            {
                return None;
            }
            let state = chunk.section.get_block_absolute_y(
                relative.x as usize,
                relative.y,
                relative.z as usize,
            )?;
            let nbt = pending.get(&pos)?;
            supported(state, nbt).then(|| (state, nbt.clone()))
        })
        .flatten()
    else {
        return Outcome::Pass;
    };
    replace(world, pos, state, &source, None, *hook, commit)
}

/// Called before loot unpack, and when a live entity with newly assigned
/// deferred loot is discovered by migration.
///
/// An already exposed vanilla
/// inventory stays vanilla rather than invalidating another owner's handle.
pub fn live(world: &World, entity: &Arc<dyn BlockEntity>, commit: bool) -> Outcome {
    let Some(hook) = HOOK.get() else {
        return Outcome::Pass;
    };
    let Some(chest) = entity.as_any().downcast_ref::<ChestBlockEntity>() else {
        return Outcome::Pass;
    };
    match chest.inventory_access.load(Ordering::Acquire) {
        2 => return Outcome::Converted,
        1 => return Outcome::Pass,
        _ => {}
    }
    if !chest.has_loot_table() {
        return Outcome::Pass;
    }
    let pos = chest.position;
    let Some(state) = world.get_block_state_id_if_loaded(&pos) else {
        return Outcome::Deferred;
    };
    let source = super::block_entity_nbt(entity.as_ref());
    if !supported(state, &source) {
        return Outcome::Pass;
    }
    let outcome = replace(world, pos, state, &source, Some(entity), *hook, commit);
    if outcome == Outcome::Deferred {
        world
            .pending_block_entity_migrations
            .push(pos.chunk_position());
    }
    outcome
}

// Keep the compare-and-swap transaction and lock ordering visible together.
#[allow(clippy::too_many_lines)]
fn replace(
    world: &World,
    pos: BlockPos,
    state: BlockStateId,
    source: &NbtCompound,
    previous: Option<&Arc<dyn BlockEntity>>,
    hook: Hook,
    commit: bool,
) -> Outcome {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            IN_PROPOSAL.set(false);
        }
    }
    let chunk_pos = pos.chunk_position();
    let relative = pos.chunk_relative_position();
    if IN_PROPOSAL.replace(true) {
        return Outcome::Deferred;
    }
    let proposal = {
        let _guard = Guard;
        hook(world, pos, state, source)
    };
    let (destination, mut data) = match proposal {
        Proposal::Pass => return Outcome::Pass,
        Proposal::Defer => return Outcome::Deferred,
        Proposal::Replace(state, nbt) => (state, nbt),
    };
    // Lazy readers and parallel hopper ticks must not run replacement
    // callbacks. The active chunk migration boundary performs the commit.
    if !commit {
        return Outcome::Deferred;
    }
    // No force-loading. A generated chunk may still need postprocessing.
    let ready = {
        let border = world
            .worldborder
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        border.contains_block(pos.0.x, pos.0.z)
            && (-2..=2).all(|dx| {
                (-2..=2).all(|dz| {
                    let neighbor = pumpkin_util::math::vector2::Vector2::new(
                        chunk_pos.x + dx,
                        chunk_pos.y + dz,
                    );
                    // Ignore wholly out-of-border neighbours, as no player can load
                    // them to satisfy the normal 5x5 readiness check.
                    let x = f64::from(neighbor.x * 16 + 8);
                    let z = f64::from(neighbor.y * 16 + 8);
                    !border.is_within_bounds(x, z, 8.0) || world.level.is_chunk_loaded(&neighbor)
                })
            })
    };
    if !ready {
        return Outcome::Deferred;
    }
    let Some(id) =
        pumpkin_data::content::state(destination.as_u16()).and_then(|entry| entry.entity)
    else {
        return Outcome::Deferred;
    };
    if data.get_string("LootTable") != source.get_string("LootTable")
        || data.get_long("LootTableSeed").unwrap_or(0)
            != source.get_long("LootTableSeed").unwrap_or(0)
    {
        return Outcome::Deferred;
    }
    data.put_string("id", id.to_owned());
    data.put_int("x", pos.0.x);
    data.put_int("y", pos.0.y);
    data.put_int("z", pos.0.z);
    let entity: Arc<dyn BlockEntity> = Arc::new(ContentBlockEntity::from_saved(id, &data, pos));
    let Some(world_arc) = world.weak_self().and_then(|weak| weak.upgrade()) else {
        return Outcome::Deferred;
    };
    let converted = world
        .level
        .read_chunk_sync(&chunk_pos, |chunk| {
            if chunk.has_pending_post_processing() {
                return false;
            }
            let mut pending = chunk
                .pending_block_entities
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut entities = world.block_entities.entry(chunk_pos).or_default();
            let write = || {
                chunk
                    .set_block_absolute_y_if(
                        relative.x as usize,
                        relative.y,
                        relative.z as usize,
                        destination,
                        |current| current == state,
                    )
                    .is_some()
            };
            if let Some(previous) = previous {
                if !entities
                    .get(&pos)
                    .is_some_and(|current| Arc::ptr_eq(current, previous))
                {
                    return false;
                }
                let Some(chest) = previous.as_any().downcast_ref::<ChestBlockEntity>() else {
                    return false;
                };
                // The pending copy can include metadata that vanilla does not
                // understand. Do not replace a live record if that differs.
                if pending.get(&pos) != Some(source) {
                    return false;
                }
                let mut loot = chest
                    .loot_table
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let items = chest
                    .items
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if loot.as_deref() != source.get_string("LootTable")
                    || chest.loot_table_seed != source.get_long("LootTableSeed").unwrap_or(0)
                    || items.iter().any(|stack| !stack.is_empty())
                    || chest
                        .inventory_access
                        .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
                        .is_err()
                {
                    return false;
                }
                if !write() {
                    chest.inventory_access.store(0, Ordering::Release);
                    return false;
                }
                // No stale Arc can obtain a vanilla inventory or unpack this key.
                *loot = None;
            } else if pending.get(&pos) != Some(source) || entities.contains_key(&pos) || !write() {
                return false;
            }
            // State CAS rejects changed blocks. Pending and entity-map locks prevent
            // a lazy materialization or snapshot from observing the intermediate.
            entities.insert(pos, entity.clone());
            pending.insert(pos, data.clone());
            chunk.mark_dirty(true);
            true
        })
        .unwrap_or(false);
    if !converted {
        return Outcome::Deferred;
    }
    // Notify only after both representations have been published.
    world_arc.on_block_state_set(
        &pos,
        state,
        destination,
        BlockFlags::NOTIFY_LISTENERS
            | BlockFlags::NOTIFY_NEIGHBORS
            | BlockFlags::SKIP_BLOCK_ADDED_CALLBACK
            | BlockFlags::SKIP_BLOCK_ENTITY_REPLACED_CALLBACK,
        true,
    );
    Outcome::Converted
}
