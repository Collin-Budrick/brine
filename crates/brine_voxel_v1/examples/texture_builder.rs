use bevy::{prelude::*, DefaultPlugins};
use bevy_image::TextureAtlasLayout;

use brine_asset::{BlockFace, MinecraftAssets};

use brine_data::{blocks::BlockStateId, MinecraftData};
use brine_voxel_v1::texture::{BlockTextures, TextureBuilderPlugin};

fn main() {
    let mc_data = MinecraftData::for_version("1.21.4");
    let mc_assets = MinecraftAssets::new("assets/1.21.4", &mc_data).unwrap();

    App::new()
        .add_plugins(DefaultPlugins)
        .insert_resource(mc_data)
        .insert_resource(mc_assets)
        .add_plugins(TextureBuilderPlugin)
        .insert_state(AppState::default())
        .init_resource::<Atlas>()
        .add_systems(Startup, load_atlas)
        .add_systems(OnEnter(AppState::Loading), load_atlas)
        .add_systems(Update, check_atlas.run_if(in_state(AppState::Loading)))
        .add_systems(OnEnter(AppState::Finished), setup)
        .run();
}

#[derive(States, Default, Debug, Clone, PartialEq, Eq, Hash)]
enum AppState {
    #[default]
    Loading,
    Finished,
}

#[derive(Default, Resource)]
struct Atlas {
    texture: Option<Handle<Image>>,
    #[allow(dead_code)]
    layout: Option<Handle<TextureAtlasLayout>>,
}

fn load_atlas(
    mc_assets: Res<MinecraftAssets>,
    asset_server: Res<AssetServer>,
    mut block_textures: ResMut<BlockTextures>,
    mut atlas: ResMut<Atlas>,
    mut atlas_images: ResMut<Assets<Image>>,
    mut atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
) {
    let block_states = (1..500).map(BlockStateId);

    let (atlas_texture, atlas_layout) =
        block_textures.create_texture_atlas(
            block_states,
            &asset_server,
            |b| mc_assets.get_texture_path_for_block_state_and_face(b, BlockFace::South),
            &mut atlas_images,
            &mut atlas_layouts,
        );

    atlas.texture = Some(atlas_texture);
    atlas.layout = Some(atlas_layout);
}

fn check_atlas(
    atlas: Res<Atlas>,
    textures: Res<Assets<Image>>,
    mut next_state: ResMut<NextState<AppState>>,
) {
    if atlas
        .texture
        .as_ref()
        .is_some_and(|handle| textures.contains(handle))
    {
        next_state.set(AppState::Finished);
    }
}

fn setup(atlas: Res<Atlas>, mut commands: Commands) {
    let texture_atlas_texture = atlas.texture.clone().unwrap();

    commands.spawn((Camera2d, Msaa::Sample4, Transform::default(), GlobalTransform::default()));

    commands.spawn((
        Sprite::from_image(texture_atlas_texture),
        Transform::from_xyz(0.0, 0.0, 0.0).with_scale(Vec3::ONE * 2.0),
        GlobalTransform::default(),
    ));
}
