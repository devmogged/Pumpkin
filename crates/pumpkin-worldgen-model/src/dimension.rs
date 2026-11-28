use serde::Deserialize;
use serde_json::Value;

/// Shared shape of a Minecraft dimension type definition.
#[derive(Deserialize)]
pub struct DimensionType {
    pub has_skylight: bool,
    pub has_ceiling: bool,
    pub ambient_light: f32,
    pub coordinate_scale: f64,
    pub min_y: i32,
    pub height: i32,
    pub logical_height: i32,
    pub infiniburn: String,
    pub monster_spawn_light_level: Value,
    pub monster_spawn_block_light_limit: u8,
    pub fixed_time: Option<i64>,
    pub has_fixed_time: Option<bool>,
    pub attributes: Option<Value>,
    pub timelines: Option<String>,
}

impl DimensionType {
    /// Checks the constraints in Minecraft's `DimensionType` codec.
    pub fn validate(&self) -> Result<(), &'static str> {
        const MIN_Y: i32 = -2032;
        const MAX_Y: i32 = 2031;
        const Y_SIZE: i32 = 4064;
        if !(MIN_Y..=MAX_Y).contains(&self.min_y)
            || !(16..=Y_SIZE).contains(&self.height)
            || !(0..=self.height).contains(&self.logical_height)
            || self.min_y + self.height > MAX_Y + 1
            || self.min_y % 16 != 0
            || self.height % 16 != 0
        {
            return Err("invalid dimension height range");
        }
        if !(f64::from(1.0e-5f32)..=3.0e7).contains(&self.coordinate_scale) {
            return Err("invalid dimension coordinate scale");
        }
        if self.monster_spawn_block_light_limit > 15 {
            return Err("invalid monster spawn block light limit");
        }
        Ok(())
    }
}

#[cfg(feature = "runtime")]
impl DimensionType {
    /// Resolves the properties of a level referring to this dimension type.
    pub fn build_level(
        &self,
        id: u8,
        world_key: &str,
    ) -> Result<pumpkin_data::dimension::Dimension, String> {
        use pumpkin_data::dimension::{BedRule, BedRuleOption, Dimension};

        self.validate()?;
        let monster_spawn_light_level =
            serde_json::from_value(self.monster_spawn_light_level.clone())
                .map_err(|error| error.to_string())?;
        let attr = |id: &str| self.attributes.as_ref().and_then(|attrs| attrs.get(id));
        let flag = |id: &str, default| attr(id).and_then(Value::as_bool).unwrap_or(default);
        let number = |id: &str| attr(id).and_then(Value::as_f64).map(|value| value as f32);
        let color = |id: &str| {
            let value = attr(id)?.as_str()?;
            u32::from_str_radix(value.strip_prefix('#')?, 16)
                .ok()
                .map(|value| value as i32)
        };
        let bed = attr("minecraft:gameplay/bed_rule");
        let can_sleep = match bed
            .and_then(|value| value.get("can_sleep"))
            .and_then(Value::as_str)
        {
            Some("always") => BedRuleOption::Always,
            Some("never") => BedRuleOption::Never,
            _ => BedRuleOption::WhenDark,
        };
        let can_set_spawn = match bed
            .and_then(|value| value.get("can_set_spawn"))
            .and_then(Value::as_str)
        {
            Some("when_dark") => BedRuleOption::WhenDark,
            Some("never") => BedRuleOption::Never,
            _ => BedRuleOption::Always,
        };
        let destroy_on_use = bed
            .and_then(|value| value.get("destroy_on_use"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let infiniburn = if self.infiniburn.contains(':') {
            self.infiniburn.clone()
        } else if let Some(tag) = self.infiniburn.strip_prefix('#') {
            format!("#minecraft:{tag}")
        } else {
            format!("minecraft:{}", self.infiniburn)
        };
        Ok(Dimension {
            id,
            minecraft_name: Box::leak(world_key.to_owned().into_boxed_str()),
            fixed_time: self.fixed_time,
            has_fixed_time: self.has_fixed_time.unwrap_or(self.fixed_time.is_some()),
            has_skylight: self.has_skylight,
            has_ceiling: self.has_ceiling,
            coordinate_scale: self.coordinate_scale,
            min_y: self.min_y,
            height: self.height,
            logical_height: self.logical_height,
            infiniburn: Box::leak(infiniburn.into_boxed_str()),
            ambient_light: self.ambient_light,
            monster_spawn_light_level,
            monster_spawn_block_light_limit: self.monster_spawn_block_light_limit,
            sky_color: color("minecraft:visual/sky_color"),
            fog_color: color("minecraft:visual/fog_color"),
            cloud_color: color("minecraft:visual/cloud_color"),
            ambient_light_color: color("minecraft:visual/ambient_light_color"),
            sky_light_color: color("minecraft:visual/sky_light_color"),
            sky_light_factor: number("minecraft:visual/sky_light_factor"),
            fog_start_distance: number("minecraft:visual/fog_start_distance"),
            fog_end_distance: number("minecraft:visual/fog_end_distance"),
            cloud_height: number("minecraft:visual/cloud_height"),
            sky_light_level: number("minecraft:gameplay/sky_light_level"),
            water_evaporates: flag("minecraft:gameplay/water_evaporates", false),
            fast_lava: flag("minecraft:gameplay/fast_lava", false),
            respawn_anchor_works: flag("minecraft:gameplay/respawn_anchor_works", false),
            piglins_zombify: flag("minecraft:gameplay/piglins_zombify", true),
            snow_golem_melts: flag("minecraft:gameplay/snow_golem_melts", false),
            can_start_raid: flag("minecraft:gameplay/can_start_raid", true),
            nether_portal_spawns_piglin: flag(
                "minecraft:gameplay/nether_portal_spawns_piglin",
                false,
            ),
            bed_rule: BedRule {
                can_sleep,
                can_set_spawn,
                destroy_on_use,
            },
            timelines: self.timelines.as_ref().map(|timelines| {
                let id = if timelines.contains(':') {
                    timelines.clone()
                } else if let Some(tag) = timelines.strip_prefix('#') {
                    format!("#minecraft:{tag}")
                } else {
                    format!("minecraft:{timelines}")
                };
                &*Box::leak(id.into_boxed_str())
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_type_checks_vanilla_build_limits_in_block_coordinates() {
        let mut ty = DimensionType {
            has_skylight: true,
            has_ceiling: false,
            ambient_light: 0.0,
            coordinate_scale: 1.0,
            min_y: -2032,
            height: 4064,
            logical_height: 320,
            infiniburn: "#minecraft:infiniburn_overworld".to_owned(),
            monster_spawn_light_level: Value::from(0),
            monster_spawn_block_light_limit: 0,
            has_fixed_time: None,
            fixed_time: None,
            attributes: None,
            timelines: None,
        };
        assert_eq!(ty.validate(), Ok(()));
        ty.min_y += 16;
        assert!(ty.validate().is_err());
        ty.height = 128;
        assert!(ty.validate().is_err());
        ty.logical_height = ty.height;
        assert_eq!(ty.validate(), Ok(()));
        ty.height += 1;
        assert!(ty.validate().is_err());
    }
}
