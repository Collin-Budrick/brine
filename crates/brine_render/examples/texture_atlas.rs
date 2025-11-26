use bevy::{asset::AssetPath, prelude::*, DefaultPlugins};
use brine_asset::TextureKey;
use brine_render::texture::{TextureAtlas, TextureManager, TextureManagerPlugin};
use minecraft_assets::api::{ResourceIdentifier, ResourcePath};

fn get_a_few_textures(
    asset_server: &AssetServer,
) -> impl Iterator<Item = (TextureKey, Handle<Image>)> + '_ {
    const TEXTURES: &[&str] = &[
        "block/water_still.png",
        "block/campfire_fire.png",
        "block/stone.png",
    ];

    TEXTURES.iter().enumerate().map(|(index, name)| {
        let key = TextureKey(index);
        let loc = ResourceIdentifier::texture(name);
        let path = ResourcePath::for_resource("1.21.4", &loc).into_inner();
        let handle = asset_server.load(AssetPath::from(path));
        (key, handle)
    })
}

// fn get_all_textures(
//     asset_server: &AssetServer,
// ) -> impl Iterator<Item = (TextureKey, Handle<Image>)> + '_ {
//     let resource_provider = FileSystemResourceProvider::new("assets/1.21.4");

//     resource_provider
//         .enumerate_resources("minecraft", ResourceKind::Texture)
//         .unwrap()
//         .into_iter()
//         .enumerate()
//         .filter_map(|(index, resource_location)| {
//             println!("{index}: {resource_location:?}");
//             if resource_location.path().starts_with("block/")
//                 || resource_location.path().starts_with("effect/")
//                 || resource_location.path().starts_with("item/")
//                 || resource_location.path().starts_with("mob_effect/")
//                 || resource_location.path().starts_with("painting/")
//                 || resource_location.path().starts_with("particle/")
//             {
//                 let key = TextureKey(index);
//                 let path = ResourcePath::for_resource("1.21.4", &resource_location);
//                 let handle = asset_server.load(path.as_ref());
//                 Some((key, handle))
//             } else {
//                 None
//             }
//         })
// }

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .init_resource::<TheAtlas>()
        .add_plugins(TextureManagerPlugin)
        .insert_state(AtlasState::default())
        .add_systems(Update, setup.run_if(in_state(AtlasState::Idle)))
        .add_systems(
            Update,
            spawn_sprite.run_if(in_state(AtlasState::LoadingTextures)),
        )
        .run();
}

#[derive(States, Default, Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum AtlasState {
    #[default]
    Idle,
    LoadingTextures,
    Stitched,
}

#[derive(Default, Resource)]
struct TheAtlas {
    handle: Handle<TextureAtlas>,
}

fn setup(
    asset_server: Res<AssetServer>,
    mut texture_manager: ResMut<TextureManager>,
    mut the_atlas: ResMut<TheAtlas>,
    atlases: Res<Assets<TextureAtlas>>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AtlasState>>,
) {
    commands.spawn((Camera2d, Msaa::Sample4, Transform::default(), GlobalTransform::default()));

    let texture_keys_and_handles = get_a_few_textures(&*asset_server);

    let atlas_handle = texture_manager.create_atlas(&*atlases, texture_keys_and_handles);

    the_atlas.handle = atlas_handle;

    next_state.set(AtlasState::LoadingTextures);
}

fn spawn_sprite(
    atlases: Res<Assets<TextureAtlas>>,
    the_atlas: Res<TheAtlas>,
    mut commands: Commands,
    mut next_state: ResMut<NextState<AtlasState>>,
) {
    if let Some(atlas) = atlases.get(&the_atlas.handle) {
        println!("Atlas stitched. Spawning sprite.");

        commands.spawn((
            Sprite::from_image(atlas.texture.clone()),
            GlobalTransform::default(),
            Transform::default(),
        ));

        next_state.set(AtlasState::Stitched);
    }
}
