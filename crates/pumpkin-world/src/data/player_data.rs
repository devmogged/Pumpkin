use pumpkin_nbt::compound::NbtCompound;
use std::collections::HashMap;
use std::fs::{File, create_dir_all};
use std::io;
use std::path::PathBuf;
use std::sync::{
    PoisonError, RwLock,
    atomic::{AtomicU64, Ordering},
};
use tempfile::NamedTempFile;
use tracing::{debug, error};
use uuid::Uuid;

/// Manages the storage and retrieval of player data from disk and memory cache.
///
/// This struct provides functions to load and save player data to/from NBT files,
/// with a memory cache to handle player disconnections temporarily.
pub struct PlayerDataStorage {
    /// Path to the directory where player data is stored
    data_path: PathBuf,
    /// Whether player data saving is enabled
    save_enabled: bool,
    file_access: RwLock<HashMap<Uuid, (u64, i64)>>,
    next_snapshot: AtomicU64,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerDataError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("NBT error: {0}")]
    Nbt(String),
}

/// Only Written proves that this particular snapshot replaced the disk file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    Written,
    Superseded,
    Disabled,
}

impl PlayerDataStorage {
    /// Creates a new `PlayerDataStorage` with the specified data path and cache expiration time.
    pub fn new(data_path: impl Into<PathBuf>, enabled: bool) -> Self {
        let path = data_path.into();
        if !path.exists()
            && let Err(e) = create_dir_all(&path)
        {
            error!(
                "Failed to create player data directory at {}: {e}",
                path.display()
            );
        }

        Self {
            data_path: path,
            save_enabled: enabled,
            file_access: RwLock::new(HashMap::new()),
            next_snapshot: AtomicU64::new(1),
        }
    }

    #[must_use]
    pub const fn get_data_path(&self) -> &PathBuf {
        &self.data_path
    }

    #[must_use]
    pub const fn is_save_enabled(&self) -> bool {
        self.save_enabled
    }

    pub const fn set_save_enabled(&mut self, enabled: bool) {
        self.save_enabled = enabled;
    }

    /// Returns the path for a player's data file based on their UUID.
    #[must_use]
    pub fn get_player_data_path(&self, uuid: &Uuid) -> PathBuf {
        self.get_data_path().join(format!("{uuid}.dat"))
    }

    /// Loads player data from NBT file or cache.
    ///
    /// This function first checks if player data exists in the cache.
    /// If not, it attempts to load the data from a .dat file on disk.
    ///
    /// # Arguments
    ///
    /// * `uuid` - The UUID of the player to load data for.
    ///
    /// # Returns
    ///
    /// A Result containing either the player's NBT data or an error.
    pub fn load_player_data(&self, uuid: &Uuid) -> Result<(bool, NbtCompound), PlayerDataError> {
        // If player data saving is disabled, return empty data
        if !self.is_save_enabled() {
            return Ok((false, NbtCompound::new()));
        }

        let _guard = self
            .file_access
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        // If not in cache, load from disk
        let path = self.get_player_data_path(uuid);
        if !path.exists() {
            debug!("No player data file found for {uuid}");
            return Ok((false, NbtCompound::new()));
        }

        let file = match File::open(&path) {
            Ok(file) => file,
            Err(e) => {
                error!("Failed to open player data file for {uuid}: {e}");
                return Err(PlayerDataError::Io(e));
            }
        };

        match pumpkin_nbt::nbt_compress::read_gzip_compound_tag(file) {
            Ok(nbt) => {
                debug!("Loaded player data for {uuid} from disk");
                Ok((true, nbt))
            }
            Err(e) => {
                error!("Failed to read player data for {uuid}: {e}");
                Err(PlayerDataError::Nbt(e.to_string()))
            }
        }
    }

    /// Replaces a player's NBT file with a complete compressed snapshot.
    ///
    /// # Arguments
    ///
    /// * `uuid` - The UUID of the player to save data for.
    /// * `data` - The NBT compound data to save.
    ///
    /// # Returns
    ///
    /// A Result indicating success or the error that occurred.
    pub fn save_player_data(&self, uuid: &Uuid, data: NbtCompound) -> Result<(), PlayerDataError> {
        self.save_player_snapshot(uuid, self.reserve_snapshot(), data)
            .map(|_| ())
    }

    /// Reserve the ordering for a player snapshot before serializing the player's state.
    #[must_use]
    pub fn reserve_snapshot(&self) -> u64 {
        self.next_snapshot.fetch_add(1, Ordering::Relaxed)
    }

    /// Save captured player data unless a newer snapshot has already reached disk.
    pub fn save_player_snapshot(
        &self,
        uuid: &Uuid,
        revision: u64,
        data: NbtCompound,
    ) -> Result<SnapshotOutcome, PlayerDataError> {
        // Skip saving if disabled in config
        if !self.is_save_enabled() {
            return Ok(SnapshotOutcome::Disabled);
        }

        let path = self.get_player_data_path(uuid);

        if let Err(e) = create_dir_all(&self.data_path) {
            error!("Failed to create player data directory for {uuid}: {e}");
            return Err(PlayerDataError::Io(e));
        }

        // Shutdown, disconnect and periodic saves can overlap for the same player.
        let checkpoint = data.get_long("moddedmc:menu_checkpoint").unwrap_or(0);
        let mut pending = NamedTempFile::new_in(&self.data_path)?;
        if let Err(e) =
            pumpkin_nbt::nbt_compress::write_gzip_compound_tag(data, pending.as_file_mut())
        {
            error!("Failed to write compressed player data for {uuid}: {e}");
            return Err(PlayerDataError::Nbt(e.to_string()));
        }
        pending.as_file().sync_all()?;
        let pending = pending.into_temp_path();
        // Windows may reject replacement while another handle to the destination is open.
        let mut saved = self
            .file_access
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(&(sequence, durable_checkpoint)) = saved.get(uuid) {
            if sequence >= revision {
                return Ok(SnapshotOutcome::Superseded);
            }
            if durable_checkpoint > checkpoint {
                return Err(PlayerDataError::Nbt("player checkpoint regression".into()));
            }
        } else {
            // The process-local sequence resets on restart. The durable
            // transaction revision does not. Inspect the existing file under
            // the same exclusion as replacement; never overwrite a corrupt
            // or newer checkpoint on the first save of a new process.
            match File::open(&path) {
                Ok(file) => {
                    let prior = pumpkin_nbt::nbt_compress::read_gzip_compound_tag(file)
                        .map_err(|e| PlayerDataError::Nbt(e.to_string()))?;
                    if prior.get("moddedmc:menu_checkpoint").is_some() {
                        let durable = prior
                            .get_long("moddedmc:menu_checkpoint")
                            .filter(|value| *value > 0)
                            .ok_or_else(|| {
                                PlayerDataError::Nbt("invalid durable player checkpoint".into())
                            })?;
                        if durable > checkpoint {
                            return Err(PlayerDataError::Nbt(
                                "player checkpoint regression".into(),
                            ));
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        inject_player_save_crash("player_before_durable_replace");
        pending.persist(path).map_err(|e| e.error)?;
        inject_player_save_crash("player_after_durable_replace");
        saved.insert(*uuid, (revision, checkpoint));
        debug!("Saved player data for {uuid} to disk");
        Ok(SnapshotOutcome::Written)
    }

    /// Inspect the actual latest player file after a save was superseded.
    /// The caller must never infer durability merely from snapshot ordering:
    /// only a successfully decoded on-disk file proves the checkpoint reached
    /// storage, and a newer image may contain a strictly newer checkpoint.
    pub fn persisted_checkpoint_at_least(
        &self,
        uuid: &Uuid,
        minimum: i64,
        tag: &str,
    ) -> Result<bool, PlayerDataError> {
        Ok(minimum > 0
            && self
                .persisted_checkpoint_revision(uuid, tag)?
                .is_some_and(|saved| saved >= minimum))
    }

    /// Read the revision from the player file which actually reached disk.
    /// A missing checkpoint is not a durable acknowledgment. Invalid or
    /// non-positive checkpoint data is an error, not permission to proceed.
    pub fn persisted_checkpoint_revision(
        &self,
        uuid: &Uuid,
        tag: &str,
    ) -> Result<Option<i64>, PlayerDataError> {
        if !self.is_save_enabled() {
            return Ok(None);
        }
        let (exists, nbt) = self.load_player_data(uuid)?;
        if !exists || nbt.get(tag).is_none() {
            return Ok(None);
        }
        nbt.get_long(tag)
            .filter(|revision| *revision > 0)
            .map(Some)
            .ok_or_else(|| PlayerDataError::Nbt("invalid persisted player checkpoint".into()))
    }
}

/// Explicitly opt-in crash point used by the transaction recovery harness.
fn inject_player_save_crash(boundary: &str) {
    if std::env::var("MODDEDMC_CRASH_AT").is_ok_and(|selected| selected == boundary) {
        error!("Injected process termination at {boundary}");
        std::process::abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_nbt::tag::NbtTag;
    use std::process::Command;
    use std::sync::Barrier;

    #[test]
    fn delayed_snapshot_cannot_restore_older_player_data() {
        let directory = tempfile::tempdir().expect("player data directory");
        let storage = PlayerDataStorage::new(directory.path(), true);
        let player = Uuid::nil();
        let old_revision = storage.reserve_snapshot();
        let new_revision = storage.reserve_snapshot();
        let mut old = NbtCompound::new();
        old.put("InventoryMarker", NbtTag::Int(1));
        let mut new = NbtCompound::new();
        new.put("InventoryMarker", NbtTag::Int(2));
        assert_eq!(
            storage
                .save_player_snapshot(&player, new_revision, new.clone())
                .unwrap(),
            SnapshotOutcome::Written
        );
        assert_eq!(
            storage
                .save_player_snapshot(&player, old_revision, old)
                .unwrap(),
            SnapshotOutcome::Superseded
        );
        let (_, saved) = storage.load_player_data(&player).unwrap();
        assert_eq!(saved, new);
    }

    #[test]
    fn superseded_save_requires_actual_checkpoint_on_disk() {
        let directory = tempfile::tempdir().unwrap();
        let storage = PlayerDataStorage::new(directory.path(), true);
        let player = Uuid::new_v4();
        let older = storage.reserve_snapshot();
        let newer = storage.reserve_snapshot();
        let mut first = NbtCompound::new();
        first.put_long("moddedmc:menu_checkpoint", 7);
        let mut latest = NbtCompound::new();
        latest.put_long("moddedmc:menu_checkpoint", 9);
        assert_eq!(
            storage
                .save_player_snapshot(&player, newer, latest)
                .unwrap(),
            SnapshotOutcome::Written
        );
        assert_eq!(
            storage.save_player_snapshot(&player, older, first).unwrap(),
            SnapshotOutcome::Superseded
        );
        assert!(
            storage
                .persisted_checkpoint_at_least(&player, 7, "moddedmc:menu_checkpoint")
                .unwrap()
        );
        assert!(
            storage
                .persisted_checkpoint_at_least(&player, 9, "moddedmc:menu_checkpoint")
                .unwrap()
        );
        assert!(
            !storage
                .persisted_checkpoint_at_least(&player, 10, "moddedmc:menu_checkpoint")
                .unwrap()
        );
        assert!(
            !storage
                .persisted_checkpoint_at_least(&Uuid::new_v4(), 7, "moddedmc:menu_checkpoint")
                .unwrap()
        );
        assert_eq!(
            storage
                .persisted_checkpoint_revision(&player, "moddedmc:menu_checkpoint")
                .unwrap(),
            Some(9)
        );
        assert_eq!(
            storage
                .persisted_checkpoint_revision(&Uuid::new_v4(), "moddedmc:menu_checkpoint")
                .unwrap(),
            None
        );
    }

    #[test]
    fn corrupt_checkpoint_cannot_be_acknowledged_as_durable() {
        let directory = tempfile::tempdir().unwrap();
        let storage = PlayerDataStorage::new(directory.path(), true);
        let player = Uuid::new_v4();
        let mut nbt = NbtCompound::new();
        nbt.put_long("moddedmc:menu_checkpoint", -1);
        storage.save_player_data(&player, nbt).unwrap();
        assert!(
            storage
                .persisted_checkpoint_revision(&player, "moddedmc:menu_checkpoint")
                .is_err()
        );
        assert!(
            storage
                .persisted_checkpoint_at_least(&player, 1, "moddedmc:menu_checkpoint")
                .is_err()
        );
    }

    #[test]
    fn checkpoint_regression_is_rejected_across_storage_restart() {
        let directory = tempfile::tempdir().unwrap();
        let player = Uuid::new_v4();
        let image = |revision| {
            let mut nbt = NbtCompound::new();
            nbt.put_long("moddedmc:menu_checkpoint", revision);
            nbt
        };
        {
            let storage = PlayerDataStorage::new(directory.path(), true);
            storage.save_player_data(&player, image(7)).unwrap();
            assert!(storage.save_player_data(&player, image(6)).is_err());
        }
        let storage = PlayerDataStorage::new(directory.path(), true);
        assert!(storage.save_player_data(&player, image(6)).is_err());
        assert_eq!(storage.load_player_data(&player).unwrap().1, image(7));
        storage.save_player_data(&player, image(8)).unwrap();
        assert_eq!(storage.load_player_data(&player).unwrap().1, image(8));
        assert!(storage.save_player_data(&player, image(7)).is_err());
    }

    #[test]
    fn player_save_crash_child_process() {
        let (Ok(directory), Ok(uuid)) = (
            std::env::var("MODDEDMC_PLAYER_CRASH_DIRECTORY"),
            std::env::var("MODDEDMC_PLAYER_CRASH_UUID"),
        ) else {
            return;
        };
        let player = Uuid::parse_str(&uuid).unwrap();
        let storage = PlayerDataStorage::new(directory, true);
        let mut nbt = NbtCompound::new();
        nbt.put_int("InventoryMarker", 2);
        let _ = storage.save_player_data(&player, nbt);
    }

    #[test]
    fn player_file_crash_points_terminate_a_real_child_and_preserve_replace_boundary()
    -> std::io::Result<()> {
        for (boundary, expected_marker) in [
            ("player_before_durable_replace", 1),
            ("player_after_durable_replace", 2),
        ] {
            let directory = tempfile::tempdir()?;
            let player = Uuid::new_v4();
            let storage = PlayerDataStorage::new(directory.path(), true);
            let mut original = NbtCompound::new();
            original.put_int("InventoryMarker", 1);
            storage
                .save_player_data(&player, original)
                .map_err(std::io::Error::other)?;
            let child = Command::new(std::env::current_exe()?)
                .arg("--exact")
                .arg("data::player_data::tests::player_save_crash_child_process")
                .env("MODDEDMC_PLAYER_CRASH_DIRECTORY", directory.path())
                .env("MODDEDMC_PLAYER_CRASH_UUID", player.to_string())
                .env("MODDEDMC_CRASH_AT", boundary)
                .output()?;
            assert!(
                !child.status.success(),
                "{boundary} must abort the child process"
            );
            let (_, persisted) = storage
                .load_player_data(&player)
                .map_err(std::io::Error::other)?;
            assert_eq!(persisted.get_int("InventoryMarker"), Some(expected_marker));
        }
        Ok(())
    }

    #[test]
    fn player_saves_replace_complete_snapshots() {
        let dir = tempfile::tempdir().expect("player data directory");
        let storage = PlayerDataStorage::new(dir.path(), true);
        let uuid = Uuid::nil();
        let mut snapshot = NbtCompound::new();
        let mut seed = 42u32;
        let payload: Box<[_]> = (0..128 * 1024)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as i8
            })
            .collect();
        snapshot.put("Inventory", NbtTag::ByteArray(payload));
        storage
            .save_player_data(&uuid, snapshot.clone())
            .expect("initial player save");

        let ready = Barrier::new(3);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    ready.wait();
                    for _ in 0..8 {
                        storage
                            .save_player_data(&uuid, snapshot.clone())
                            .expect("concurrent player save");
                    }
                });
            }
            ready.wait();
            for _ in 0..16 {
                let (found, data) = storage.load_player_data(&uuid).expect("valid player save");
                assert!(found);
                assert_eq!(
                    data.get_byte_array("Inventory"),
                    snapshot.get_byte_array("Inventory")
                );
            }
        });
    }
}
