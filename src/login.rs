use bevy::{app::AppExit, ecs::schedule::IntoScheduleConfigs, prelude::*};

use brine_proto::event::{
    clientbound::{Disconnect, LoginSuccess},
    serverbound::Login,
};

#[derive(Debug, Clone, Eq, PartialEq, Hash, States, Default)]
pub enum GameState {
    #[default]
    Idle,
    Login,
    Play,
}

#[derive(Debug, Clone, Resource)]
struct LoginInfo {
    server: String,
    username: String,
    exit_on_disconnect: bool,
}

/// Simple plugin that initiates login to a Minecraft server on app startup.
pub struct LoginPlugin {
    info: LoginInfo,
}

impl LoginPlugin {
    pub fn new(server: String, username: String) -> Self {
        Self {
            info: LoginInfo {
                server,
                username,
                exit_on_disconnect: false,
            },
        }
    }

    pub fn exit_on_disconnect(mut self) -> Self {
        self.info.exit_on_disconnect = true;
        self
    }
}

impl Plugin for LoginPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.info.clone())
            .init_state::<GameState>()
            .add_systems(Startup, initiate_login)
            .add_systems(
                Update,
                (await_success, handle_disconnect).run_if(in_state(GameState::Login)),
            )
            .add_systems(Update, handle_disconnect.run_if(in_state(GameState::Play)));
    }
}

fn initiate_login(
    login_info: Res<LoginInfo>,
    mut login_events: MessageWriter<Login>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    info!("Initiating login");
    login_events.write(Login {
        server: login_info.server.clone(),
        username: login_info.username.clone(),
    });
    next_state.set(GameState::Login);
}

fn await_success(
    mut login_success_events: MessageReader<LoginSuccess>,
    mut next_state: ResMut<NextState<GameState>>,
) {
    if login_success_events.read().last().is_some() {
        info!("Login successful, advancing to state Play");
        next_state.set(GameState::Play);
    }
}

fn handle_disconnect(
    login_info: Res<LoginInfo>,
    mut disconnect_events: MessageReader<Disconnect>,
    mut next_state: ResMut<NextState<GameState>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if let Some(disconnect) = disconnect_events.read().last() {
        info!("Disconnected from server. Reason: {}", disconnect.reason);
        next_state.set(GameState::Idle);

        if login_info.exit_on_disconnect {
            app_exit.write(AppExit::Success);
        }
    }
}
