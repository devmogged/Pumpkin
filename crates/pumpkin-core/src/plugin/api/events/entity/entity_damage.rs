use pumpkin_data::damage::DamageType;
use pumpkin_macros::{Event, cancellable};

/// An event that occurs when an entity takes damage.
#[cancellable]
#[derive(Event, Clone)]
pub struct EntityDamageEvent {
    /// The ID of the entity taking damage.
    pub entity_id: i32,

    /// The amount of damage taken.
    pub damage: f32,

    /// The damage type.
    pub damage_type: DamageType,

    /// The entity responsible for the damage (for example, a projectile's owner).
    pub causing_entity_id: Option<i32>,

    /// The entity directly dealing the damage (for example, a projectile).
    pub direct_entity_id: Option<i32>,
}

impl EntityDamageEvent {
    #[must_use]
    pub const fn new(entity_id: i32, damage_type: DamageType, damage_amount: f32) -> Self {
        Self {
            entity_id,
            damage: damage_amount,
            damage_type,
            causing_entity_id: None,
            direct_entity_id: None,
            cancelled: false,
        }
    }

    #[must_use]
    pub const fn with_source(mut self, causing: Option<i32>, direct: Option<i32>) -> Self {
        self.causing_entity_id = causing;
        self.direct_entity_id = direct;
        self
    }
}
