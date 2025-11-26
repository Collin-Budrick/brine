use bevy::{
    asset::AssetPlugin,
    prelude::{PluginGroup, *},
    DefaultPlugins,
};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use brine_asset::MinecraftAssets;
use brine_data::MinecraftData;
use brine_render::texture::{
    MinecraftTexturesPlugin, MinecraftTexturesState, TextureAtlas, TextureManager,
    TextureManagerPlugin,
};

fn main() {
    let mc_data = MinecraftData::for_version("1.21.4");

    println!("Loading asset metadata");
    let mc_assets = MinecraftAssets::new("assets/1.21.4", &mc_data).unwrap();

    App::new()
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            file_path: "../../assets".into(),
            ..default()
        }))
        .add_plugins(WorldInspectorPlugin::new())
        .insert_resource(mc_assets)
        .add_plugins((TextureManagerPlugin, MinecraftTexturesPlugin))
        .add_systems(Startup, setup)
        .add_systems(OnEnter(MinecraftTexturesState::Loaded), spawn_sprite)
        .run();
}

fn setup(mut commands: Commands) {
    commands.spawn((Camera2d, Msaa::Sample4, Transform::default(), GlobalTransform::default()));
}

fn spawn_sprite(
    texture_manager: Res<TextureManager>,
    atlases: Res<Assets<TextureAtlas>>,
    mut commands: Commands,
) {
    println!("Atlas stitched. Spawning sprite.");

    let atlas_handle = texture_manager.atlases().next().unwrap();

    let atlas = atlases.get(atlas_handle).unwrap();

    commands.spawn((
        Sprite::from_image(atlas.texture.clone()),
        Transform::from_scale(Vec3::ONE * 0.5),
        GlobalTransform::default(),
    ));
}
