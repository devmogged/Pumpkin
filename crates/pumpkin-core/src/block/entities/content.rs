use std::{
    any::Any,
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
    data: Mutex<NbtCompound>,
}

impl ContentBlockEntity {
    #[must_use]
    pub fn new(id: &'static str, position: BlockPos) -> Self {
        Self {
            id,
            position,
            data: Mutex::new(NbtCompound::new()),
        }
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
