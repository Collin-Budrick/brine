use bevy::prelude::*;

use brine_asset::{MinecraftAssets, TextureKey};

use crate::texture::{TextureAtlas, TextureManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States, Default)]
pub enum MinecraftTexturesState {
    #[default]
    Loading,
    Loaded,
}

pub struct MinecraftTexturesPlugin;

impl Plugin for MinecraftTexturesPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<MinecraftTexturesState>();
        app.init_resource::<TheAtlas>();
        app.add_systems(OnEnter(MinecraftTexturesState::Loading), setup);
        app.add_systems(
            Update,
            await_loaded.run_if(in_state(MinecraftTexturesState::Loading)),
        );
    }
}

#[derive(Resource, Default)]
struct TheAtlas {
    handle: Handle<TextureAtlas>,
}

fn get_all_textures<'a>(
    mc_assets: &'a MinecraftAssets,
    asset_server: &'a AssetServer,
) -> impl Iterator<Item = (TextureKey, Handle<Image>)> + 'a {
    mc_assets
        .textures()
        .iter()
        .filter_map(|(texture_key, texture_id)| {
            trace!("{texture_key:?}: {texture_id:?}");

            if texture_id.path().starts_with("block/")
                // || texture_id.path().starts_with("effect/")
                // || texture_id.path().starts_with("item/")
                // || texture_id.path().starts_with("mob_effect/")
                || texture_id.path().starts_with("painting/")
            // || texture_id.path().starts_with("particle/")
            {
                let path = mc_assets.get_texture_path(texture_key).unwrap();
                let handle = asset_server.load(path);
                Some((texture_key, handle))
            } else {
                None
            }
        })
}

/// This system kicks off the creation of the texture atlas(es).
fn setup(
    mc_assets: Res<MinecraftAssets>,
    asset_server: Res<AssetServer>,
    atlases: Res<Assets<TextureAtlas>>,
    mut the_atlas: ResMut<TheAtlas>,
    mut texture_manager: ResMut<TextureManager>,
) {
    let textures = get_all_textures(&*mc_assets, &*asset_server);

    let atlas_handle = texture_manager.create_atlas(&*atlases, textures);
    the_atlas.handle = atlas_handle;
}

/// This system advances the state to `Loaded` once the texture atlas(es) is/are available.
fn await_loaded(
    the_atlas: Res<TheAtlas>,
    atlases: Res<Assets<TextureAtlas>>,
    mut next_state: ResMut<NextState<MinecraftTexturesState>>,
) {
    if atlases.contains(&the_atlas.handle) {
        next_state.set(MinecraftTexturesState::Loaded);
    }
}
