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
    file_access: RwLock<HashMap<Uuid, u64>>,
    next_snapshot: AtomicU64,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerDataError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("NBT error: {0}")]
    Nbt(String),
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
    ) -> Result<(), PlayerDataError> {
        // Skip saving if disabled in config
        if !self.is_save_enabled() {
            return Ok(());
        }

        let path = self.get_player_data_path(uuid);

        if let Err(e) = create_dir_all(&self.data_path) {
            error!("Failed to create player data directory for {uuid}: {e}");
            return Err(PlayerDataError::Io(e));
        }

        // Shutdown, disconnect and periodic saves can overlap for the same player.
        let mut pending = NamedTempFile::new_in(&self.data_path)?;
        if let Err(e) =
            pumpkin_nbt::nbt_compress::write_gzip_compound_tag(data, pending.as_file_mut())
        {
            error!("Failed to write compressed player data for {uuid}: {e}");
            return Err(PlayerDataError::Nbt(e.to_string()));
        }
        let pending = pending.into_temp_path();
        // Windows may reject replacement while another handle to the destination is open.
        let mut saved = self
            .file_access
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if saved.get(uuid).is_some_and(|latest| *latest >= revision) {
            return Ok(());
        }
        pending.persist(path).map_err(|e| e.error)?;
        saved.insert(*uuid, revision);
        debug!("Saved player data for {uuid} to disk");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_nbt::tag::NbtTag;
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
        storage
            .save_player_snapshot(&player, new_revision, new.clone())
            .unwrap();
        storage
            .save_player_snapshot(&player, old_revision, old)
            .unwrap();
        let (_, saved) = storage.load_player_data(&player).unwrap();
        assert_eq!(saved, new);
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
