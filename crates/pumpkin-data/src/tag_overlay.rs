//! Tag memberships that datapacks add on top of the generated vanilla tags, including the ones in
//! each mod's `data/`. Tag checks in [`crate::tag::Taggable`] consult them.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::tag::RegistryKey;

type Memberships = HashMap<(RegistryKey, u16), Box<[Box<str>]>>;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static EXTRA: RwLock<Option<Memberships>> = RwLock::new(None);

/// Replaces the added memberships: for each registry entry, the full names (`namespace:path`) of
/// the tags it joins beyond the generated ones.
pub fn set(memberships: HashMap<(RegistryKey, u16), Vec<String>>) {
    let memberships: Memberships = memberships
        .into_iter()
        .map(|(entry, tags)| {
            (
                entry,
                tags.into_iter().map(String::into_boxed_str).collect(),
            )
        })
        .collect();
    let active = !memberships.is_empty();
    *EXTRA
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(memberships);
    ACTIVE.store(active, Ordering::Release);
}

/// The shared `c:` tag a Forge 1.20 convention tag (`forge:ingots/iron`) became. Ported data
/// still names the Forge ones.
#[must_use]
pub fn convention_tag(tag: &str) -> Option<String> {
    tag.strip_prefix("forge:").map(|path| format!("c:{path}"))
}

/// Whether a datapack added entry `id` of `key` to `tag` (`namespace:path`).
#[must_use]
pub fn contains(key: RegistryKey, id: u16, tag: &str) -> bool {
    if !ACTIVE.load(Ordering::Acquire) {
        return false;
    }
    EXTRA
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|memberships| memberships.get(&(key, id)))
        .is_some_and(|tags| tags.iter().any(|t| **t == *tag))
}

#[cfg(all(test, feature = "item"))]
mod tests {
    use crate::item::Item;
    use crate::tag::Taggable;

    #[test]
    fn forge_convention_tags_resolve_as_shared_tags() {
        assert_eq!(
            Item::IRON_NUGGET.is_tagged_with("#forge:nuggets/iron"),
            Some(true)
        );
        assert_eq!(
            Item::GOLD_NUGGET.is_tagged_with("forge:nuggets/iron"),
            Some(false)
        );
        assert_eq!(Item::IRON_NUGGET.is_tagged_with("forge:no_such_tag"), None);
    }
}
