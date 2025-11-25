use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::{any::Any, marker::PhantomData};

use bevy::{pbr::MeshMaterial3d, prelude::*, tasks::AsyncComputeTaskPool};
use bevy_image::{TextureAtlasLayout, TextureAtlasSources};
use bevy_mesh::Mesh3d;
use futures_lite::future;

use brine_asset::{api::BlockFace, MinecraftAssets};
use brine_chunk::ChunkSection;
use brine_data::BlockStateId;
use brine_proto::event;

use crate::chunk_builder::component::PendingChunk;
use crate::mesh::VoxelMesh;
use crate::texture::BlockTextures;

use super::component::{ChunkSection as ChunkSectionComponent, PendingMeshAtlas};

use super::{
    component::{BuiltChunkBundle, BuiltChunkSectionBundle},
    ChunkBuilder,
};

/// Plugin that asynchronously generates renderable entities from chunk data.
///
/// The [`ChunkBuilderPlugin`] listens for [`ChunkData`] events from the backend
/// and spawns a task to run a particular [`ChunkBuilder`]. When the task
/// completes, the plugin adds the result to the game world.
///
/// [`ChunkData`]: brine_proto::event::clientbound::ChunkData
pub struct ChunkBuilderPlugin<T: ChunkBuilder> {
    shared: bool,
    _phantom: PhantomData<T>,
}

impl<T: ChunkBuilder> ChunkBuilderPlugin<T> {
    /// For (potentially premature) performance reasons, the default behavior of
    /// the [`ChunkBuilderPlugin`] is to consume `ChunkData` events (i.e.,
    /// [`Messages::drain()`]) so they can be moved into the builder task rather
    /// than cloned.
    ///
    /// [`Messages::drain()`]: bevy::prelude::Messages::drain
    ///
    /// This constructor allows multiple chunk builder plugins to exist
    /// simultaneously without them clobbering each other. It forces the plugin
    /// to use a regular old [`EventReader`] rather than draining the events.
    pub fn shared() -> Self {
        Self {
            shared: true,
            ..Default::default()
        }
    }
}

impl<T: ChunkBuilder> Default for ChunkBuilderPlugin<T> {
    fn default() -> Self {
        Self {
            shared: false,
            _phantom: PhantomData,
        }
    }
}

impl<T> Plugin for ChunkBuilderPlugin<T>
where
    T: ChunkBuilder + Default + Send + Sync + 'static,
{
    fn build(&self, app: &mut App) {
        if self.shared {
            app.add_systems(Update, Self::builder_task_spawn_shared);
        } else {
            app.add_systems(Update, Self::builder_task_spawn_unique);
        }

        app.add_systems(
            Update,
            (Self::receive_built_meshes, Self::add_built_chunks_to_world),
        );
    }
}

impl<T> ChunkBuilderPlugin<T>
where
    T: ChunkBuilder + Default + Any + Send + Sync + 'static,
{
    fn builder_task_spawn(chunk_event: event::clientbound::ChunkData, commands: &mut Commands) {
        let chunk = chunk_event.chunk_data;
        if !chunk.is_full() {
            return;
        }

        let chunk_x = chunk.chunk_x;
        let chunk_z = chunk.chunk_z;

        debug!("Received chunk ({}, {}), spawning task", chunk_x, chunk_z);

        let task_pool = AsyncComputeTaskPool::get();
        let task = task_pool.spawn(async move {
            let built = T::default().build_chunk(&chunk);
            (chunk, built)
        });

        let mut pending_chunk = PendingChunk::new(T::TYPE);
        pending_chunk.task = Some(task);

        commands.spawn((
            pending_chunk,
            Name::new(format!("Pending Chunk ({}, {})", chunk_x, chunk_z)),
        ));
    }

    fn build_texture_atlas_for_mesh(
        mesh: &VoxelMesh,
        chunk_section: &ChunkSection,
        asset_server: &AssetServer,
        mc_assets: &MinecraftAssets,
        texture_builder: &mut BlockTextures,
        atlas_layouts: &mut Assets<TextureAtlasLayout>,
        textures: &mut Assets<Image>,
    ) -> PendingMeshAtlas {
        // One strong texture handle for each unique texture that will make up
        // the atlas.
        let mut texture_handles: HashSet<Handle<Image>> = Default::default();

        // Texture handles, one for each face in the mesh.
        let mut face_textures: Vec<Handle<Image>> = Vec::with_capacity(mesh.faces.len());

        // Cached mapping from block state id to texture handle.
        let mut handle_cache: HashMap<(BlockStateId, BlockFace), Handle<Image>> =
            Default::default();

        for face in mesh.faces.iter() {
            let [x, y, z] = face.voxel;

            let face = face.axis.into();

            let block_state_id = chunk_section.get_block((x, y, z)).unwrap();
            let block_state_id = BlockStateId(block_state_id.0 as u16);

            let key = (block_state_id, face);
            let handle = match handle_cache.entry(key) {
                Entry::Vacant(entry) => {
                    let strong_handle = match mc_assets
                        .get_texture_path_for_block_state_and_face(block_state_id, face)
                    {
                        Some(path) => asset_server.load(path),
                        None => {
                            debug!("No texture for {:?}:{:?}", block_state_id, face);
                            texture_builder.placeholder_texture.clone()
                        }
                    };

                    if !texture_handles.contains(&strong_handle) {
                        texture_handles.insert(strong_handle.clone());
                    }

                    entry.insert(strong_handle.clone()).clone()
                }
                Entry::Occupied(entry) => entry.get().clone(),
            };

            face_textures.push(handle);
        }

        // debug!("texture_handles: {:#?}", &texture_handles);
        // debug!("face_textures: {:#?}", &face_textures);
        // debug!("handle_cache: {:#?}", &handle_cache);

        let (atlas_texture, layout) = texture_builder.create_texture_atlas_with_textures(
            texture_handles.into_iter(),
            textures,
            atlas_layouts,
        );

        PendingMeshAtlas {
            texture: atlas_texture,
            layout,
            face_textures,
        }
    }

    fn add_built_chunk_to_world(
        chunk_data: brine_chunk::Chunk,
        voxel_meshes: Vec<VoxelMesh>,
        atlas_data: Vec<(&TextureAtlasLayout, &TextureAtlasSources, Handle<Image>)>,
        face_textures: Vec<Vec<Handle<Image>>>,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        commands: &mut Commands,
    ) -> Entity {
        debug!(
            "Adding chunk ({}, {}) to world",
            chunk_data.chunk_x, chunk_data.chunk_z
        );
        commands
            .spawn(BuiltChunkBundle::new(
                T::TYPE,
                chunk_data.chunk_x,
                chunk_data.chunk_z,
            ))
            .with_children(move |parent| {
                for (((section, mut mesh), (layout, sources, texture_handle)), face_textures) in
                    chunk_data
                        .sections
                        .into_iter()
                        .zip(voxel_meshes.into_iter())
                        .zip(atlas_data.into_iter())
                        .zip(face_textures.into_iter())
                {
                    mesh.adjust_tex_coords(layout, sources, &face_textures);

                    parent
                        .spawn((
                            BuiltChunkSectionBundle::new(T::TYPE, section.chunk_y),
                            Mesh3d(meshes.add(mesh.to_render_mesh())),
                            MeshMaterial3d(materials.add(StandardMaterial {
                                base_color_texture: Some(texture_handle.clone()),
                                unlit: true,
                                ..Default::default()
                            })),
                        ))
                        .insert(ChunkSectionComponent(section));
                }
            })
            .id()
    }

    /*
      ____            _
     / ___| _   _ ___| |_ ___ _ __ ___  ___
     \___ \| | | / __| __/ _ \ '_ ` _ \/ __|
      ___) | |_| \__ \ ||  __/ | | | | \__ \
     |____/ \__, |___/\__\___|_| |_| |_|___/
            |___/
    */

    fn builder_task_spawn_unique(
        mut chunk_events: ResMut<Messages<event::clientbound::ChunkData>>,
        mut commands: Commands,
    ) {
        for chunk_event in chunk_events.drain() {
            Self::builder_task_spawn(chunk_event, &mut commands);
        }
    }

    fn builder_task_spawn_shared(
        mut chunk_events: MessageReader<event::clientbound::ChunkData>,
        mut commands: Commands,
    ) {
        for chunk_event in chunk_events.read() {
            Self::builder_task_spawn(chunk_event.clone(), &mut commands);
        }
    }

    fn receive_built_meshes(
        asset_server: Res<AssetServer>,
        mc_assets: Res<MinecraftAssets>,
        mut chunks_with_pending_meshes: Query<(Entity, &mut PendingChunk)>,
        mut texture_builder: ResMut<BlockTextures>,
        mut atlas_layouts: ResMut<Assets<TextureAtlasLayout>>,
        mut textures: ResMut<Assets<Image>>,
    ) {
        const MAX_PER_FRAME: usize = 1;

        for (i, (_, mut pending_chunk)) in chunks_with_pending_meshes.iter_mut().enumerate() {
            if i >= MAX_PER_FRAME {
                break;
            }

            if pending_chunk.builder != T::TYPE {
                continue;
            }

            if let Some(task) = pending_chunk.task.as_mut() {
                if let Some((chunk, voxel_meshes)) = future::block_on(future::poll_once(task)) {
                    debug!(
                        "Received meshes for Chunk ({}, {})",
                        chunk.chunk_x, chunk.chunk_z
                    );

                    let texture_atlases = voxel_meshes
                        .iter()
                        .zip(chunk.sections.iter())
                        .map(|(mesh, chunk_section)| {
                            Self::build_texture_atlas_for_mesh(
                                mesh,
                                chunk_section,
                                &*asset_server,
                                &*mc_assets,
                                &mut *texture_builder,
                                &mut *atlas_layouts,
                                &mut *textures,
                            )
                        })
                        .collect();

                    pending_chunk.chunk_data = Some(chunk);
                    pending_chunk.voxel_meshes = Some(voxel_meshes);
                    pending_chunk.texture_atlases = Some(texture_atlases);
                    pending_chunk.task = None;
                }
            }
        }
    }

    fn add_built_chunks_to_world(
        atlas_layouts: Res<Assets<TextureAtlasLayout>>,
        block_textures: Res<BlockTextures>,
        mut chunks_with_pending_atlases: Query<(Entity, &mut PendingChunk)>,
        mut meshes: ResMut<Assets<Mesh>>,
        mut materials: ResMut<Assets<StandardMaterial>>,
        mut commands: Commands,
    ) {
        for (entity, mut pending_chunk) in chunks_with_pending_atlases.iter_mut() {
            if pending_chunk.builder != T::TYPE {
                continue;
            }

            let Some(pending_atlases) = pending_chunk.texture_atlases.as_ref() else {
                continue;
            };

            let mut atlas_data = Vec::with_capacity(pending_atlases.len());
            let mut ready = true;
            for pending_atlas in pending_atlases.iter() {
                let layout = match atlas_layouts.get(&pending_atlas.layout) {
                    Some(layout) => layout,
                    None => {
                        ready = false;
                        break;
                    }
                };
                let sources = match block_textures.atlas_sources(&pending_atlas.texture) {
                    Some(sources) => sources,
                    None => {
                        ready = false;
                        break;
                    }
                };
                atlas_data.push((layout, sources, pending_atlas.texture.clone()));
            }

            if !ready {
                continue;
            }

            let face_textures: Vec<Vec<Handle<Image>>> = pending_chunk
                .texture_atlases
                .take()
                .unwrap()
                .into_iter()
                .map(|atlas| atlas.face_textures)
                .collect();

            let chunk = pending_chunk.chunk_data.take().unwrap();
            let voxel_meshes = pending_chunk.voxel_meshes.take().unwrap();

            debug!(
                "Received all texture atlases for Chunk ({}, {})",
                chunk.chunk_x, chunk.chunk_z
            );

            Self::add_built_chunk_to_world(
                chunk,
                voxel_meshes,
                atlas_data,
                face_textures,
                &mut *meshes,
                &mut *materials,
                &mut commands,
            );

            commands.entity(entity).despawn();
        }
    }
}
