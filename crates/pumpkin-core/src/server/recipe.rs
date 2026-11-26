use std::{
    borrow::Cow,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};

use pumpkin_data::{
    item::Item,
    recipes::{CookingRecipeKind, get_cooking_recipe_with_ingredient, get_recipe_experience},
};

use pumpkin_inventory::crafting::recipe_provider::RecipeProvider;
pub use pumpkin_protocol::codec::recipe::DynamicRecipe;
use pumpkin_protocol::codec::recipe::{OwnedCookingRecipe, OwnedCookingRecipeType};

use crate::world::World;

pub struct CookingRecipe {
    pub id: Cow<'static, str>,
    pub result: &'static Item,
    pub count: u8,
    pub ticks: i32,
}

fn cooking_definition(recipe: &DynamicRecipe) -> Option<(CookingRecipeKind, &OwnedCookingRecipe)> {
    let DynamicRecipe::Cooking(recipe) = recipe else {
        return None;
    };
    Some(match recipe {
        OwnedCookingRecipeType::Blasting(recipe) => (CookingRecipeKind::Blasting, recipe),
        OwnedCookingRecipeType::Smelting(recipe) => (CookingRecipeKind::Smelting, recipe),
        OwnedCookingRecipeType::Smoking(recipe) => (CookingRecipeKind::Smoking, recipe),
        OwnedCookingRecipeType::CampfireCooking(recipe) => {
            (CookingRecipeKind::CampfireCooking, recipe)
        }
    })
}

pub fn cooking_recipe(
    world: &World,
    item: &Item,
    kind: CookingRecipeKind,
) -> Option<CookingRecipe> {
    if *item == Item::AIR {
        return None;
    }
    if let Some(dynamic) = world
        .server
        .upgrade()
        .and_then(|server| server.recipe_manager.cooking_for(item, &kind))
    {
        return Some(dynamic);
    }
    let recipe = get_cooking_recipe_with_ingredient(item, kind)?;
    Some(CookingRecipe {
        id: Cow::Borrowed(recipe.recipe_id),
        result: Item::from_registry_key(recipe.result.id)?,
        count: recipe.result.count,
        ticks: recipe.cookingtime,
    })
}

pub struct RecipeManager {
    dynamic_recipes: RwLock<Vec<DynamicRecipe>>,
    revision: AtomicU64,
}

impl Default for RecipeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RecipeManager {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            dynamic_recipes: RwLock::new(Vec::new()),
            revision: AtomicU64::new(1),
        }
    }

    fn cooking_for(&self, item: &Item, kind: &CookingRecipeKind) -> Option<CookingRecipe> {
        self.dynamic_recipes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(cooking_definition)
            .find_map(|(candidate_kind, recipe)| {
                if std::mem::discriminant(kind) != std::mem::discriminant(&candidate_kind)
                    || !recipe.ingredient.match_item(item)
                    || !(1..=i32::from(u16::MAX)).contains(&recipe.cooking_time)
                    || recipe.result.count == 0
                {
                    return None;
                }
                let result = Item::from_registry_key(&recipe.result.item_id)?;
                Some(CookingRecipe {
                    id: Cow::Owned(recipe.recipe_id.clone()),
                    result,
                    count: recipe.result.count,
                    ticks: recipe.cooking_time,
                })
            })
    }

    #[must_use]
    pub fn cooking_experience(&self, id: &str) -> Option<f32> {
        self.dynamic_recipes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(cooking_definition)
            .find(|(_, recipe)| recipe.recipe_id == id)
            .map(|(_, recipe)| recipe.experience)
            .or_else(|| get_recipe_experience(id))
    }

    pub fn add_recipe(&self, recipe: DynamicRecipe) {
        let mut recipes = self
            .dynamic_recipes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recipes.push(recipe);
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn add_recipes(&self, new_recipes: impl IntoIterator<Item = DynamicRecipe>) {
        let mut recipes = self
            .dynamic_recipes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recipes.extend(new_recipes);
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn set_recipes(&self, new_recipes: Vec<DynamicRecipe>) {
        let mut recipes = self
            .dynamic_recipes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *recipes = new_recipes;
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn clear(&self) {
        let mut recipes = self
            .dynamic_recipes
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        recipes.clear();
        self.revision.fetch_add(1, Ordering::Release);
    }

    pub fn get_dynamic_recipes_internal(&self) -> Vec<DynamicRecipe> {
        self.dynamic_recipes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl RecipeProvider for RecipeManager {
    fn get_dynamic_recipes(&self) -> Vec<DynamicRecipe> {
        self.dynamic_recipes
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn recipe_revision(&self) -> Option<u64> {
        Some(self.revision.load(Ordering::Acquire))
    }
}
