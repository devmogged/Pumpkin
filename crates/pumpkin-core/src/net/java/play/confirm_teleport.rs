#[allow(clippy::wildcard_imports)]
use super::*;

impl JavaClient {
    pub fn handle_confirm_teleport(&self, player: &Player, confirm_teleport: &SConfirmTeleport) {
        let mut awaiting_teleport = player
            .awaiting_teleport
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Like vanilla, ignore a confirmation of an older teleport (one superseded before the
        // client answered, e.g. /tp right after joining) instead of kicking the player.
        if let Some((id, position)) = awaiting_teleport.as_ref()
            && id == &confirm_teleport.teleport_id
        {
            // We should set the position now to what we requested in the teleport packet.
            // This may fix issues when the client sends the position while being teleported.
            player.get_entity().set_pos(*position);
            *awaiting_teleport = None;
        }
    }
}
