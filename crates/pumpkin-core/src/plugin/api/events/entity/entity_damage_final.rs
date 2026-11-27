use pumpkin_macros::Event;

use super::EntityDamageEvent;

/// Damage after armor and magic mitigation, before absorption and health loss.
#[derive(Event, Clone)]
pub struct EntityDamageFinalEvent {
    pub hit: EntityDamageEvent,
    /// Whether the original amount would exhaust both absorption and health.
    pub lethal: bool,
    /// Absorption before applying the hit.
    pub absorption: f32,
}
