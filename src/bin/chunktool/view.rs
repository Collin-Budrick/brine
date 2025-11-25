use std::{
    f32::consts::PI,
    path::{Path, PathBuf},
};

use bevy::{
    input::ButtonInput,
    log::{Level, LogPlugin},
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
    render::{
        render_resource::WgpuFeatures,
        settings::{RenderCreation, WgpuSettings},
        RenderPlugin,
    },
};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use brine_asset::MinecraftAssets;
use brine_chunk::{Chunk, ChunkSection};
use brine_data::MinecraftData;
use brine_proto::{event, ProtocolPlugin};
use brine_voxel_v1::{
    chunk_builder::{
        component::{BuiltChunk, BuiltChunkSection},
        ChunkBuilderPlugin, GreedyQuadsChunkBuilder, NaiveBlocksChunkBuilder,
        VisibleFacesChunkBuilder,
    },
    texture::TextureBuilderPlugin,
};

use brine::{
    chunk::{load_chunk, Result},
    error::log_error,
    DEFAULT_LOG_FILTER,
};
use clap::ValueEnum;

/// Loads a chunk from a file and views it in 3D.
#[derive(clap::Args)]
pub struct Args {
    /// Paths to one or more chunk data files to load.
    files: Vec<PathBuf>,

    /// Which chunk builder to test.
    #[arg(value_enum, short, long, default_value_t = ChunkBuilderType::VisibleFaces)]
    builder: ChunkBuilderType,
}

#[derive(Clone, ValueEnum)]
#[clap(value_enum, rename_all = "snake_case")]
enum ChunkBuilderType {
    VisibleFaces,
    GreedyQuads,
}

#[derive(Resource)]
struct Chunks {
    files: Vec<PathBuf>,
    next_file: usize,

    chunk: Option<Chunk>,
    next_section: usize,
}

impl Chunks {
    fn new(files: Vec<PathBuf>) -> Self {
        Self {
            files,
            next_file: 0,

            chunk: None,
            next_section: 0,
        }
    }

    fn chunk(&self) -> &Chunk {
        self.chunk.as_ref().unwrap()
    }

    fn next_file(&mut self) -> &Path {
        let path = &self.files[self.next_file];
        self.next_file = (self.next_file + 1) % self.files.len();
        path
    }

    fn load_next_file(&mut self) -> Result<()> {
        let path = self.next_file();
        let chunk = load_chunk(path)?;
        self.next_section = chunk.sections.len() - 1;
        self.chunk = Some(chunk);
        Ok(())
    }

    fn next_section(&mut self) -> ChunkSection {
        let sections = &self.chunk().sections;
        let section = sections[self.next_section].clone();
        self.next_section = if self.next_section == 0 {
            sections.len() - 1
        } else {
            self.next_section - 1
        };
        section
    }

    fn send_next_section(&mut self, chunk_events: &mut MessageWriter<event::clientbound::ChunkData>) {
        let section = self.next_section();

        let single_section_chunk = Chunk {
            sections: vec![section],
            ..Chunk::empty(self.chunk().chunk_x, self.chunk().chunk_z)
        };

        chunk_events.write(event::clientbound::ChunkData {
            chunk_data: single_section_chunk,
        });
    }
}

const DISTANCE_FROM_ORIGIN: f32 = 13.0;

pub fn main(args: Args) {
    let mut app = App::new();

    app.add_plugins(
        DefaultPlugins
            .set(LogPlugin {
                level: Level::DEBUG,
                filter: String::from(DEFAULT_LOG_FILTER),
                ..default()
            })
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    features: WgpuFeatures::POLYGON_MODE_LINE,
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(WireframeConfig {
        global: true,
        default_color: Color::WHITE,
    })
    .add_plugins((
        WireframePlugin::default(),
        WorldInspectorPlugin::new(),
        ProtocolPlugin,
    ));

    let mc_data = MinecraftData::for_version("1.14.4");
    let mc_assets = MinecraftAssets::new("assets/1.14.4", &mc_data).unwrap();
    app.insert_resource(mc_data);
    app.insert_resource(mc_assets);
    app.add_plugins(TextureBuilderPlugin);

    app.add_plugins(ChunkBuilderPlugin::<NaiveBlocksChunkBuilder>::shared());

    match args.builder {
        ChunkBuilderType::VisibleFaces => {
            app.add_plugins(ChunkBuilderPlugin::<VisibleFacesChunkBuilder>::shared());
        }
        ChunkBuilderType::GreedyQuads => {
            app.add_plugins(ChunkBuilderPlugin::<GreedyQuadsChunkBuilder>::shared());
        }
    }

    app.add_plugins(ChunkViewerPlugin);

    app.add_systems(Startup, (load_first_chunk.pipe(log_error), set_up_camera))
        .add_systems(Update, load_next_chunk.pipe(log_error));

    app.insert_resource(Chunks::new(args.files));
    app.run();
}

fn load_first_chunk(
    mut chunks: ResMut<Chunks>,
    mut chunk_events: MessageWriter<event::clientbound::ChunkData>,
) -> Result<()> {
    chunks.load_next_file()?;
    chunks.send_next_section(&mut chunk_events);
    Ok(())
}

fn load_next_chunk(
    input: Res<ButtonInput<KeyCode>>,
    mut chunks: ResMut<Chunks>,
    mut chunk_events: MessageWriter<event::clientbound::ChunkData>,
    query: Query<Entity, With<BuiltChunk>>,
    mut commands: Commands,
) -> Result<()> {
    let should_show_next =
        input.just_pressed(KeyCode::Enter) || input.just_pressed(KeyCode::Space);
    let should_load_next_file = input.just_pressed(KeyCode::Enter);

    if should_load_next_file {
        chunks.load_next_file()?;
    }

    if should_show_next {
        for entity in query.iter() {
            commands.entity(entity).despawn();
        }

        chunks.send_next_section(&mut chunk_events);
    }

    Ok(())
}

fn set_up_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_translation(Vec3::new(0.0, 8.0, 38.0)).looking_at(Vec3::ZERO, Vec3::Y),
        GlobalTransform::default(),
    ));
}

struct ChunkViewerPlugin;

impl Plugin for ChunkViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                Self::center_section_at_bottom_of_chunk,
                Self::rename_chunks,
                Self::move_and_rotate,
                Self::rotate_chunk,
            ),
        );
    }
}

impl ChunkViewerPlugin {
    fn move_and_rotate(mut query: Query<(&mut Transform, &BuiltChunk), Added<BuiltChunk>>) {
        for (mut transform, built_chunk) in query.iter_mut() {
            transform.rotate(Quat::from_rotation_y(PI / 4.0));

            use brine_voxel_v1::chunk_builder::ChunkBuilderType;
            match built_chunk.builder {
                ChunkBuilderType::NAIVE_BLOCKS => {
                    transform.translation = -Vec3::X * DISTANCE_FROM_ORIGIN;
                }
                _ => {
                    transform.translation = Vec3::X * DISTANCE_FROM_ORIGIN;
                }
            }
        }
    }

    fn rename_chunks(mut query: Query<(&mut Name, &BuiltChunk), Added<Name>>) {
        for (mut name, built_chunk) in query.iter_mut() {
            let builder_name = built_chunk.builder.0;
            let new_name = format!("{} ({})", name.as_str(), builder_name);
            name.set(new_name);
        }
    }

    fn center_section_at_bottom_of_chunk(
        mut query: Query<&mut Transform, Added<BuiltChunkSection>>,
    ) {
        for mut transform in query.iter_mut() {
            transform.translation = Vec3::new(-8.0, -8.0, -8.0);
        }
    }

    fn rotate_chunk(
        input: Res<ButtonInput<KeyCode>>,
        mut query: Query<&mut Transform, With<BuiltChunk>>,
    ) {
        for mut transform in query.iter_mut() {
            for keypress in input.get_just_pressed() {
                match keypress {
                    KeyCode::ArrowLeft => transform.rotate(Quat::from_rotation_y(-PI / 4.0)),
                    KeyCode::ArrowRight => transform.rotate(Quat::from_rotation_y(PI / 4.0)),
                    KeyCode::ArrowDown => transform.rotate(Quat::from_rotation_x(PI / 4.0)),
                    KeyCode::ArrowUp => transform.rotate(Quat::from_rotation_x(-PI / 4.0)),
                    KeyCode::Escape => transform.rotation = Quat::from_rotation_y(PI / 4.0),
                    _ => {}
                }
            }
        }
    }
}
