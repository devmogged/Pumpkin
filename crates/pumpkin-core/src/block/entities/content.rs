use std::{
    any::Any,
    sync::atomic::{AtomicU64, Ordering},
    sync::{Mutex, PoisonError},
};

use pumpkin_nbt::compound::NbtCompound;
use pumpkin_util::math::position::BlockPos;

use super::BlockEntity;
use crate::world::World;
use pumpkin_data::BlockStateId;
use std::sync::OnceLock;

#[derive(Clone, Copy)]
pub enum Change {
    State(BlockStateId, BlockStateId),
    /// Replaced by a `BlockFlags::MOVED` write, such as a contraption picking the block up.
    Moved(BlockStateId, BlockStateId),
    Loaded(BlockStateId),
    Unloaded,
}

type Observer = fn(&World, BlockPos, Change);
static OBSERVER: OnceLock<Observer> = OnceLock::new();

pub fn observe(observer: Observer) -> bool {
    OBSERVER.set(observer).is_ok()
}

pub(crate) fn state_changed(
    world: &World,
    pos: BlockPos,
    old: BlockStateId,
    new: BlockStateId,
    moved: bool,
) {
    if old != new && (old.as_u16() >= BlockStateId::COUNT || new.as_u16() >= BlockStateId::COUNT) {
        let change = if moved {
            Change::Moved(old, new)
        } else {
            Change::State(old, new)
        };
        notify(world, pos, change);
    }
}

pub(crate) fn notify(world: &World, position: BlockPos, change: Change) {
    if let Some(observer) = OBSERVER.get() {
        observer(world, position, change);
    }
}

pub struct ContentBlockEntity {
    id: &'static str,
    position: BlockPos,
    /// Process-local identity; never persisted or reused for a replacement.
    incarnation: u64,
    data: Mutex<NbtCompound>,
}

static NEXT_INCARNATION: AtomicU64 = AtomicU64::new(1);

impl ContentBlockEntity {
    #[must_use]
    pub fn new(id: &'static str, position: BlockPos) -> Self {
        Self {
            id,
            position,
            incarnation: NEXT_INCARNATION.fetch_add(1, Ordering::Relaxed),
            data: Mutex::new(NbtCompound::new()),
        }
    }

    #[must_use]
    pub const fn incarnation(&self) -> u64 {
        self.incarnation
    }

    #[must_use]
    pub fn from_saved(id: &'static str, data: &NbtCompound, position: BlockPos) -> Self {
        let entity = Self::new(id, position);
        entity.set_data(data.clone());
        entity
    }

    pub fn set_data(&self, mut data: NbtCompound) {
        for key in ["id", "x", "y", "z"] {
            data.child_tags.remove(key);
        }
        *self.data.lock().unwrap_or_else(PoisonError::into_inner) = data;
    }

    /// Compare the full saved snapshot, then replace only the custom fields.
    /// The comparison and mutation share the entity data lock.
    pub fn compare_data(
        &self,
        expected: &NbtCompound,
        replacement: &NbtCompound,
    ) -> Option<NbtCompound> {
        let metadata_matches = |nbt: &NbtCompound| {
            nbt.get_string("id") == Some(self.id)
                && nbt.get_int("x") == Some(self.position.0.x)
                && nbt.get_int("y") == Some(self.position.0.y)
                && nbt.get_int("z") == Some(self.position.0.z)
        };
        if !metadata_matches(expected) || !metadata_matches(replacement) {
            return None;
        }
        let mut expected_fields = expected.clone();
        let mut replacement_fields = replacement.clone();
        for key in ["id", "x", "y", "z"] {
            expected_fields.child_tags.remove(key);
            replacement_fields.child_tags.remove(key);
        }
        let mut data = self.data.lock().unwrap_or_else(PoisonError::into_inner);
        if *data != expected_fields {
            return None;
        }
        *data = replacement_fields;
        Some(replacement.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_util::math::vector3::Vector3;

    #[test]
    fn compare_data_rejects_stale_snapshot_and_changed_identity() {
        let entity = ContentBlockEntity::new("test:container", BlockPos(Vector3::new(1, 2, 3)));
        let replacement =
            ContentBlockEntity::new("test:container", BlockPos(Vector3::new(1, 2, 3)));
        assert_ne!(entity.incarnation(), replacement.incarnation());
        let mut original = NbtCompound::new();
        entity.write_internal(&mut original);
        let mut first = original.clone();
        first.put_string("LootrId", "first".to_owned());
        assert_eq!(entity.compare_data(&original, &first), Some(first.clone()));
        let mut second = first.clone();
        second.put_string("LootrId", "second".to_owned());
        assert!(entity.compare_data(&original, &second).is_none());
        let mut wrong_position = second.clone();
        wrong_position.put_int("x", 2);
        assert!(entity.compare_data(&first, &wrong_position).is_none());
        assert_eq!(entity.compare_data(&first, &second), Some(second.clone()));
        let mut saved = NbtCompound::new();
        entity.write_internal(&mut saved);
        assert_eq!(saved, second);
    }
}

impl BlockEntity for ContentBlockEntity {
    fn write_nbt(&self, nbt: &mut NbtCompound) {
        nbt.child_tags.extend(
            self.data
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .child_tags
                .clone(),
        );
    }

    fn from_nbt(nbt: &NbtCompound, position: BlockPos) -> Self {
        let id = nbt
            .get_string("id")
            .and_then(pumpkin_data::content::entity_name)
            .unwrap_or("pumpkin:content");
        Self::from_saved(id, nbt, position)
    }

    fn resource_location(&self) -> &'static str {
        self.id
    }

    fn get_position(&self) -> BlockPos {
        self.position
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
