use bevy::{
    pbr::{
        wireframe::{WireframeConfig, WireframePlugin},
        MeshMaterial3d,
    },
    prelude::{
        default, App, Assets, Camera3d, Color, Commands, GlobalTransform, Mesh, Msaa, PluginGroup,
        Res, ResMut, StandardMaterial, Startup, Transform, Vec3,
    },
    render::{
        render_resource::WgpuFeatures,
        settings::{RenderCreation, WgpuSettings},
        RenderPlugin,
    },
    DefaultPlugins,
};
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use bevy_mesh::Mesh3d;

use brine_asset::MinecraftAssets;
use brine_chunk::{BlockState, BlockStates, ChunkSection, BLOCKS_PER_SECTION};
use brine_data::MinecraftData;
use brine_render::chunk::ChunkBakery;

fn main() {
    let mc_data = MinecraftData::for_version("1.21.4");
    let mc_assets = MinecraftAssets::new("assets/1.21.4", &mc_data).unwrap();

    App::new()
        .add_plugins(DefaultPlugins.set(RenderPlugin {
            render_creation: RenderCreation::Automatic(WgpuSettings {
                features: WgpuFeatures::POLYGON_MODE_LINE,
                ..default()
            }),
            ..default()
        }))
        .insert_resource(WireframeConfig {
            global: true,
            default_color: Color::WHITE,
        })
        .add_plugins(WireframePlugin::default())
        .add_plugins(WorldInspectorPlugin::new())
        .insert_resource(mc_data)
        .insert_resource(mc_assets)
        .add_systems(Startup, setup)
        .run();
}

fn random_block_state() -> BlockState {
    let id = fastrand::u32(1..10000);
    BlockState(id)
}

fn random_chunk() -> ChunkSection {
    let mut block_states = [BlockState::AIR; BLOCKS_PER_SECTION];

    let mut block_count = 0;
    for block_state in block_states.iter_mut() {
        if fastrand::f32() >= 0.9 {
            *block_state = random_block_state();
            block_count += 1;
        }
    }

    ChunkSection {
        block_count,
        chunk_y: 0,
        block_states: BlockStates(block_states),
    }
}

fn bake_chunk(chunk: &ChunkSection, mc_data: &MinecraftData, mc_assets: &MinecraftAssets) -> Mesh {
    let chunk_bakery = ChunkBakery::new(mc_data, mc_assets);

    let baked_chunk = chunk_bakery.bake_chunk(chunk);

    baked_chunk.mesh
}

fn setup(
    mc_data: Res<MinecraftData>,
    mc_assets: Res<MinecraftAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let chunk = random_chunk();

    let mesh = bake_chunk(&chunk, &*mc_data, &*mc_assets);

    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            ..default()
        })),
        Transform::default(),
        GlobalTransform::default(),
    ));

    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_translation(Vec3::new(30.0, 24.0, 30.0))
            .looking_at(Vec3::ONE * 8.0, Vec3::Y),
        GlobalTransform::default(),
    ));
}
