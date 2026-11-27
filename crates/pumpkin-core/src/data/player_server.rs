use crate::{
    entity::{NBTStorage, player::Player},
    server::Server,
};
use crossbeam::atomic::AtomicCell;
use pumpkin_data::item_stack::ItemStack;
use pumpkin_inventory::screen_handler::ScreenHandler;
use pumpkin_nbt::compound::NbtCompound;
use pumpkin_world::data::player_data::{PlayerDataError, PlayerDataStorage, SnapshotOutcome};
use std::sync::{Arc, OnceLock, PoisonError, TryLockError};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tracing::{debug, error};

const MENU_CHECKPOINT: &str = "moddedmc:menu_checkpoint";
const MENU_CURSOR: &str = "moddedmc:menu_cursor";
const MENU_PROOF: &str = "moddedmc:menu_checkpoint_proof";
fn read_proof(nbt: &NbtCompound) -> Option<[u8; 32]> {
    if nbt.get(MENU_PROOF).is_none() {
        return Some([0; 32]);
    }
    let encoded = nbt.get_string(MENU_PROOF)?;
    if encoded.len() != 64 || !encoded.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut proof = [0; 32];
    for (i, byte) in proof.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&encoded[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(proof)
}
fn has_orphaned_menu_metadata(nbt: &NbtCompound) -> bool {
    nbt.get(MENU_CHECKPOINT).is_none()
        && [MENU_PROOF, "moddedmc:menu_lineage", MENU_CURSOR]
            .iter()
            .any(|key| nbt.get(key).is_some())
}
type RecoverPlayerData = fn(&Player) -> bool;
type PlayerDataSaved = fn(&Path, uuid::Uuid, i64, [u8; 32], &NbtCompound) -> bool;
static PLAYER_DATA_HOOKS: OnceLock<(RecoverPlayerData, PlayerDataSaved)> = OnceLock::new();
type SerializePlayerLineage = fn(&Path, uuid::Uuid, i64, [u8; 32], &mut NbtCompound) -> bool;
static PLAYER_LINEAGE_HOOK: OnceLock<SerializePlayerLineage> = OnceLock::new();

pub fn install_player_lineage_hook(hook: SerializePlayerLineage) -> bool {
    PLAYER_LINEAGE_HOOK.set(hook).is_ok()
}

/// Register embedder recovery before player admission and acknowledgement after
/// a complete player file has been replaced. A failed recovery denies login.
pub fn install_player_data_hooks(recover: RecoverPlayerData, saved: PlayerDataSaved) -> bool {
    PLAYER_DATA_HOOKS.set((recover, saved)).is_ok()
}

fn acknowledge(
    root: &Path,
    uuid: uuid::Uuid,
    revision: i64,
    proof: [u8; 32],
    nbt: &NbtCompound,
) -> Result<(), PlayerDataError> {
    if revision > 0
        && let Some((_, saved)) = PLAYER_DATA_HOOKS.get()
    {
        if !saved(root, uuid, revision, proof, nbt) {
            return Err(PlayerDataError::Nbt(
                "cannot acknowledge durable player checkpoint".into(),
            ));
        }
    }
    Ok(())
}

/// A superseding save is not itself an acknowledgement: inspect the newer
/// durable player file and acknowledge only revisions it actually contains.
fn acknowledge_snapshot(
    storage: &PlayerDataStorage,
    root: &Path,
    uuid: uuid::Uuid,
    revision: i64,
    outcome: SnapshotOutcome,
) -> Result<(), PlayerDataError> {
    match outcome {
        SnapshotOutcome::Written | SnapshotOutcome::Superseded if revision > 0 => {
            // The file was replaced by this snapshot, but another save can
            // replace it before a slow completion callback, or this attempt
            // may itself have been superseded. The newest disk revision must
            // be forwarded, never the stale snapshot revision.
            let (exists, nbt) = storage.load_player_data(&uuid)?;
            if exists {
                let persisted = nbt
                    .get_long(MENU_CHECKPOINT)
                    .filter(|value| *value >= revision)
                    .ok_or_else(|| {
                        PlayerDataError::Nbt("missing or stale persisted checkpoint".into())
                    })?;
                let proof = read_proof(&nbt).ok_or_else(|| {
                    PlayerDataError::Nbt("invalid persisted checkpoint proof".into())
                })?;
                acknowledge(root, uuid, persisted, proof, &nbt)?;
            } else {
                return Err(PlayerDataError::Nbt("missing persisted player file".into()));
            }
        }
        SnapshotOutcome::Written | SnapshotOutcome::Superseded | SnapshotOutcome::Disabled => (),
    }
    Ok(())
}

/// Helper for managing player data in the server context.
///
/// This struct provides server-wide access to the `PlayerDataStorage` and
/// convenience methods for player handling.
pub struct ServerPlayerData {
    storage: Arc<PlayerDataStorage>,
    save_interval: Duration,
    last_save: AtomicCell<Instant>,
}

impl ServerPlayerData {
    /// Creates a new `ServerPlayerData` with specified configuration.
    pub fn new(data_path: impl Into<PathBuf>, save_interval: Duration, enabled: bool) -> Self {
        Self {
            storage: Arc::new(PlayerDataStorage::new(data_path, enabled)),
            save_interval,
            last_save: AtomicCell::new(Instant::now()),
        }
    }

    #[must_use]
    pub fn is_save_enabled(&self) -> bool {
        self.storage.is_save_enabled()
    }

    /// Restore the ordinary player file's menu revision and cursor, then let
    /// the embedder recover any newer jointly committed inventory image.
    pub fn recover_player(&self, player: &Player, nbt: Option<&NbtCompound>) -> bool {
        if nbt.is_some_and(has_orphaned_menu_metadata) {
            error!(
                "Cannot admit player {} with orphaned native menu metadata",
                player.gameprofile.id
            );
            return false;
        }
        if let Some(nbt) = nbt
            && nbt.get(MENU_CHECKPOINT).is_some()
        {
            let Some(revision) = nbt
                .get_long(MENU_CHECKPOINT)
                .filter(|revision| *revision > 0)
            else {
                return false;
            };
            let cursor = if nbt.get(MENU_CURSOR).is_some() {
                let Some(stack) = nbt
                    .get_compound(MENU_CURSOR)
                    .and_then(ItemStack::read_item_stack)
                else {
                    return false;
                };
                stack
            } else {
                ItemStack::EMPTY.clone()
            };
            let handler = player
                .player_screen_handler
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            *handler
                .get_behaviour()
                .cursor_stack
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = cursor;
            let Some(proof) = read_proof(nbt).filter(|proof| *proof != [0; 32]) else {
                return false;
            };
            player.native_menu_checkpoint_proof.store(proof);
            player.native_menu_checkpoint.store(revision);
        }
        if !PLAYER_DATA_HOOKS
            .get()
            .is_none_or(|(recover, _)| recover(player))
        {
            return false;
        }
        // A recovered journal image must reach the ordinary player file and
        // retire before the player can perform unrelated gameplay mutations.
        if player.native_menu_checkpoint.load()
            > nbt
                .and_then(|data| data.get_long(MENU_CHECKPOINT))
                .unwrap_or(0)
        {
            if let Err(error) = self.extract_data_and_save_player(player) {
                error!(
                    "Cannot persist recovered player {}: {error}",
                    player.gameprofile.id
                );
                return false;
            }
        }
        true
    }

    /// A menu click holds its screen lock from trial through inventory commit.
    /// Capture the cursor and revision under the same lock as the inventory
    /// image so a periodic save cannot acknowledge a half-applied click.
    fn snapshot(&self, player: &Player) -> Result<(u64, NbtCompound), PlayerDataError> {
        loop {
            let screen = player
                .current_screen_handler
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            let handler = screen.lock().unwrap_or_else(PoisonError::into_inner);
            // Some screen operations take the active-screen lock first. Never
            // wait on it while holding the individual handler lock.
            let active = match player.current_screen_handler.try_lock() {
                Ok(active) => active,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => continue,
            };
            if !Arc::ptr_eq(&screen, &active) {
                continue;
            }
            let revision = self.storage.reserve_snapshot();
            let mut nbt = NbtCompound::new();
            player.write_nbt(&mut nbt);
            let checkpoint = player.native_menu_checkpoint.load();
            if checkpoint > 0 {
                nbt.put_long(MENU_CHECKPOINT, checkpoint);
                let proof = player.native_menu_checkpoint_proof.load();
                if proof != [0; 32] {
                    nbt.put_string(
                        MENU_PROOF,
                        proof
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect::<String>(),
                    );
                }
                let cursor = handler
                    .get_behaviour()
                    .cursor_stack
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if !cursor.is_empty() {
                    let encoded = cursor.to_nbt().ok_or_else(|| {
                        PlayerDataError::Nbt("cannot encode nonempty menu cursor".to_string())
                    })?;
                    nbt.put_compound(MENU_CURSOR, encoded);
                }
            }
            if checkpoint > 0 {
                let Some(serialize) = PLAYER_LINEAGE_HOOK.get() else {
                    return Err(PlayerDataError::Nbt(
                        "menu checkpoint exists without a durable lineage serializer".into(),
                    ));
                };
                if !serialize(
                    &player.world().level.level_folder.root_folder,
                    player.gameprofile.id,
                    checkpoint,
                    player.native_menu_checkpoint_proof.load(),
                    &mut nbt,
                ) {
                    return Err(PlayerDataError::Nbt(
                        "cannot serialize durable menu lineage".into(),
                    ));
                }
            }
            return Ok((revision, nbt));
        }
    }

    /// Handles a player leaving the server.
    ///
    /// This function saves player data when they disconnect.
    ///
    /// # Arguments
    ///
    /// * `player` - The player who left.
    ///
    /// # Returns
    ///
    /// A Result indicating success or the error that occurred.
    pub fn handle_player_leave(&self, player: &Arc<Player>) -> Result<(), PlayerDataError> {
        player
            .player_screen_handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .on_closed(player.as_ref());
        player.on_handled_screen_closed();

        self.extract_data_and_save_player(player)
    }

    /// Performs periodic maintenance tasks.
    ///
    /// This function is called synchronously on the server tick loop to check
    /// if it is time to save player data.
    pub fn tick(&self, server: &Server) {
        let now = Instant::now();

        // Only save players periodically based on save_interval
        let last_save = self.last_save.load();
        let should_save = now.duration_since(last_save) >= self.save_interval;

        if should_save && self.storage.is_save_enabled() {
            self.last_save.store(now);
            // Snapshot all online players periodically across all worlds
            let mut snapshots = Vec::new();
            for world in server.worlds.load().iter() {
                for player in world.players.load().iter() {
                    let (revision, nbt) = match self.snapshot(player) {
                        Ok(snapshot) => snapshot,
                        Err(e) => {
                            error!("Cannot snapshot player {}: {e}", player.gameprofile.id);
                            continue;
                        }
                    };
                    let root = world.level.level_folder.root_folder.clone();
                    snapshots.push((player.gameprofile.id, revision, root, nbt));
                }
            }

            if snapshots.is_empty() {
                return;
            }

            let storage = self.storage.clone();
            // The tick runs on a simulation thread with no Tokio context, so use the
            // server's runtime handle instead of `tokio::task::spawn_blocking`.
            server.runtime.spawn_blocking(move || {
                for (uuid, revision, root, nbt) in snapshots {
                    let checkpoint = nbt.get_long(MENU_CHECKPOINT).unwrap_or(0);
                    match storage.save_player_snapshot(&uuid, revision, nbt) {
                        Ok(outcome) => {
                            if let Err(e) =
                                acknowledge_snapshot(&storage, &root, uuid, checkpoint, outcome)
                            {
                                error!("Failed to verify saved checkpoint for {uuid}: {e}");
                            }
                        }
                        Err(e) => error!("Failed to save player data for {uuid}: {e}"),
                    }
                }
                debug!("Periodic player data save completed");
            });
        }
    }

    /// Saves all players' data immediately.
    ///
    /// This function immediately saves all online players' data to disk.
    /// Useful for server shutdown or backup operations.
    pub fn save_all_players(&self, server: &Server) -> Result<(), PlayerDataError> {
        let mut total_players = 0;

        // Save players from all worlds
        for world in server.worlds.load().iter() {
            for player in world.players.load().iter() {
                self.extract_data_and_save_player(player)?;
                total_players += 1;
            }
        }

        debug!("Saved data for {total_players} online players");
        Ok(())
    }

    /// Loads player data and applies it to a player.
    ///
    /// This function loads a player's data and applies it to their Player instance.
    /// For new players, it creates default data without errors.
    ///
    /// # Arguments
    ///
    /// * `player` - The player to load data for and apply to.
    ///
    /// # Returns
    ///
    /// A Result indicating success or the error that occurred.
    pub fn load_data(&self, uuid: &uuid::Uuid) -> Result<Option<NbtCompound>, PlayerDataError> {
        let result = self.storage.load_player_data(uuid);

        match result {
            Ok((should_load, data)) => {
                if !should_load {
                    // No data to load, continue with default data
                    return Ok(None);
                }
                Ok(Some(data))
            }
            Err(e) => {
                if self.storage.is_save_enabled() {
                    // Only log as error if player data saving is enabled
                    error!("Error loading player data for {uuid}: {e}");
                    return Err(e);
                } else {
                    // Otherwise just log as info since it's expected
                    debug!("Not loading player data for {uuid} (saving disabled)");
                }
                Ok(None)
            }
        }
    }

    /// Extracts and saves data from a player.
    ///
    /// This function extracts NBT data from a player and saves it to disk.
    ///
    /// # Arguments
    ///
    /// * `player` - The player to extract and save data for.
    ///
    /// # Returns
    ///
    /// A Result indicating success or the error that occurred.
    pub fn extract_data_and_save_player(&self, player: &Player) -> Result<(), PlayerDataError> {
        if !self.storage.is_save_enabled() {
            return Ok(());
        }

        let uuid = player.gameprofile.id;
        let (revision, nbt) = self.snapshot(player)?;
        let checkpoint = nbt.get_long(MENU_CHECKPOINT).unwrap_or(0);
        let outcome = self.storage.save_player_snapshot(&uuid, revision, nbt)?;
        acknowledge_snapshot(
            &self.storage,
            &player.world().level.level_folder.root_folder,
            uuid,
            checkpoint,
            outcome,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::data::player_server::{ServerPlayerData, has_orphaned_menu_metadata};
    use pumpkin_nbt::{compound::NbtCompound, tag::NbtTag};
    use pumpkin_world::data::player_data::PlayerDataStorage;
    use std::time::Duration;
    use std::time::Instant;
    use tempfile::tempdir;
    use uuid::Uuid;

    #[test]
    fn login_rejects_orphaned_native_menu_metadata_but_accepts_legacy_player_data() {
        assert!(!has_orphaned_menu_metadata(&NbtCompound::new()));

        let mut valid_checkpoint = NbtCompound::new();
        valid_checkpoint.put_long("moddedmc:menu_checkpoint", 7);
        valid_checkpoint.put_string("moddedmc:menu_checkpoint_proof", "00".repeat(32));
        assert!(!has_orphaned_menu_metadata(&valid_checkpoint));

        for (index, key) in [
            "moddedmc:menu_checkpoint_proof",
            "moddedmc:menu_lineage",
            "moddedmc:menu_cursor",
        ]
        .into_iter()
        .enumerate()
        {
            let mut orphaned = NbtCompound::new();
            match index {
                0 => orphaned.put_string(key, "00".repeat(32)),
                1 => orphaned.put(key, NbtTag::List(Vec::new())),
                _ => orphaned.put_compound(key, NbtCompound::new()),
            }
            assert!(has_orphaned_menu_metadata(&orphaned), "orphaned {key}");
        }
    }

    #[tokio::test]
    async fn player_data_storage_new() {
        // Create a temporary directory for testing
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let storage = PlayerDataStorage::new(path.clone(), true);

        assert_eq!(storage.get_data_path().as_path(), path.as_path());
        // Note: save_enabled might be configured differently in your actual code
    }

    #[tokio::test]
    async fn player_data_storage_get_player_data_path() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let storage = PlayerDataStorage::new(path.clone(), true);

        let uuid = Uuid::new_v4();
        let expected_path = path.join(format!("{uuid}.dat"));

        assert_eq!(storage.get_player_data_path(&uuid), expected_path);
    }

    #[tokio::test]
    async fn player_data_storage_save_and_load() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let storage = PlayerDataStorage::new(path, true); // Ensure saving is enabled for this test

        let uuid = Uuid::new_v4();

        // Create test data
        let mut nbt = NbtCompound::new();
        nbt.put_string("TestKey", "TestValue".to_string());
        nbt.put_int("TestInt", 42);

        // Save the data
        storage.save_player_data(&uuid, nbt).unwrap();

        // Load the data
        let (load_success, loaded_nbt) = storage.load_player_data(&uuid).unwrap();

        assert!(load_success);
        assert_eq!(loaded_nbt.get_string("TestKey").unwrap(), "TestValue");
        assert_eq!(loaded_nbt.get_int("TestInt").unwrap(), 42);
    }

    #[tokio::test]
    async fn player_data_storage_load_nonexistent() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let storage = PlayerDataStorage::new(path, true); // Ensure saving is enabled for this test

        let uuid = Uuid::new_v4();

        // Try to load non-existent data
        let (load_success, empty_nbt) = storage.load_player_data(&uuid).unwrap();

        assert!(!load_success);
        assert_eq!(empty_nbt.child_tags.len(), 0);
    }

    #[tokio::test]
    async fn player_data_storage_disabled() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let storage = PlayerDataStorage::new(path, false);

        let uuid = Uuid::new_v4();
        let mut nbt = NbtCompound::new();
        nbt.put_string("TestKey", "TestValue".to_string());

        // Save should succeed but do nothing
        let save_result = storage.save_player_data(&uuid, nbt);
        assert!(save_result.is_ok());

        // Load should return empty data
        let (load_success, empty_nbt) = storage.load_player_data(&uuid).unwrap();
        assert!(!load_success);
        assert_eq!(empty_nbt.child_tags.len(), 0);
    }

    #[tokio::test]
    async fn server_player_data_new() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();
        let save_interval = Duration::from_mins(5);

        let player_data = ServerPlayerData::new(path, save_interval, true);

        assert_eq!(player_data.save_interval, save_interval);
        assert!(
            Instant::now().duration_since(player_data.last_save.load()) < Duration::from_secs(1)
        );
    }

    #[tokio::test]
    async fn player_data_file_structure() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().to_path_buf();

        let uuid = Uuid::new_v4();
        let storage = PlayerDataStorage::new(path, true);

        // Create and save player data
        let mut nbt = NbtCompound::new();
        nbt.put_string("name", "TestPlayer".to_string());
        nbt.put_int("level", 42);
        storage.save_player_data(&uuid, nbt).unwrap();

        // Verify the file exists
        let player_data_path = storage.get_player_data_path(&uuid);
        assert!(player_data_path.exists());

        // Load it again and verify content
        let (success, loaded_data) = storage.load_player_data(&uuid).unwrap();
        assert!(success);
        assert_eq!(loaded_data.get_string("name").unwrap(), "TestPlayer");
        assert_eq!(loaded_data.get_int("level").unwrap(), 42);
    }
}
