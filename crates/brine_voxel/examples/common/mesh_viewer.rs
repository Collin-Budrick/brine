use bevy::{
    asset::RenderAssetUsages,
    pbr::{
        wireframe::{WireframeConfig, WireframePlugin},
        MeshMaterial3d,
    },
    prelude::{StandardMaterial, *},
    render::{
        render_resource::{PrimitiveTopology, WgpuFeatures},
        settings::{RenderCreation, WgpuSettings},
        view::Msaa,
        RenderPlugin,
    },
};
use bevy_mesh::{Indices, Mesh3d};

use brine_voxel::Mesh as VoxelMesh;

use super::CHUNK_SIDE;

#[derive(Component)]
struct Root;

pub struct MeshViewerPlugin {
    mesh: VoxelMesh,
}

impl MeshViewerPlugin {
    pub fn new(mesh: VoxelMesh) -> Self {
        Self { mesh }
    }
}

impl Plugin for MeshViewerPlugin {
    fn build(&self, app: &mut App) {
        let mesh = build_bevy_mesh(&self.mesh);

        let mut meshes = app.world_mut().get_resource_mut::<Assets<Mesh>>().unwrap();
        let handle = meshes.add(mesh);

        app.world_mut().insert_resource(MeshHandle(handle));

        app.add_plugins(RenderPlugin {
                render_creation: RenderCreation::Automatic(WgpuSettings {
                    features: WgpuFeatures::POLYGON_MODE_LINE,
                    ..default()
                }),
                ..default()
            })
            .insert_resource(WireframeConfig {
                global: true,
                default_color: Color::WHITE,
            })
            .add_plugins(WireframePlugin::default())
            .add_systems(Startup, setup)
            .add_systems(Update, rotate);
    }
}

#[derive(Resource)]
struct MeshHandle(Handle<Mesh>);

fn setup(
    mesh: Res<MeshHandle>,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut commands: Commands,
) {
    let offset = CHUNK_SIDE as f32 / 2.0;

    commands
        .spawn((Transform::default(), GlobalTransform::default(), Root))
        .with_children(|parent| {
            parent.spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color_texture: Some(asset_server.load("placeholder.png")),
                    unlit: true,
                    ..Default::default()
                })),
                Transform::from_translation(Vec3::new(-offset, -offset, -offset)),
                GlobalTransform::default(),
            ));
        });

    // let mut camera = OrthographicCameraBundle::new_3d();
    // camera.transform =
    //     Transform::from_translation(Vec3::new(5.0, 5.0, 5.0)).looking_at(Vec3::ZERO, Vec3::Y);
    // camera.orthographic_projection.scale = 5.0;

    commands.spawn((
        Camera3d::default(),
        Msaa::Sample4,
        Transform::from_translation(Vec3::new(5.0, 5.0, 5.0)).looking_at(Vec3::ZERO, Vec3::Y),
        GlobalTransform::default(),
    ));
}

fn rotate(input: Res<ButtonInput<KeyCode>>, mut query: Query<&mut Transform, With<Root>>) {
    if let Ok(mut transform) = query.single_mut() {
        if input.just_pressed(KeyCode::ArrowRight) {
            transform.rotate(Quat::from_rotation_y(90.0_f32.to_radians()));
        }
        if input.just_pressed(KeyCode::ArrowLeft) {
            transform.rotate(Quat::from_rotation_y(-90.0_f32.to_radians()));
        }
    }
}

pub fn build_bevy_mesh(voxel_mesh: &VoxelMesh) -> Mesh {
    let num_vertices = voxel_mesh.quads.len() * 4;
    let num_indices = voxel_mesh.quads.len() * 6;
    let mut positions = Vec::with_capacity(num_vertices);
    let mut normals = Vec::with_capacity(num_vertices);
    let mut tex_coords = Vec::with_capacity(num_vertices);
    let mut indices = Vec::with_capacity(num_indices);

    for quad in voxel_mesh.quads.iter() {
        indices.extend_from_slice(
            &quad
                .get_indices()
                .map(|i| positions.len() as u32 + i as u32),
        );

        positions.extend_from_slice(&quad.positions);
        normals.extend_from_slice(&quad.get_normals());
        tex_coords.extend_from_slice(&quad.get_tex_coords());
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
