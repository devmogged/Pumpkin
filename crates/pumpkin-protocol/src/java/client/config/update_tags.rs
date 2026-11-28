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
    climbable_carriers: &'a [u16],
    pub tags: &'a [pumpkin_data::tag::RegistryKey],
}

impl<'a> CUpdateTags<'a> {
    #[must_use]
    pub const fn new(tags: &'a [RegistryKey]) -> Self {
        Self {
            tags,
            item_tags: None,
            climbable_carriers: &[],
        }
    }

    #[must_use]
    pub const fn with_climbable_carriers(mut self, ids: &'a [u16]) -> Self {
        self.climbable_carriers = ids;
        self
    }

    #[must_use]
    pub const fn with_item_tags(mut self, tags: &'a BTreeMap<String, Vec<u16>>) -> Self {
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
                if registry_key == RegistryKey::Block
                    && *key == pumpkin_data::tag::Block::MINECRAFT_CLIMBABLE.2
                    && *version >= JavaMinecraftVersion::V_26_3
                    && !self.climbable_carriers.is_empty()
                {
                    let mut ids = values.1.to_vec();
                    for &id in self.climbable_carriers {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    p.write_list(&ids, |p, &id| p.write_var_int(&VarInt::from(id)))?;
                } else {
                    p.write_list(values.1, |p, &id| p.write_var_int(&VarInt::from(id)))?;
                }
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ser::NetworkReadExt;
    use pumpkin_data::BlockId;

    #[test]
    fn adds_only_projected_ids_to_the_climbable_tag() -> Result<(), Box<dyn std::error::Error>> {
        let carrier = BlockId::WAXED_COPPER_TRAPDOOR.as_u16();
        let keys = [RegistryKey::Block];
        let mut packet = Vec::new();
        CUpdateTags::new(&keys)
            .with_climbable_carriers(&[carrier, carrier])
            .write_packet_data(&mut packet, &JavaMinecraftVersion::V_26_3)?;

        let mut input = packet.as_slice();
        assert_eq!(input.get_var_int()?.0, 1);
        assert_eq!(&*input.get_str()?, "minecraft:block");
        let count = input.get_var_int()?.0;
        let mut found = false;
        for _ in 0..count {
            let name = input.get_str()?;
            let size = input.get_var_int()?.0;
            let values: Vec<_> = (0..size)
                .map(|_| input.get_var_int().map(|value| value.0))
                .collect::<Result<_, _>>()?;
            if &*name == "minecraft:climbable" {
                assert!(values.contains(&i32::from(BlockId::LADDER.as_u16())));
                assert_eq!(
                    values
                        .iter()
                        .filter(|&&id| id == i32::from(carrier))
                        .count(),
                    1
                );
                found = true;
            }
        }
        assert!(found);
        assert!(input.is_empty());
        Ok(())
    }
}
