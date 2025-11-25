use std::str::FromStr;

use bevy::{
    asset::RenderAssetUsages,
    input::ButtonInput,
    pbr::MeshMaterial3d,
    prelude::*,
    render::render_resource::PrimitiveTopology,
};
use bevy::math::{primitives::Cuboid, Rect};
use bevy_mesh::{Indices, Mesh3d};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use brine::debug::DebugWireframePlugin;
use brine_asset::{BakedModel, BlockFace, MinecraftAssets};
use brine_data::{BlockStateId, MinecraftData};
use brine_render::texture::{
    MinecraftTexturesPlugin, MinecraftTexturesState, TextureAtlas, TextureManager,
    TextureManagerPlugin,
};

use crate::parse_block_reference;

/// Displays a block.
#[derive(clap::Args)]
pub struct Args {
    /// Block reference, e.g., "stone", "42", "100:111".
    block_reference: String,

    /// Optionally show only a specific face.
    #[arg(long)]
    show_faces: Option<ShowFaces>,
}

#[derive(Debug, Clone, Copy, Resource)]
struct ShowFaces {
    pub down: bool,
    pub up: bool,
    pub north: bool,
    pub south: bool,
    pub west: bool,
    pub east: bool,
}

impl ShowFaces {
    pub const fn all() -> Self {
        Self {
            down: true,
            up: true,
            north: true,
            south: true,
            west: true,
            east: true,
        }
    }

    pub const fn none() -> Self {
        Self {
            down: false,
            up: false,
            north: false,
            south: false,
            west: false,
            east: false,
        }
    }

    pub const fn only(face: BlockFace) -> Self {
        Self::none().with(face, true)
    }

    pub const fn with(self, face: BlockFace, show: bool) -> Self {
        match face {
            BlockFace::Down => Self { down: show, ..self },
            BlockFace::Up => Self { up: show, ..self },
            BlockFace::North => Self {
                north: show,
                ..self
            },
            BlockFace::South => Self {
                south: show,
                ..self
            },
            BlockFace::West => Self { west: show, ..self },
            BlockFace::East => Self { east: show, ..self },
        }
    }

    pub fn show(&self, face: BlockFace) -> bool {
        match face {
            BlockFace::Down => self.down,
            BlockFace::Up => self.up,
            BlockFace::North => self.north,
            BlockFace::South => self.south,
            BlockFace::West => self.west,
            BlockFace::East => self.east,
        }
    }

    pub fn parse(string: &str) -> Self {
        let mut show = Self::none();

        string
            .split(',')
            .filter_map(Self::parse_face)
            .for_each(|block_face| {
                show = show.with(block_face, true);
            });

        show
    }

    fn parse_face(face_str: &str) -> Option<BlockFace> {
        let lower = face_str.to_lowercase();
        match lower.as_str() {
            "d" | "down" => Some(BlockFace::Down),
            "u" | "up" => Some(BlockFace::Up),
            "n" | "north" => Some(BlockFace::North),
            "s" | "south" => Some(BlockFace::South),
            "w" | "west" => Some(BlockFace::West),
            "e" | "east" => Some(BlockFace::East),
            _ => None,
        }
    }
}

impl FromStr for ShowFaces {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::parse(value))
    }
}

pub(crate) fn main(args: Args) {
    let show_faces = args.show_faces.unwrap_or_else(ShowFaces::all);

    display_block(&args.block_reference, show_faces);
}

fn display_block(block_reference: &str, show_faces: ShowFaces) {
    let mc_data = MinecraftData::for_version("1.21.4");

    let block_state_ids = parse_block_reference(block_reference, &mc_data);
    println!("Requested to view block states: {:?}", block_state_ids);

    println!("Requested faces: {:?}", show_faces);

    println!("Loading Assets");
    let mc_assets = MinecraftAssets::new("assets/1.21.4", &mc_data).unwrap();

    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DebugWireframePlugin)
        .add_plugins(WorldInspectorPlugin::new())
        .insert_resource(show_faces)
        .insert_resource(mc_data)
        .insert_resource(mc_assets)
        .add_plugins(TextureManagerPlugin)
        .add_plugins(MinecraftTexturesPlugin)
        .insert_resource(TheBlocks::new(block_state_ids))
        .add_systems(OnEnter(MinecraftTexturesState::Loaded), setup)
        .add_systems(
            Update,
            next_block_state.run_if(in_state(MinecraftTexturesState::Loaded)),
        )
        .run();
}

#[derive(Debug, Resource)]
struct TheBlocks {
    block_state_ids: Vec<BlockStateId>,
    index: usize,
}

impl TheBlocks {
    pub fn new(block_state_ids: Vec<BlockStateId>) -> Self {
        Self {
            block_state_ids,
            index: 0,
        }
    }

    pub fn current_block(&self) -> BlockStateId {
        self.block_state_ids[self.index]
    }

    pub fn prev_block(&mut self) {
        self.index = if self.index == 0 {
            self.block_state_ids.len() - 1
        } else {
            self.index - 1
        };
    }

    pub fn next_block(&mut self) {
        self.index = (self.index + 1) % self.block_state_ids.len();
    }
}

#[derive(Component)]
struct BlockMarker;

fn setup(
    the_blocks: Res<TheBlocks>,
    show_faces: Res<ShowFaces>,
    mc_data: Res<MinecraftData>,
    mc_assets: Res<MinecraftAssets>,
    texture_manager: Res<TextureManager>,
    texture_atlases: Res<Assets<TextureAtlas>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_translation(Vec3::new(4.0, 3.0, 4.0)).looking_at(Vec3::ZERO, Vec3::Y),
        GlobalTransform::default(),
    ));

    let origin_cube = Mesh::from(Cuboid::from_size(Vec3::splat(1.0 / 16.0)));
    let origin_material = StandardMaterial {
        base_color: Color::srgb(1.0, 0.0, 1.0),
        unlit: true,
        ..default()
    };
    commands.spawn((
        Mesh3d(meshes.add(origin_cube)),
        MeshMaterial3d(materials.add(origin_material)),
        Transform::default(),
        GlobalTransform::default(),
        Visibility::Hidden,
        Name::new("Origin"),
    ));

    spawn_block_state(
        the_blocks.current_block(),
        show_faces.into_inner(),
        mc_data.into_inner(),
        mc_assets.into_inner(),
        texture_manager.into_inner(),
        texture_atlases.into_inner(),
        meshes.into_inner(),
        materials.into_inner(),
        &mut commands,
    );
}

fn next_block_state(
    input: Res<ButtonInput<KeyCode>>,
    the_blocks: ResMut<TheBlocks>,
    show_faces: Res<ShowFaces>,
    mc_data: Res<MinecraftData>,
    mc_assets: Res<MinecraftAssets>,
    texture_manager: Res<TextureManager>,
    texture_atlases: Res<Assets<TextureAtlas>>,
    blocks: Query<Entity, With<BlockMarker>>,
    meshes: ResMut<Assets<Mesh>>,
    materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let count = if input.pressed(KeyCode::ShiftLeft) {
        10
    } else {
        1
    };

    let next_block: Box<dyn Fn(&mut TheBlocks)> = if input.just_pressed(KeyCode::ArrowLeft) {
        Box::new(|b: &mut TheBlocks| {
            for _ in 0..count {
                b.prev_block()
            }
        })
    } else if input.just_pressed(KeyCode::ArrowRight) {
        Box::new(|b: &mut TheBlocks| {
            for _ in 0..count {
                b.next_block()
            }
        })
    } else {
        return;
    };

    let the_blocks = the_blocks.into_inner();
    let show_faces = show_faces.into_inner();
    let mc_data = mc_data.into_inner();
    let mc_assets = mc_assets.into_inner();
    let texture_manager = texture_manager.into_inner();
    let texture_atlases = texture_atlases.into_inner();
    let meshes = meshes.into_inner();
    let materials = materials.into_inner();

    // Despawn previous meshes
    for entity in blocks.iter() {
        commands.entity(entity).despawn();
    }

    next_block(the_blocks);

    while !spawn_block_state(
        the_blocks.current_block(),
        show_faces,
        mc_data,
        mc_assets,
        texture_manager,
        texture_atlases,
        meshes,
        materials,
        &mut commands,
    ) {
        info!("Skipping {:?}", the_blocks.current_block());
        next_block(the_blocks);
    }

    info!("Showing {:?}", the_blocks.current_block());
}

fn spawn_block_state(
    block_state_id: BlockStateId,
    show_faces: &ShowFaces,
    mc_data: &MinecraftData,
    mc_assets: &MinecraftAssets,
    texture_manager: &TextureManager,
    texture_atlases: &Assets<TextureAtlas>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    commands: &mut Commands,
) -> bool {
    let baked_block_state = mc_assets.block_states().get_by_key(block_state_id).unwrap();

    let mut has_model = false;

    for grab_bag in baked_block_state.models.iter() {
        let model_key = grab_bag.choices.first().unwrap();
        let baked_model = mc_assets.models().get_by_key(*model_key).unwrap();

        if baked_model.quads.is_empty() {
            continue;
        }

        debug!("Baked model: {:#?}", baked_model);

        has_model = true;

        let texture_key = baked_model.quads.first().unwrap().texture;
        let atlas_handle = texture_manager.get_atlas(texture_key).unwrap();
        let atlas = texture_atlases.get(&atlas_handle).unwrap();

        let mesh = baked_model_to_mesh(baked_model, atlas, show_faces);

        // debug!("{:#?}", mesh);

        let material = StandardMaterial {
            base_color_texture: Some(atlas.texture.clone()),
            unlit: true,
            ..Default::default()
        };

        commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(materials.add(material)),
                Transform::default(),
                GlobalTransform::default(),
                Name::new(get_entity_name(block_state_id, mc_data)),
                BlockMarker,
            ));
    }

    has_model
}

fn get_entity_name(block_state_id: BlockStateId, mc_data: &MinecraftData) -> String {
    let block = mc_data.blocks().get_by_state_id(block_state_id).unwrap();

    let display_name = block.display_name;

    let mut state_values: Vec<String> = block
        .state
        .iter()
        .map(|(property, value)| format!("{property}={value}"))
        .collect();
    state_values.sort();

    format!("{} [{}]", display_name, state_values.join(","))
}

fn baked_model_to_mesh(
    baked_model: &BakedModel,
    texture_atlas: &TextureAtlas,
    show_faces: &ShowFaces,
) -> Mesh {
    let num_quads = baked_model.quads.len();
    let num_vertices = num_quads * 4;
    let num_indices = num_quads * 6;

    let mut positions = Vec::with_capacity(num_vertices);
    let mut normals = Vec::with_capacity(num_vertices);
    let mut tex_coords = Vec::with_capacity(num_vertices);
    let mut indices = Vec::with_capacity(num_indices);

    for quad in baked_model.quads.iter() {
        debug!("quad.face = {:?}", quad.face);
        if !show_faces.show(quad.face) {
            continue;
        }

        indices.extend_from_slice(
            &quad
                .indices()
                .map(|index| (positions.len() + index as usize) as u32),
        );

        positions.extend_from_slice(&quad.positions);
        normals.extend_from_slice(&[quad.normal; 4]);

        let uvs_within_atlas = texture_atlas.get_uv(quad.texture);
        tex_coords.extend_from_slice(&adjust_tex_coords(quad.tex_coords, uvs_within_atlas));
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, tex_coords);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

fn adjust_tex_coords(tex_coords: [[f32; 2]; 4], atlas_rect: Rect) -> [[f32; 2]; 4] {
    tex_coords.map(|uv| adjust_uv_to_rect(uv, atlas_rect))
}

fn adjust_uv_to_rect([u, v]: [f32; 2], rect: Rect) -> [f32; 2] {
    let u = rect.min.x + rect.width() * u;
    // Using width as height is a temporary hack until I figure out how to deal
    // with tall textures.
    let v = rect.min.y + rect.width() * v;
    // let v = rect.min.y + rect.height() * v;

    [u, v]
}
