use std::sync::Arc;

use crate::entity::EntityBase;
use crate::entity::player::Player;
use pumpkin_macros::{Event, cancellable};
use pumpkin_protocol::java::server::play::ActionType;
use pumpkin_util::{Hand, math::vector3::Vector3};

use super::PlayerEvent;

/// Event that is triggered when a player interacts with an entity.
///
/// This event is fired for all entity interaction types: interact (right-click),
/// attack (left-click), and interact-at (right-click at specific position).
/// It can be cancelled to prevent the default interaction behavior.
#[cancellable]
#[derive(Event, Clone)]
pub struct PlayerInteractEntityEvent {
    /// The player who performed the interaction.
    pub player: Arc<Player>,

    /// The entity that was interacted with.
    pub target: Arc<dyn EntityBase>,

    /// The type of interaction (Interact, Attack, or `InteractAt`).
    pub action: ActionType,

    /// The position on the entity that was clicked (only for `InteractAt`).
    pub target_position: Option<Vector3<f64>>,

    /// Whether the player was sneaking during the interaction.
    pub sneaking: bool,

    /// The hand the client used; the main hand for attacks.
    pub hand: Hand,
}

impl PlayerInteractEntityEvent {
    pub fn new(
        player: &Arc<Player>,
        target: Arc<dyn EntityBase>,
        action: ActionType,
        target_position: Option<Vector3<f64>>,
        sneaking: bool,
    ) -> Self {
        Self {
            player: Arc::clone(player),
            target,
            action,
            target_position,
            sneaking,
            hand: Hand::Right,
            cancelled: false,
        }
    }

    #[must_use]
    pub const fn with_hand(mut self, hand: Hand) -> Self {
        self.hand = hand;
        self
    }
}

impl PlayerEvent for PlayerInteractEntityEvent {
    fn get_player(&self) -> &Arc<Player> {
        &self.player
    }
}
