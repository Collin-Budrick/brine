use bevy::{ecs::schedule::IntoScheduleConfigs, prelude::*};

use crate::event::{clientbound::LoginSuccess, serverbound::Login, Uuid};

/// A plugin that responds immediately with success to the first login request.
///
/// # Events
///
/// The plugin does not register any events.
///
/// The plugin acts on the following events:
///
/// * [`Login`]
///
/// The plugin sends the following events:
///
/// * [`LoginSuccess`]
///
/// # Resources
///
/// The plugin does not register any resources.
///
/// The plugin does not expect any resources to exist.
pub struct AlwaysSuccessfulLoginPlugin;

impl Plugin for AlwaysSuccessfulLoginPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<ServerState>();
        app.add_systems(Update, handle_login.run_if(in_state(ServerState::Login)));
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, States, Default)]
enum ServerState {
    #[default]
    Login,
    Play,
}

fn handle_login(
    mut next_state: ResMut<NextState<ServerState>>,
    mut rx: MessageReader<Login>,
    mut tx: MessageWriter<LoginSuccess>,
) {
    if let Some(login) = rx.read().last() {
        debug!("Dummy server advancing to state Play");
        next_state.set(ServerState::Play);

        tx.write(LoginSuccess {
            uuid: Uuid::new_v4(),
            username: login.username.clone(),
        });
    }
}
