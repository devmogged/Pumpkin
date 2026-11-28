use pumpkin_macros::Event;

use super::EntityDamageEvent;

/// Notification after a living entity accepted a hit and applied its consequences.
#[derive(Event, Clone)]
pub struct EntityDamageAcceptedEvent {
    pub hit: EntityDamageEvent,
    pub health_damage: f32,
    pub absorbed_damage: f32,
}

/// Notification when native living damage returns false, including early immunity.
#[derive(Event, Clone)]
pub struct EntityDamageRejectedEvent {
    pub hit: EntityDamageEvent,
}
