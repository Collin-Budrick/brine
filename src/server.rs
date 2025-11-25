use std::{
    any::Any,
    fs,
    path::{Path, PathBuf},
};

use bevy::{
    prelude::*,
    tasks::{IoTaskPool, Task},
};

use brine_chunk::Chunk;
use brine_proto::event::clientbound::ChunkData;
use futures_lite::future;

use crate::chunk::{load_chunk, Result};

/// A plugin that acts as a phony server, sending ChunkData events containing
/// data read from a directory of chunk data files.
pub struct ServeChunksFromDirectoryPlugin<P> {
    path: P,
}

impl<P> ServeChunksFromDirectoryPlugin<P> {
    pub fn new(path: P) -> Self {
        Self { path }
    }
}

impl<P> Plugin for ServeChunksFromDirectoryPlugin<P>
where
    P: AsRef<Path> + Any + Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        let path = PathBuf::from(self.path.as_ref());
        app.insert_resource(ChunkDirectory { path });
        app.add_systems(Startup, load_chunks);
        app.add_systems(Update, send_chunks);
    }
}

#[derive(Resource, Debug)]
pub struct ChunkDirectory {
    path: PathBuf,
}

#[derive(Component)]
struct LoadChunkTask(Task<Result<Chunk>>);

fn load_chunks(chunk_directory: Res<ChunkDirectory>, mut commands: Commands) {
    let task_pool = IoTaskPool::get();
    let entries = match fs::read_dir(&chunk_directory.path) {
        Ok(entries) => entries,
        Err(err) => {
            error!("Failed to read chunk directory: {}", err);
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                error!("Failed to read directory entry: {}", err);
                continue;
            }
        };

        let path_string = entry.file_name().to_string_lossy().to_string();

        if path_string.starts_with("chunk_light_") || !path_string.ends_with(".dump") {
            continue;
        }

        let path = entry.path();
        let chunk_name = path.to_string_lossy().to_string();
        let task_path = path.clone();
        let task = task_pool.spawn(async move { load_chunk(task_path) });

        commands.spawn((
            LoadChunkTask(task),
            Name::new(format!("Loading Chunk {}", chunk_name)),
        ));
    }
}

fn send_chunks(
    mut tasks: Query<(Entity, &mut LoadChunkTask)>,
    mut chunk_events: MessageWriter<ChunkData>,
    mut commands: Commands,
) {
    for (task_entity, mut task) in tasks.iter_mut() {
        if let Some(chunk_data) = future::block_on(future::poll_once(&mut task.0)) {
            match chunk_data {
                Ok(chunk_data) => {
                    chunk_events.write(ChunkData { chunk_data });
                    commands.entity(task_entity).despawn();
                }
                Err(err) => {
                    error!("{}", err);
                    commands.entity(task_entity).despawn();
                }
            }
        }
    }
}
