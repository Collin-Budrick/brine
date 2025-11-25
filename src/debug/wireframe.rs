use bevy::{
    pbr::wireframe::{WireframeConfig, WireframePlugin},
    prelude::*,
};

pub struct DebugWireframePlugin;

impl Plugin for DebugWireframePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WireframePlugin::default())
            .register_type::<EnableWireframe>()
            .add_systems(Startup, spawn_component)
            .add_systems(Update, update_wireframe_config);
    }
}

#[derive(Component, Reflect, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[reflect(Component)]
pub struct EnableWireframe {
    pub enable: bool,
}

fn spawn_component(mut commands: Commands) {
    commands.spawn((
        Name::new("Debug Wireframe"),
        EnableWireframe { enable: true },
    ));
}

fn update_wireframe_config(
    component: Query<&EnableWireframe>,
    mut wireframe_config: ResMut<WireframeConfig>,
) {
    if let Ok(component) = component.single() {
        wireframe_config.global = component.enable;
    }
}
