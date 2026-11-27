use pumpkin_protocol::codec::recipe::DynamicRecipe;

pub trait RecipeProvider: Send + Sync {
    fn get_dynamic_recipes(&self) -> Vec<DynamicRecipe>;

    /// A nonzero, monotonically advancing revision for the complete dynamic
    /// crafting catalog. Menus can check it before dispensing a cached result
    /// after `/reload` without cloning all recipes on every inventory sync.
    /// Mutable providers without a reliable revision should return `None`
    /// (the default), so their result slots revalidate on every read.
    fn recipe_revision(&self) -> Option<u64> {
        None
    }
}

#[derive(Clone, Copy)]
pub enum GenericRecipe<'a> {
    Vanilla(&'a pumpkin_data::recipes::CraftingRecipeTypes),
    Dynamic(&'a pumpkin_protocol::codec::recipe::OwnedCraftingRecipe),
}

#[derive(Clone, Copy)]
pub enum IngredientRef<'a> {
    Vanilla(&'a pumpkin_data::recipes::RecipeIngredientTypes),
    Dynamic(&'a pumpkin_protocol::codec::recipe::OwnedRecipeIngredient),
}

impl IngredientRef<'_> {
    #[must_use]
    pub fn match_item(&self, item: &pumpkin_data::item::Item) -> bool {
        match self {
            Self::Vanilla(v) => v.match_item(item),
            Self::Dynamic(d) => d.match_item(item),
        }
    }
}
