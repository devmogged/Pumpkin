use std::borrow::Cow;

use serde::Deserialize;
use serde_json::Value;
#[cfg(feature = "codegen")]
use {
    heck::ToShoutySnakeCase,
    proc_macro2::TokenStream,
    quote::{ToTokens, quote},
};

use crate::noise_settings::BlockStateCodecStruct;

fn resource_id(name: &str) -> Cow<'_, str> {
    if name.contains(':') {
        name.into()
    } else {
        format!("minecraft:{name}").into()
    }
}

/// Seeds for the xoroshiro and legacy positional factories.
#[must_use]
pub fn gradient_key(name: &str) -> (u64, u64, i32) {
    let name = resource_id(name);
    let digest = md5::compute(name.as_bytes()).0;
    let [lo, hi] = [0, 8].map(|i| u64::from_be_bytes(std::array::from_fn(|j| digest[i + j])));
    let hash = name.encode_utf16().fold(0i32, |hash, unit| {
        hash.wrapping_mul(31).wrapping_add(i32::from(unit))
    });
    (lo, hi, hash)
}

/// Deserialized Y offset that can be expressed relative to different reference points.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum YOffsetStruct {
    Absolute { absolute: i16 },
    AboveBottom { above_bottom: i8 },
    BelowTop { below_top: i8 },
    RelativeToSeaLevel { relative_to_sea_level: i16 },
}

#[cfg(feature = "codegen")]
impl ToTokens for YOffsetStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Absolute { absolute } => {
                tokens.extend(quote!(YOffset::Absolute(pumpkin_util::y_offset::Absolute { absolute: #absolute })));
            }
            Self::AboveBottom { above_bottom } => {
                tokens.extend(quote!(YOffset::AboveBottom(pumpkin_util::y_offset::AboveBottom { above_bottom: #above_bottom })));
            }
            Self::BelowTop { below_top } => {
                tokens.extend(quote!(YOffset::BelowTop(pumpkin_util::y_offset::BelowTop { below_top: #below_top })));
            }
            Self::RelativeToSeaLevel {
                relative_to_sea_level,
            } => {
                tokens.extend(quote!(YOffset::RelativeToSeaLevel(pumpkin_util::y_offset::RelativeToSeaLevel { relative_to_sea_level: #relative_to_sea_level })));
            }
        }
    }
}

/// Raw deserialized surface material condition from `material_condition` or inline in `material_rule`.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum RawMaterialCondition {
    Ref(String),
    Direct(DirectMaterialCondition),
}

#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
enum DirectMaterialCondition {
    #[serde(rename = "minecraft:biome", alias = "biome")]
    Biome {
        #[serde(deserialize_with = "deserialize_string_or_vec")]
        biome_is: Vec<String>,
    },
    #[serde(rename = "minecraft:noise_threshold", alias = "noise_threshold")]
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
        #[serde(default)]
        is_3d: bool,
    },
    #[serde(rename = "minecraft:vertical_gradient", alias = "vertical_gradient")]
    VerticalGradient {
        random_name: String,
        true_at_and_below: YOffsetStruct,
        false_at_and_above: YOffsetStruct,
    },
    #[serde(rename = "minecraft:y_above", alias = "y_above")]
    YAbove {
        anchor: YOffsetStruct,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    #[serde(rename = "minecraft:water", alias = "water")]
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    #[serde(rename = "minecraft:temperature", alias = "temperature")]
    Temperature,
    #[serde(rename = "minecraft:steep", alias = "steep")]
    Steep,
    #[serde(rename = "minecraft:not", alias = "not")]
    Not { invert: Box<RawMaterialCondition> },
    #[serde(rename = "minecraft:hole", alias = "hole")]
    Hole,
    #[serde(
        rename = "minecraft:above_preliminary_surface",
        alias = "above_preliminary_surface"
    )]
    AbovePreliminarySurface,
    #[serde(rename = "minecraft:stone_depth", alias = "stone_depth")]
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: Surface,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    Ceiling,
    Floor,
}

fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct StringOrVec;

    impl<'de> serde::de::Visitor<'de> for StringOrVec {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or list of strings")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(vec![value.to_owned()])
        }

        fn visit_seq<S>(self, mut seq: S) -> Result<Self::Value, S::Error>
        where
            S: serde::de::SeqAccess<'de>,
        {
            let mut vec = Vec::new();
            while let Some(el) = seq.next_element()? {
                vec.push(el);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_any(StringOrVec)
}

/// Raw deserialized material rule from `material_rule` folder or inline.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
enum RawMaterialRule {
    Ref(String),
    Direct(DirectMaterialRule),
}

#[derive(Deserialize, Clone)]
#[serde(tag = "type")]
enum DirectMaterialRule {
    #[serde(rename = "minecraft:block", alias = "block")]
    Block { result_state: BlockStateCodecStruct },
    #[serde(rename = "minecraft:sequence", alias = "sequence")]
    Sequence { sequence: Vec<RawMaterialRule> },
    #[serde(rename = "minecraft:condition", alias = "condition")]
    Condition {
        if_true: RawMaterialCondition,
        then_run: Box<RawMaterialRule>,
    },
    #[serde(
        rename = "minecraft:bandlands",
        alias = "minecraft:badlands",
        alias = "bandlands"
    )]
    Badlands,
    #[serde(rename = "minecraft:ore_vein", alias = "ore_vein")]
    OreVein(Box<RawOreVein>),
}

#[derive(Deserialize, Clone)]
struct RawOreVein {
    ore_block: BlockStateCodecStruct,
    raw_ore_block: BlockStateCodecStruct,
    filler_block: BlockStateCodecStruct,
    raw_ore_chance: f32,
    density: Value,
    richness: Value,
    filler_gap: Value,
}

/// Deserialized surface material condition that gates a material rule.
pub enum MaterialConditionStruct {
    Biome {
        biome_is: Vec<String>,
    },
    NoiseThreshold {
        noise: String,
        min_threshold: f64,
        max_threshold: f64,
        is_3d: bool,
    },
    VerticalGradient {
        random_name: String,
        true_at_and_below: YOffsetStruct,
        false_at_and_above: YOffsetStruct,
    },
    YAbove {
        anchor: YOffsetStruct,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Water {
        offset: i32,
        surface_depth_multiplier: i32,
        add_stone_depth: bool,
    },
    Temperature,
    Steep,
    Not {
        invert: Box<Self>,
    },
    Hole,
    AbovePreliminarySurface,
    StoneDepth {
        offset: i32,
        add_surface_depth: bool,
        secondary_depth_range: i32,
        surface_type: Surface,
    },
}

#[cfg(feature = "codegen")]
impl ToTokens for MaterialConditionStruct {
    #[expect(clippy::too_many_lines, reason = "one emission arm per condition")]
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Biome { biome_is } => {
                let biomes = biome_is
                    .iter()
                    .map(|b| b.strip_prefix("minecraft:").unwrap_or(b).to_uppercase());
                let biome_refs: Vec<TokenStream> = biomes
                    .map(|b| {
                        let ident = quote::format_ident!("{}", b);
                        quote!(&crate::biome::Biome::#ident)
                    })
                    .collect();

                tokens.extend(quote!(
                    MaterialCondition::Biome(BiomeMaterialCondition {
                        biome_is: &[#(#biome_refs),*],
                    })
                ));
            }
            Self::NoiseThreshold {
                noise,
                min_threshold,
                max_threshold,
                is_3d,
            } => {
                let noise_id = quote::format_ident!(
                    "{}",
                    noise
                        .strip_prefix("minecraft:")
                        .unwrap_or(noise)
                        .to_shouty_snake_case()
                );

                let condition = if *is_3d {
                    quote!(MaterialCondition::NoiseThreshold3d)
                } else {
                    quote!(MaterialCondition::NoiseThreshold)
                };
                tokens.extend(quote!(
                    #condition(NoiseThresholdMaterialCondition {
                        noise: DoublePerlinNoiseParameters::#noise_id,
                        min_threshold: #min_threshold,
                        max_threshold: #max_threshold,
                    })
                ));
            }
            Self::VerticalGradient {
                random_name,
                true_at_and_below,
                false_at_and_above,
            } => {
                let (lo, hi, hash) = gradient_key(random_name);
                tokens.extend(quote!(
                    MaterialCondition::VerticalGradient(VerticalGradientMaterialCondition {
                        random_lo: #lo,
                        random_hi: #hi,
                        random_hash: #hash,
                        true_at_and_below: #true_at_and_below,
                        false_at_and_above: #false_at_and_above,
                    })
                ));
            }
            Self::YAbove {
                anchor,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                tokens.extend(quote!(
                    MaterialCondition::YAbove(AboveYMaterialCondition {
                        anchor: #anchor,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                ));
            }
            Self::Water {
                offset,
                surface_depth_multiplier,
                add_stone_depth,
            } => {
                tokens.extend(quote!(
                    MaterialCondition::Water(WaterMaterialCondition {
                        offset: #offset,
                        surface_depth_multiplier: #surface_depth_multiplier,
                        add_stone_depth: #add_stone_depth,
                    })
                ));
            }
            Self::Temperature => {
                tokens.extend(quote!(MaterialCondition::Temperature));
            }
            Self::Steep => {
                tokens.extend(quote!(MaterialCondition::Steep));
            }
            Self::Not { invert } => {
                tokens.extend(quote!(
                    MaterialCondition::Not(NotMaterialCondition {
                        invert: &#invert,
                    })
                ));
            }
            Self::Hole => {
                tokens.extend(quote!(MaterialCondition::Hole(HoleMaterialCondition)));
            }
            Self::AbovePreliminarySurface => {
                tokens.extend(quote!(MaterialCondition::AbovePreliminarySurface(
                    SurfaceMaterialCondition
                )));
            }
            Self::StoneDepth {
                offset,
                add_surface_depth,
                secondary_depth_range,
                surface_type,
            } => {
                let surface_type_token = match surface_type {
                    Surface::Ceiling => quote!(
                        pumpkin_util::math::vertical_surface_type::VerticalSurfaceType::Ceiling
                    ),
                    Surface::Floor => quote!(
                        pumpkin_util::math::vertical_surface_type::VerticalSurfaceType::Floor
                    ),
                };

                tokens.extend(quote!(
                    MaterialCondition::StoneDepth(StoneDepthMaterialCondition {
                        offset: #offset,
                        add_surface_depth: #add_surface_depth,
                        secondary_depth_range: #secondary_depth_range,
                        surface_type: #surface_type_token,
                    })
                ));
            }
        }
    }
}

/// Deserialized surface material rule that determines which block to place at a given surface point.
pub enum MaterialRuleStruct {
    Block {
        result_state: BlockStateCodecStruct,
    },
    Sequence {
        sequence: Vec<Self>,
    },
    Condition {
        if_true: MaterialConditionStruct,
        then_run: Box<Self>,
    },
    Badlands,
    OreVein {
        ore_block: BlockStateCodecStruct,
        raw_ore_block: BlockStateCodecStruct,
        filler_block: BlockStateCodecStruct,
        raw_ore_chance: f32,
        min_y: i32,
        max_y: i32,
        is_iron: bool,
    },
}

#[cfg(feature = "codegen")]
impl ToTokens for MaterialRuleStruct {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Block { result_state } => {
                tokens.extend(quote!(
                    MaterialRule::Block(BlockMaterialRule {
                        result_state: #result_state
                    })
                ));
            }
            Self::Sequence { sequence } => {
                tokens.extend(quote!(
                    MaterialRule::Sequence(SequenceMaterialRule {
                        sequence: &[#(#sequence),*]
                    })
                ));
            }
            Self::Condition { if_true, then_run } => {
                tokens.extend(quote!(
                    MaterialRule::Condition(ConditionMaterialRule {
                        if_true: #if_true,
                        then_run: &#then_run
                    })
                ));
            }
            Self::Badlands => {
                tokens.extend(quote!(MaterialRule::Badlands(BadLandsMaterialRule)));
            }
            Self::OreVein {
                ore_block,
                raw_ore_block,
                filler_block,
                raw_ore_chance,
                min_y,
                max_y,
                is_iron,
            } => {
                tokens.extend(quote!(
                    MaterialRule::OreVein(OreVeinMaterialRule {
                        ore_block: #ore_block,
                        raw_ore_block: #raw_ore_block,
                        filler_block: #filler_block,
                        raw_ore_chance: #raw_ore_chance,
                        min_y: #min_y,
                        max_y: #max_y,
                        is_iron: #is_iron,
                    })
                ));
            }
        }
    }
}

/// Resolves inline rules and references in the caller's merged registries.
pub fn resolve_rule(
    root: Value,
    lookup: impl Fn(MaterialRegistry, &str) -> Result<Value, String>,
) -> Result<MaterialRuleStruct, String> {
    let rule = serde_json::from_value(root).map_err(|error| error.to_string())?;
    Resolver {
        lookup,
        remaining: 65_536,
    }
    .rule(&rule, 0)
}

#[derive(Clone, Copy)]
pub enum MaterialRegistry {
    Rule,
    Condition,
}

impl MaterialRegistry {
    #[must_use]
    pub const fn directory(self) -> &'static str {
        match self {
            Self::Rule => "material_rule",
            Self::Condition => "material_condition",
        }
    }
}

struct Resolver<F> {
    lookup: F,
    remaining: usize,
}

impl<F: Fn(MaterialRegistry, &str) -> Result<Value, String>> Resolver<F> {
    fn visit(&mut self, depth: usize) -> Result<(), String> {
        if depth >= 128 || self.remaining == 0 {
            return Err("cyclic or oversized material rules".into());
        }
        self.remaining -= 1;
        Ok(())
    }

    fn reference<T: serde::de::DeserializeOwned>(
        &self,
        registry: MaterialRegistry,
        name: &str,
    ) -> Result<T, String> {
        let id = resource_id(name);
        let raw = (self.lookup)(registry, &id)?;
        serde_json::from_value(raw).map_err(|error| format!("{id}: {error}"))
    }

    fn condition(
        &mut self,
        raw: &RawMaterialCondition,
        depth: usize,
    ) -> Result<MaterialConditionStruct, String> {
        self.visit(depth)?;
        match raw {
            RawMaterialCondition::Ref(name) => {
                let resolved = self.reference(MaterialRegistry::Condition, name)?;
                self.condition(&resolved, depth + 1)
            }
            RawMaterialCondition::Direct(direct) => Ok(match direct {
                DirectMaterialCondition::Biome { biome_is } => MaterialConditionStruct::Biome {
                    biome_is: biome_is.clone(),
                },
                DirectMaterialCondition::NoiseThreshold {
                    noise,
                    min_threshold,
                    max_threshold,
                    is_3d,
                } => MaterialConditionStruct::NoiseThreshold {
                    noise: noise.clone(),
                    min_threshold: *min_threshold,
                    max_threshold: *max_threshold,
                    is_3d: *is_3d,
                },
                DirectMaterialCondition::VerticalGradient {
                    random_name,
                    true_at_and_below,
                    false_at_and_above,
                } => MaterialConditionStruct::VerticalGradient {
                    random_name: random_name.clone(),
                    true_at_and_below: true_at_and_below.clone(),
                    false_at_and_above: false_at_and_above.clone(),
                },
                DirectMaterialCondition::YAbove {
                    anchor,
                    surface_depth_multiplier,
                    add_stone_depth,
                } => MaterialConditionStruct::YAbove {
                    anchor: anchor.clone(),
                    surface_depth_multiplier: *surface_depth_multiplier,
                    add_stone_depth: *add_stone_depth,
                },
                DirectMaterialCondition::Water {
                    offset,
                    surface_depth_multiplier,
                    add_stone_depth,
                } => MaterialConditionStruct::Water {
                    offset: *offset,
                    surface_depth_multiplier: *surface_depth_multiplier,
                    add_stone_depth: *add_stone_depth,
                },
                DirectMaterialCondition::Temperature => MaterialConditionStruct::Temperature,
                DirectMaterialCondition::Steep => MaterialConditionStruct::Steep,
                DirectMaterialCondition::Not { invert } => MaterialConditionStruct::Not {
                    invert: Box::new(self.condition(invert, depth + 1)?),
                },
                DirectMaterialCondition::Hole => MaterialConditionStruct::Hole,
                DirectMaterialCondition::AbovePreliminarySurface => {
                    MaterialConditionStruct::AbovePreliminarySurface
                }
                DirectMaterialCondition::StoneDepth {
                    offset,
                    add_surface_depth,
                    secondary_depth_range,
                    surface_type,
                } => MaterialConditionStruct::StoneDepth {
                    offset: *offset,
                    add_surface_depth: *add_surface_depth,
                    secondary_depth_range: *secondary_depth_range,
                    surface_type: *surface_type,
                },
            }),
        }
    }

    fn rule(&mut self, raw: &RawMaterialRule, depth: usize) -> Result<MaterialRuleStruct, String> {
        self.visit(depth)?;
        match raw {
            RawMaterialRule::Ref(name) => {
                let resolved = self.reference(MaterialRegistry::Rule, name)?;
                self.rule(&resolved, depth + 1)
            }
            RawMaterialRule::Direct(direct) => Ok(match direct {
                DirectMaterialRule::Block { result_state } => MaterialRuleStruct::Block {
                    result_state: result_state.clone(),
                },
                DirectMaterialRule::Sequence { sequence } => MaterialRuleStruct::Sequence {
                    sequence: sequence
                        .iter()
                        .map(|rule| self.rule(rule, depth + 1))
                        .collect::<Result<_, _>>()?,
                },
                DirectMaterialRule::Condition { if_true, then_run } => {
                    MaterialRuleStruct::Condition {
                        if_true: self.condition(if_true, depth + 1)?,
                        then_run: Box::new(self.rule(then_run, depth + 1)?),
                    }
                }
                DirectMaterialRule::Badlands => MaterialRuleStruct::Badlands,
                DirectMaterialRule::OreVein(vein) => {
                    let RawOreVein {
                        ore_block,
                        raw_ore_block,
                        filler_block,
                        raw_ore_chance,
                        density,
                        richness,
                        filler_gap,
                    } = vein.as_ref();
                    let is_iron = match density.as_str() {
                        Some("minecraft:overworld/ore_vein/iron_density") => true,
                        Some("minecraft:overworld/ore_vein/copper_density") => false,
                        _ => return Err(format!("unsupported ore vein density {density}")),
                    };
                    if richness != "minecraft:overworld/ore_vein/richness"
                        || filler_gap != "minecraft:overworld/ore_vein/gap"
                    {
                        return Err("unsupported ore vein richness or gap function".into());
                    }
                    let (min_y, max_y) = if is_iron { (-60, -8) } else { (0, 50) };
                    MaterialRuleStruct::OreVein {
                        ore_block: ore_block.clone(),
                        raw_ore_block: raw_ore_block.clone(),
                        filler_block: filler_block.clone(),
                        raw_ore_chance: *raw_ore_chance,
                        min_y,
                        max_y,
                        is_iron,
                    }
                }
            }),
        }
    }
}
