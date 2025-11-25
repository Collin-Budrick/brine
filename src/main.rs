//! The Brine Minecraft client entrypoint.

use std::path::PathBuf;

use bevy::{
    diagnostic::{FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin},
    log::{Level, LogPlugin},
    prelude::*,
    render::{
        render_resource::WgpuFeatures,
        settings::{RenderCreation, WgpuSettings},
        RenderPlugin,
    },
};
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin};
use bevy_inspector_egui::quick::WorldInspectorPlugin;
use brine_asset::MinecraftAssets;
use brine_data::MinecraftData;
use clap::Parser;

use brine_proto::{AlwaysSuccessfulLoginPlugin, ProtocolPlugin};
use brine_proto_backend::ProtocolBackendPlugin;
use brine_voxel_v1::{
    chunk_builder::{
        component::BuiltChunkSection, ChunkBuilderPlugin, VisibleFacesChunkBuilder,
    },
    texture::TextureBuilderPlugin,
};

use brine::{
    debug::DebugWireframePlugin, login::LoginPlugin, server::ServeChunksFromDirectoryPlugin,
    DEFAULT_LOG_FILTER,
};

const SERVER: &str = "localhost:25565";
const USERNAME: &str = "user";

/// Brine Minecraft Client
#[derive(Parser)]
struct Args {
    /// Run with additional debug utilities (e.g., egui inspector).
    #[clap(short, long)]
    debug: bool,

    /// Run with a fake server that serves chunks from a directory of chunk files.
    #[clap(name = "chunks", long, value_name = "CHUNK_DIR")]
    chunk_dir: Option<PathBuf>,

    /// Address of the server to connect to (host:port). Defaults to localhost:25565.
    #[clap(long, value_name = "HOST:PORT")]
    server: Option<String>,
}

fn main() {
    let args = Args::parse();

    let mut app = App::new();

    // Default plugins.
    let mut default_plugins = DefaultPlugins.set(LogPlugin {
        level: Level::DEBUG,
        filter: String::from(DEFAULT_LOG_FILTER),
        ..default()
    });

    if args.debug {
        default_plugins = default_plugins.set(RenderPlugin {
            render_creation: RenderCreation::Automatic(WgpuSettings {
                features: WgpuFeatures::POLYGON_MODE_LINE,
                ..default()
            }),
            ..default()
        });
    }

    app.add_plugins(default_plugins);

    // Brine-specific plugins.

    app.add_plugins(ProtocolPlugin);

    if let Some(chunk_dir) = args.chunk_dir {
        app.add_plugins((
            AlwaysSuccessfulLoginPlugin,
            ServeChunksFromDirectoryPlugin::new(chunk_dir),
        ));
    } else {
        app.add_plugins(ProtocolBackendPlugin);
        let server = args.server.clone().unwrap_or_else(|| SERVER.to_string());
        app.add_plugins(
            LoginPlugin::new(server, USERNAME.to_string()).exit_on_disconnect(),
        );
    }

    let mc_data = MinecraftData::for_version("1.14.4");
    // Point at the vanilla 1.14.4 assets directory (contains assets/, data/, pack.mcmeta).
    let mc_assets = MinecraftAssets::new("assets/1.14.4", &mc_data).unwrap();
    app.insert_resource(mc_data);
    app.insert_resource(mc_assets);
    app.add_plugins((TextureBuilderPlugin, MinecraftWorldViewerPlugin));

    // Debugging, diagnostics, and utility plugins.

    if args.debug {
        app.add_plugins((
            WorldInspectorPlugin::new(),
            DebugWireframePlugin,
            FrameTimeDiagnosticsPlugin::default(),
            LogDiagnosticsPlugin::default(),
        ));
    }

    app.run();
}

#[derive(Default)]
pub struct MinecraftWorldViewerPlugin;

impl Plugin for MinecraftWorldViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
                NoCameraPlayerPlugin,
                ChunkBuilderPlugin::<VisibleFacesChunkBuilder>::default(),
                // ChunkBuilderPlugin::<GreedyQuadsChunkBuilder>::default(),
            ))
            .add_systems(Startup, set_up_camera)
            .add_systems(Update, give_chunk_sections_correct_y_height);
    }
}

fn set_up_camera(mut commands: Commands) {
    // Screenshot coords.
    let camera_start = Transform::from_translation(Vec3::new(-200.0, 87.8, 157.3))
        .with_rotation(Quat::from_euler(EulerRot::XYZ, 0.1338, 0.183, -0.025));

    // let camera_start = Transform::from_translation(Vec3::new(-260.0, 115.0, 200.0))
    //     .looking_at(Vec3::new(-40.0, 100.0, 0.0), Vec3::Y);

    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        FlyCam,
        camera_start,
        GlobalTransform::default(),
    ));
}

fn give_chunk_sections_correct_y_height(mut query: Query<(&mut Transform, &BuiltChunkSection)>) {
    for (mut transform, chunk_section) in query.iter_mut() {
        let height = (chunk_section.section_y as f32) * 16.0;
        if transform.translation.y != height {
            transform.translation.y = height;
        }
    }
}
