use std::{collections::BTreeMap, io::Write};

use crate::{ClientPacket, WritingError, ser::NetworkWriteExt};

use crate::codec::var_int::VarInt;
use pumpkin_data::{
    packet::clientbound::config::UPDATE_TAGS,
    tag::{RegistryKey, get_registry_key_tags},
};
use pumpkin_macros::java_packet;
use pumpkin_util::version::JavaMinecraftVersion;

#[java_packet(UPDATE_TAGS)]
pub struct CUpdateTags<'a> {
    item_tags: Option<&'a BTreeMap<String, Vec<u16>>>,
    pub tags: &'a [pumpkin_data::tag::RegistryKey],
}

impl<'a> CUpdateTags<'a> {
    #[must_use]
    pub const fn new(tags: &'a [RegistryKey]) -> Self {
        Self {
            tags,
            item_tags: None,
        }
    }

    #[must_use]
    pub fn with_item_tags(mut self, tags: &'a BTreeMap<String, Vec<u16>>) -> Self {
        self.item_tags = Some(tags);
        self
    }
}

impl ClientPacket for CUpdateTags<'_> {
    fn write_packet_data(
        &self,
        mut write: impl Write,
        version: &JavaMinecraftVersion,
    ) -> Result<(), WritingError> {
        let valid_keys: Vec<_> = self
            .tags
            .iter()
            .copied()
            .filter(|key| key.is_valid_for_version(*version))
            .collect();

        write.write_list(&valid_keys, |p, &registry_key| {
            p.write_string(&format!("minecraft:{}", registry_key.identifier_string()))?;

            if registry_key == RegistryKey::Item
                && *version >= JavaMinecraftVersion::V_26_3
                && let Some(tags) = self.item_tags.filter(|tags| !tags.is_empty())
            {
                p.write_var_int(&VarInt(tags.len() as i32))?;
                for (key, ids) in tags {
                    p.write_string_bounded(key, u16::MAX as usize)?;
                    p.write_list(ids, |p, &id| p.write_var_int(&VarInt::from(id)))?;
                }
                return Ok(());
            }

            let Some(values) = get_registry_key_tags(*version, registry_key) else {
                // no tags defined for that registry key in this version
                // write an empty list and continue
                p.write_var_int(&VarInt::from(0))?;
                return Ok(());
            };
            p.write_var_int(&values.len().try_into().map_err(|_| {
                WritingError::Message(format!("{} isn't representable as a VarInt", values.len()))
            })?)?;

            for (key, values) in values.entries() {
                // This is technically a `ResourceLocation` but same thing
                p.write_string_bounded(key, u16::MAX as usize)?;
                p.write_list(values.1, |p, &id| p.write_var_int(&VarInt::from(id)))?;
            }

            Ok(())
        })
    }
}
