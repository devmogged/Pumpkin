//! Vanilla chest layout and click handling backed by a mod's committed inventory.

use std::{
    any::Any,
    sync::{Arc, Mutex, PoisonError, TryLockError, atomic::Ordering},
};

use pumpkin_data::{
    data_component_impl::{BundleContentsImpl, EquipmentSlot},
    item_stack::ItemStack,
    screen::WindowType,
};
use pumpkin_protocol::java::server::play::SlotActionType;

use crate::{
    SimpleInventory,
    entity_equipment::EntityEquipment,
    generic_container_screen_handler::GenericContainerScreenHandler,
    inventory::Inventory,
    player::player_inventory::PlayerInventory,
    screen_handler::{
        InventoryPlayer, ScreenHandler, ScreenHandlerBehaviour, SlotClickEffect, SlotClickEffects,
    },
    slot::{NormalSlot, Slot},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SlotAccess {
    pub take: bool,
    pub insert: bool,
}

/// Inventory shared by all open views of the same mod-owned record.
pub struct MenuInventory {
    stacks: Mutex<Vec<ItemStack>>,
    size: usize,
}

impl MenuInventory {
    #[must_use]
    pub fn new(stacks: Vec<ItemStack>) -> Option<Self> {
        let size = stacks.len();
        ((9..=54).contains(&size) && size.is_multiple_of(9)).then_some(Self {
            stacks: Mutex::new(stacks),
            size,
        })
    }

    #[must_use]
    pub const fn size(&self) -> usize {
        self.size
    }
}

struct MenuSlot {
    slot: NormalSlot,
    access: SlotAccess,
}

impl Slot for MenuSlot {
    fn get_inventory(&self) -> Arc<dyn Inventory> {
        self.slot.get_inventory()
    }
    fn get_index(&self) -> usize {
        self.slot.get_index()
    }
    fn set_id(&self, id: usize) {
        self.slot.set_id(id);
    }
    fn mark_dirty(&self) {
        self.slot.mark_dirty();
    }
    fn can_insert(&self, _: &ItemStack) -> bool {
        self.access.insert
    }
    fn can_take_items(&self, _: &dyn InventoryPlayer) -> bool {
        self.access.take
    }
    fn allow_modification(&self, _: &dyn InventoryPlayer) -> bool {
        self.access.take || self.access.insert
    }
}

struct PlayerSnapshot {
    main: [ItemStack; PlayerInventory::MAIN_SIZE],
    equipment: EntityEquipment,
    selected: u8,
}

impl PlayerSnapshot {
    fn capture(inventory: &PlayerInventory) -> Self {
        let main = inventory
            .main_inventory
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let equipment = inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        Self {
            main,
            equipment,
            selected: inventory.get_selected_slot(),
        }
    }

    fn project(&self, inventory: &PlayerInventory) {
        inventory
            .main_inventory
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .clone_from(&self.main);
        inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone_from(&self.equipment);
        inventory
            .selected_slot
            .store(self.selected, Ordering::Relaxed);
    }

    fn commit(
        self,
        inventory: &PlayerInventory,
        expected: &Self,
        persist: impl FnOnce() -> bool,
    ) -> bool {
        let mut equipment = inventory
            .entity_equipment
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Other player inventory operations can read the main inventory before equipment.
        let mut main = match inventory.main_inventory.try_write() {
            Ok(main) => main,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => return false,
        };
        if inventory.get_selected_slot() != expected.selected
            || !same_stacks(&main[..], &expected.main)
            || equipment.equipment.len() != expected.equipment.equipment.len()
            || equipment.equipment.iter().any(|(slot, stack)| {
                expected
                    .equipment
                    .equipment
                    .get(slot)
                    .is_none_or(|other| !same_stack(stack, other))
            })
            || !persist()
        {
            return false;
        }
        *main = self.main;
        *equipment = self.equipment;
        true
    }

    fn record_equipment_changes(
        &self,
        before: &Self,
        inventory: &PlayerInventory,
        effects: &mut Vec<SlotClickEffect>,
    ) {
        for slot in inventory.equipment_slots.values() {
            let stack = self.equipment.get(slot);
            if !same_stack(&before.equipment.get(slot), &stack) {
                effects.push(SlotClickEffect::Equipment(slot.clone(), stack));
            }
        }
        let main_hand = &self.main[usize::from(self.selected)];
        if !same_stack(&before.main[usize::from(before.selected)], main_hand) {
            effects.push(SlotClickEffect::Equipment(
                EquipmentSlot::MAIN_HAND,
                main_hand.clone(),
            ));
        }
    }
}

fn same_stack(left: &ItemStack, right: &ItemStack) -> bool {
    (left.is_empty() && right.is_empty())
        || (left.item_count == right.item_count && left.are_items_and_components_equal(right))
}

fn same_stacks(left: &[ItemStack], right: &[ItemStack]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| same_stack(left, right))
}

pub type PersistMenu =
    dyn Fn(&dyn InventoryPlayer, &[ItemStack], &[ItemStack]) -> bool + Send + Sync;

/// Runs a vanilla click against trial slots. A failed write preserves the live inventories,
/// cursor and world, and sends the old view back to the player.
pub struct NativeMenu {
    inner: GenericContainerScreenHandler,
    inventory: Arc<MenuInventory>,
    access: Vec<SlotAccess>,
    player_inventory: Arc<PlayerInventory>,
    trial_inventory: Arc<SimpleInventory>,
    trial_player: Arc<PlayerInventory>,
    persist: Arc<PersistMenu>,
}

impl NativeMenu {
    #[must_use]
    pub fn new(
        sync_id: u8,
        player: &dyn InventoryPlayer,
        inventory: Arc<MenuInventory>,
        access: Vec<SlotAccess>,
        persist: Arc<PersistMenu>,
    ) -> Option<Self> {
        if inventory.size() != access.len() {
            return None;
        }
        let rows = u8::try_from(inventory.size() / 9).ok()?;
        let window = match rows {
            1 => WindowType::Generic9x1,
            2 => WindowType::Generic9x2,
            3 => WindowType::Generic9x3,
            4 => WindowType::Generic9x4,
            5 => WindowType::Generic9x5,
            6 => WindowType::Generic9x6,
            _ => return None,
        };
        let player_inventory = player.get_inventory();
        let trial_inventory = Arc::new(SimpleInventory::new(inventory.size()));
        let trial_player = Arc::new(PlayerInventory::new(
            Arc::new(Mutex::new(EntityEquipment::new())),
            Arc::clone(&player_inventory.equipment_slots),
        ));
        let mut inner = GenericContainerScreenHandler::new(
            window,
            sync_id,
            &trial_player,
            trial_inventory.clone(),
            rows,
            9,
            player.is_spectator(),
        );
        for (index, &access) in access.iter().enumerate() {
            let slot = MenuSlot {
                slot: NormalSlot::new(trial_inventory.clone(), index),
                access,
            };
            slot.set_id(index);
            inner.get_behaviour_mut().slots[index] = Arc::new(slot);
        }
        let menu = Self {
            inner,
            inventory,
            access,
            player_inventory,
            trial_inventory,
            trial_player,
            persist,
        };
        menu.refresh();
        Some(menu)
    }

    fn project_slots(&self, stacks: &[ItemStack]) {
        let mut slots = self
            .trial_inventory
            .stacks
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        for (slot, stack) in slots.iter_mut().zip(stacks) {
            if !same_stack(slot, stack) {
                slot.clone_from(stack);
                if let Some(bundle) = slot.get_data_component_mut::<BundleContentsImpl>() {
                    bundle.selected_item = BundleContentsImpl::NO_SELECTED_ITEM_INDEX;
                }
            }
        }
    }

    fn refresh(&self) {
        self.project_slots(
            &self
                .inventory
                .stacks
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        PlayerSnapshot::capture(&self.player_inventory).project(&self.trial_player);
    }

    #[must_use]
    pub fn is_view_of(&self, inventory: &Arc<MenuInventory>) -> bool {
        Arc::ptr_eq(&self.inventory, inventory)
    }

    /// Restore matching slots first, then fit displaced and loose stacks wherever possible.
    /// A missing target slot means the source stack is loose.
    pub fn restore_matching(
        &mut self,
        player: &dyn InventoryPlayer,
        targets: &[(usize, Option<usize>)],
    ) -> bool {
        if player.is_spectator() || targets.is_empty() || targets.len() > self.inventory.size() {
            return false;
        }
        let mut sources = [false; 54];
        let mut destinations = [false; 41];
        for &(source, destination) in targets {
            if !self.access.get(source).is_some_and(|access| access.take)
                || std::mem::replace(&mut sources[source], true)
            {
                return false;
            }
            if let Some(slot) = destination
                && (slot >= destinations.len()
                    || (slot >= PlayerInventory::MAIN_SIZE
                        && !self.player_inventory.equipment_slots.contains_key(&slot))
                    || std::mem::replace(&mut destinations[slot], true))
            {
                return false;
            }
        }

        let mut stored = self
            .inventory
            .stacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let before = PlayerSnapshot::capture(&self.player_inventory);
        before.project(&self.trial_player);
        let mut proposed = stored.clone();
        let mut displaced = Vec::new();
        let mut loose = Vec::new();
        for &(source, destination) in targets {
            if proposed[source].is_empty() {
                continue;
            }
            let stack = std::mem::replace(&mut proposed[source], ItemStack::EMPTY.clone());
            if let Some(slot) = destination {
                displaced.push(self.trial_player.get_stack(slot));
                self.trial_player.set_stack(slot, stack);
            } else {
                loose.push(stack);
            }
        }
        if displaced.is_empty() && loose.is_empty() {
            return false;
        }
        displaced.append(&mut loose);
        displaced.retain_mut(|stack| {
            self.trial_player.insert_stack_anywhere(stack);
            !stack.is_empty()
        });
        let after = PlayerSnapshot::capture(&self.trial_player);
        let mut effects = Vec::new();
        after.record_equipment_changes(&before, &self.player_inventory, &mut effects);
        if !after.commit(&self.player_inventory, &before, || {
            (self.persist)(player, &proposed, &displaced)
        }) {
            drop(stored);
            self.refresh();
            return false;
        }
        self.project_slots(&proposed);
        *stored = proposed;
        drop(stored);
        for effect in effects {
            effect.apply(player);
        }
        self.inner.send_content_updates();
        true
    }
}

impl ScreenHandler for NativeMenu {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
    fn get_behaviour(&self) -> &ScreenHandlerBehaviour {
        self.inner.get_behaviour()
    }
    fn get_behaviour_mut(&mut self) -> &mut ScreenHandlerBehaviour {
        self.inner.get_behaviour_mut()
    }
    fn quick_move(&mut self, player: &dyn InventoryPlayer, slot: i32) -> ItemStack {
        self.inner.quick_move(player, slot)
    }

    fn on_closed(&mut self, player: &dyn InventoryPlayer) {
        self.inner.on_closed(player);
    }

    fn send_content_updates(&mut self) {
        self.refresh();
        self.inner.send_content_updates();
    }

    fn set_selected_bundle_item_index(&mut self, slot_index: i32, selected_item_index: i32) {
        let Ok(index) = usize::try_from(slot_index) else {
            return;
        };
        if index < self.inventory.size() {
            self.inner
                .set_selected_bundle_item_index(slot_index, selected_item_index);
        } else if let Some(slot) = self.inner.get_behaviour().slots.get(index) {
            let mut slots = self
                .player_inventory
                .main_inventory
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            if let Some(bundle) = slots
                .get_mut(slot.get_index())
                .and_then(|stack| stack.get_data_component_mut::<BundleContentsImpl>())
            {
                bundle.toggle_selected_item(selected_item_index);
            }
        }
    }

    fn on_slot_click(
        &mut self,
        slot: i32,
        button: i32,
        action: SlotActionType,
        player: &dyn InventoryPlayer,
    ) {
        if player.is_spectator() {
            return;
        }
        let mut stored = self
            .inventory
            .stacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let view_is_current = same_stacks(
            &stored,
            &self
                .trial_inventory
                .stacks
                .read()
                .unwrap_or_else(PoisonError::into_inner),
        );
        if !view_is_current {
            self.project_slots(&stored);
            return;
        }
        let before = PlayerSnapshot::capture(&self.player_inventory);
        before.project(&self.trial_player);
        let cursor = self.inner.get_behaviour().cursor_stack.clone();
        let old_cursor = cursor
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let mut effects = Vec::new();
        self.inner.on_slot_click_with_effects(
            slot,
            button,
            action,
            player,
            &mut SlotClickEffects::Staged {
                inventory: &self.trial_player,
                effects: &mut effects,
            },
        );
        let proposed = self
            .trial_inventory
            .stacks
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        let changed = !same_stacks(&stored, &proposed);
        let after = PlayerSnapshot::capture(&self.trial_player);
        after.record_equipment_changes(&before, &self.player_inventory, &mut effects);
        let committed = after.commit(&self.player_inventory, &before, || {
            !changed || (self.persist)(player, &proposed, &[])
        });
        if committed {
            if changed {
                stored.clone_from(&proposed);
            }
        } else {
            *cursor.lock().unwrap_or_else(PoisonError::into_inner) = old_cursor;
        }
        drop(proposed);
        drop(stored);
        if committed {
            for effect in effects {
                effect.apply(player);
            }
        } else {
            self.refresh();
            self.inner.sync_state();
        }
    }
}
