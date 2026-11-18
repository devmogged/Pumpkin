use std::{any::Any, path::Path};

use super::{LoaderError, PluginLoadFuture, PluginLoader, PluginUnloadFuture};

/// Owns plugins compiled into the server binary. It never loads files: built-in plugins are
/// handed to [`PluginManager::load_builtin_plugin`](crate::plugin::PluginManager::load_builtin_plugin)
/// directly, and they live as long as the process.
pub struct BuiltinPluginLoader;

impl PluginLoader for BuiltinPluginLoader {
    fn load<'a>(&'a self, _path: &'a Path) -> PluginLoadFuture<'a> {
        Box::pin(async { Err(LoaderError::EntrypointMissing) })
    }

    fn can_load(&self, _path: &Path) -> bool {
        false
    }

    fn unload(&self, _data: Box<dyn Any + Send + Sync>) -> PluginUnloadFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn can_unload(&self) -> bool {
        false
    }
}
