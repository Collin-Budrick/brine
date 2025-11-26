//! Implementation of the Minecraft protocol login handshake.
//!
//! This is driven by only a single message from the user's point of view:
//! [`Login`]. These systems handle all of the login logic.
//!
//! # The Login Process
//!
//! The login process consists of three phases:
//!
//! * Protocol Discovery
//!   1. Client connects
//!   1. C -> S: Handshake with Next State set to 1 (Status)
//!   2. C -> S: Status Request
//!   3. S -> C: Status Response (includes server's protocol version)
//!   4. C -> S: Status Ping
//!   5. S -> C: Status Pong
//!   6. Server disconnects
//!
//! * Login (unauthenticated)
//!   1. Client connects
//!   2. C -> S: Handshake with Next State set to 2 (Login)
//!   3. C -> S: Login Start
//!   4. S -> C: Login Success
//!
//! * Play
//!   * Periodic KeepAlive packets
//!   * Other play packets
//!
//! See these pages for reference:
//!
//! * <https://wiki.vg/Protocol#Handshaking>
//! * <https://wiki.vg/Protocol#Login>
//! * <https://wiki.vg/Protocol_FAQ#What.27s_the_normal_login_sequence_for_a_client.3F>

use bevy::{ecs::schedule::IntoScheduleConfigs, prelude::*};
use steven_protocol::protocol::{Serializable, VarInt};

use brine_net::{CodecReader, CodecWriter, NetworkError, NetworkEvent, NetworkResource};
use brine_proto::event::{
    clientbound::{Disconnect, LoginSuccess},
    serverbound::Login,
    Uuid,
};

use crate::codec::{HANDSHAKE_LOGIN_NEXT, HANDSHAKE_STATUS_NEXT};

use super::codec::{packet, Packet, ProtocolCodec};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, States, Default)]
enum LoginState {
    #[default]
    Idle,

    // Phase 1
    StatusAwaitingConnect,
    StatusAwaitingResponse,
    StatusAwaitingDisconnect,

    // Phase 2
    LoginAwaitingConnect,
    LoginAwaitingSuccess,

    Play,
}

/// Keeps data around that is needed by systems occurring later in the state machine.
#[derive(Resource)]
struct LoginResource {
    username: String,
    server_addr: String,
}

#[derive(Resource, Default)]
struct ConfigurationState {
    started: bool,
}

pub(crate) fn build(app: &mut App) {
    app.init_state::<LoginState>();
    app.init_resource::<ConfigurationState>();

    protocol_discovery::build(app);
    login::build(app);
    play::build(app);
}

fn make_handshake_packet(protocol_version: i32, next_state: i32) -> Packet {
    Packet::Known(packet::Packet::HandshakingServerboundSetProtocol(Box::new(
        packet::handshake::serverbound::SetProtocol {
            protocolVersion: VarInt(protocol_version),
            // Next state to go to (1 for status, 2 for login)
            nextState: VarInt(next_state),
            ..Default::default()
        },
    )))
}

/// System that listens for any connection failure event and emits a LoginFailure event.
fn handle_connection_error(
    mut network_events: MessageReader<NetworkEvent<ProtocolCodec>>,
    mut login_failure_events: MessageWriter<Disconnect>,
    mut login_state: ResMut<NextState<LoginState>>,
) {
    for event in network_events.read() {
        if let NetworkEvent::Error(NetworkError::ConnectFailed(io_error)) = event {
            error!("Connection failed: {}", io_error);

            login_failure_events.write(Disconnect {
                reason: format!("Connection failed: {}", io_error),
            });

            login_state.set(LoginState::Idle);
            break;
        }
    }
}

mod protocol_discovery {
    use super::*;

    pub(crate) fn build(app: &mut App) {
        app.add_systems(
            Update,
            await_login_event_then_connect.run_if(in_state(LoginState::Idle)),
        );
        app.add_systems(
            Update,
            (
                handle_connection_error,
                await_connect_then_send_handshake_and_status_request,
            )
                .run_if(in_state(LoginState::StatusAwaitingConnect)),
        );
        app.add_systems(
            Update,
            await_response_then_send_status_ping
                .run_if(in_state(LoginState::StatusAwaitingResponse)),
        );
        app.add_systems(
            Update,
            await_disconnect_then_connect_for_login
                .run_if(in_state(LoginState::StatusAwaitingDisconnect)),
        );
    }

    fn await_login_event_then_connect(
        mut login_events: MessageReader<Login>,
        mut login_state: ResMut<NextState<LoginState>>,
        mut net_resource: ResMut<NetworkResource<ProtocolCodec>>,
        mut commands: Commands,
    ) {
        if let Some(login) = login_events.read().last() {
            info!("Logging in to server {}", login.server);

            debug!("Connecting to server for protocol discovery.");
            net_resource.connect(login.server.clone());

            commands.insert_resource(LoginResource {
                username: login.username.clone(),
                server_addr: login.server.clone(),
            });

            login_state.set(LoginState::StatusAwaitingConnect);
        }
    }

    fn await_connect_then_send_handshake_and_status_request(
        mut network_events: MessageReader<NetworkEvent<ProtocolCodec>>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
        mut login_state: ResMut<NextState<LoginState>>,
        net_resource: Res<NetworkResource<ProtocolCodec>>,
    ) {
        for event in network_events.read() {
            if let NetworkEvent::Connected = event {
                debug!("Connection established. Sending Handshake and StatusRequest packets.");

                let handshake = make_handshake_packet(
                    net_resource.codec().protocol_version(),
                    HANDSHAKE_STATUS_NEXT,
                );
                trace!("{:#?}", &handshake);
                packet_writer.send(handshake);

                let status_request = Packet::Known(packet::Packet::StatusServerboundPingStart(
                    Box::new(packet::status::serverbound::PingStart::default()),
                ));
                packet_writer.send(status_request);

                login_state.set(LoginState::StatusAwaitingResponse);
                break;
            }
        }
    }

    fn await_response_then_send_status_ping(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
        mut login_state: ResMut<NextState<LoginState>>,
        net_resource: Res<NetworkResource<ProtocolCodec>>,
    ) {
        for packet in packet_reader.iter() {
            if let Packet::Known(packet::Packet::StatusClientboundServerInfo(_)) = packet {
                // The codec will have already switched its internal protocol
                // version in response to decoding the StatusResponse packet,
                // so just read it from there.
                let protocol_version = net_resource.codec().protocol_version();

                debug!(
                    "StatusResponse received. Server protocol version = {}",
                    protocol_version
                );

                debug!("Sending StatusPing.");
                let status_ping = Packet::Known(packet::Packet::StatusServerboundPing(Box::new(
                    packet::status::serverbound::Ping { time: 0 },
                )));
                packet_writer.send(status_ping);

                login_state.set(LoginState::StatusAwaitingDisconnect);
                break;
            }
        }
    }

    fn await_disconnect_then_connect_for_login(
        mut network_events: MessageReader<NetworkEvent<ProtocolCodec>>,
        mut login_state: ResMut<NextState<LoginState>>,
        mut net_resource: ResMut<NetworkResource<ProtocolCodec>>,
        login_resource: Res<LoginResource>,
    ) {
        for event in network_events.read() {
            if let NetworkEvent::Disconnected = event {
                debug!("Server disconnected as expected.");
                debug!("Connecting to server for login.");
                net_resource.connect(login_resource.server_addr.clone());

                login_state.set(LoginState::LoginAwaitingConnect);
            }
        }
    }
}

#[allow(clippy::module_inception)]
mod login {
    use super::*;

    pub(crate) fn build(app: &mut App) {
        app.add_systems(
            Update,
            (
                handle_connection_error,
                await_connect_then_send_handshake_and_login_start,
            )
                .run_if(in_state(LoginState::LoginAwaitingConnect)),
        );
        app.add_systems(
            Update,
            await_login_success.run_if(in_state(LoginState::LoginAwaitingSuccess)),
        );
    }

    fn make_login_start_packet(_protocol_version: i32, username: String) -> Packet {
        Packet::Known(packet::Packet::LoginServerboundLoginStart(Box::new(
            packet::login::serverbound::LoginStart {
                username,
                ..Default::default()
            },
        )))
    }

    /// System that listens for a successful connection event and then sends the
    /// first two packets of the login exchange.
    fn await_connect_then_send_handshake_and_login_start(
        mut network_events: MessageReader<NetworkEvent<ProtocolCodec>>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
        mut login_state: ResMut<NextState<LoginState>>,
        login_resource: Res<LoginResource>,
        net_resource: Res<NetworkResource<ProtocolCodec>>,
    ) {
        for event in network_events.read() {
            if let NetworkEvent::Connected = event {
                debug!("Connection established. Sending Handshake and LoginStart packets.");

                let protocol_version = net_resource.codec().protocol_version();

                let handshake = make_handshake_packet(protocol_version, HANDSHAKE_LOGIN_NEXT);
                trace!("{:#?}", &handshake);
                packet_writer.send(handshake);

                let login_start =
                    make_login_start_packet(protocol_version, login_resource.username.clone());
                trace!("{:#?}", &login_start);
                packet_writer.send(login_start);

                login_state.set(LoginState::LoginAwaitingSuccess);
                break;
            }
        }
    }

    /// System that listens for either a LoginSuccess or LoginDisconnect packet and
    /// emits the proper event in response.
    fn await_login_success(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
        mut login_success_events: MessageWriter<LoginSuccess>,
        mut disconnect_events: MessageWriter<Disconnect>,
        mut login_state: ResMut<NextState<LoginState>>,
    ) {
        let mut on_login_success = |username: String, uuid: Uuid| {
            info!("Successfully logged in to server.");

            login_success_events.write(LoginSuccess { username, uuid });

            login_state.set(LoginState::Play);
        };

        for packet in packet_reader.iter() {
            match packet {
                Packet::Known(packet::Packet::LoginClientboundSuccess(login_success)) => {
                    // Acknowledge login per 1.21 protocol.
                    let ack = Packet::Known(packet::Packet::LoginServerboundLoginAcknowledged(
                        Box::new(packet::login::serverbound::LoginAcknowledged {}),
                    ));
                    packet_writer.send(ack);

                    let mut uuid_bytes = Vec::with_capacity(16);
                    login_success.uuid.write_to(&mut uuid_bytes).unwrap();
                    let uuid = Uuid::from_bytes(uuid_bytes.try_into().unwrap());

                    on_login_success(login_success.username.clone(), uuid);
                    break;
                }

                Packet::Known(packet::Packet::LoginClientboundDisconnect(login_disconnect)) => {
                    let message = format!("Login disconnect: {}", login_disconnect.reason);
                    error!("{}", &message);

                    disconnect_events.write(Disconnect { reason: message });

                    login_state.set(LoginState::Idle);
                    break;
                }

                _ => {}
            }
        }
    }
}

mod play {
    use super::*;

    pub(crate) fn build(app: &mut App) {
        app.add_systems(
            Update,
            (
                respond_to_keep_alive_packets,
                handle_configuration_start,
                respond_to_position_packets,
                respond_to_chunk_batch_packets,
                handle_disconnect,
            )
                .run_if(in_state(LoginState::Play)),
        );
    }

    fn handle_configuration_start(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
        mut config_state: ResMut<ConfigurationState>,
    ) {
        let send_play_settings = |writer: &mut CodecWriter<ProtocolCodec>| {
            let settings = Packet::Known(packet::Packet::PlayServerboundSettings(Box::new(
                packet::play::serverbound::Settings {
                    locale: "en_us".to_string(),
                    viewDistance: 12,
                    chatFlags: VarInt(0),
                    chatColors: true,
                    skinParts: 0x7F,
                    mainHand: VarInt(1), // 0=left,1=right
                    enableTextFiltering: false,
                    enableServerListing: true,
                    particleStatus: packet::SettingsParticlestatus::All,
                },
            )));
            writer.send(settings);
        };

        for packet in packet_reader.iter() {
            if let Packet::Known(packet::Packet::PlayClientboundStartConfiguration(_)) = packet {
                // Send default client settings expected during configuration, then finish configuration.
                let settings = Packet::Known(packet::Packet::ConfigurationServerboundSettings(
                    Box::new(packet::configuration::serverbound::Settings {
                        locale: "en_us".to_string(),
                        viewDistance: 12,
                        chatFlags: VarInt(0),
                        chatColors: true,
                        skinParts: 0x7F,
                        mainHand: VarInt(1), // 0=left,1=right
                        enableTextFiltering: false,
                        enableServerListing: true,
                        particleStatus: packet::SettingsParticlestatus::All,
                    }),
                ));
                packet_writer.send(settings);

                config_state.started = true;
                break;
            }

            if let Packet::Known(packet::Packet::ConfigurationClientboundFinishConfiguration(_)) =
                packet
            {
                if config_state.started {
                    let finish =
                        Packet::Known(packet::Packet::ConfigurationServerboundFinishConfiguration(
                            Box::new(packet::configuration::serverbound::FinishConfiguration {}),
                        ));
                    packet_writer.send(finish);
                    config_state.started = false;

                    // Acknowledge the transition back to Play and send play-state settings as well.
                    let acknowledged =
                        Packet::Known(packet::Packet::PlayServerboundConfigurationAcknowledged(
                            Box::new(packet::play::serverbound::ConfigurationAcknowledged {}),
                        ));
                    packet_writer.send(acknowledged);
                    send_play_settings(&mut packet_writer);
                }
                break;
            }
        }
    }

    fn respond_to_position_packets(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
    ) {
        for packet in packet_reader.iter() {
            match packet {
                Packet::Known(packet::Packet::PlayClientboundPosition(pos)) => {
                    let confirm = Packet::Known(packet::Packet::PlayServerboundTeleportConfirm(
                        Box::new(packet::play::serverbound::TeleportConfirm {
                            teleportId: pos.teleportId,
                        }),
                    ));
                    packet_writer.send(confirm);

                    // Echo the server's suggested position and angles to finish the teleport.
                    let movement = Packet::Known(packet::Packet::PlayServerboundPositionLook(
                        Box::new(packet::play::serverbound::PositionLook {
                            x: pos.x,
                            y: pos.y,
                            z: pos.z,
                            yaw: pos.yaw,
                            pitch: pos.pitch,
                            flags: 0,
                        }),
                    ));
                    packet_writer.send(movement);
                }
                _ => {}
            }
        }
    }

    fn respond_to_chunk_batch_packets(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
    ) {
        let mut saw_batch_start = false;

        for packet in packet_reader.iter() {
            match packet {
                Packet::Known(packet::Packet::PlayClientboundChunkBatchStart(_)) => {
                    saw_batch_start = true;
                }
                Packet::Known(packet::Packet::PlayClientboundChunkBatchFinished(_)) => {
                    // Acknowledge the batch; pick a sane chunks-per-tick budget.
                    let ack =
                        Packet::Known(packet::Packet::PlayServerboundChunkBatchReceived(Box::new(
                            packet::play::serverbound::ChunkBatchReceived { chunksPerTick: 5.0 },
                        )));
                    packet_writer.send(ack);
                    saw_batch_start = false;
                }
                _ => {}
            }
        }

        if saw_batch_start {
            // If we saw a start but no finish yet, still keep the reader drained.
        }
    }

    fn respond_to_keep_alive_packets(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut packet_writer: CodecWriter<ProtocolCodec>,
    ) {
        for packet in packet_reader.iter() {
            let response = match packet {
                Packet::Known(packet::Packet::ConfigurationClientboundKeepAlive(keep_alive)) => {
                    Packet::Known(packet::Packet::ConfigurationServerboundKeepAlive(Box::new(
                        packet::configuration::serverbound::KeepAlive {
                            keepAliveId: keep_alive.keepAliveId,
                        },
                    )))
                }
                Packet::Known(packet::Packet::ConfigurationClientboundPing(ping)) => {
                    Packet::Known(packet::Packet::ConfigurationServerboundPong(Box::new(
                        packet::configuration::serverbound::Pong { id: ping.id },
                    )))
                }
                Packet::Known(packet::Packet::PlayClientboundKeepAlive(keep_alive)) => {
                    Packet::Known(packet::Packet::PlayServerboundKeepAlive(Box::new(
                        packet::play::serverbound::KeepAlive {
                            keepAliveId: keep_alive.keepAliveId,
                        },
                    )))
                }

                _ => continue,
            };

            debug!("KeepAlive");
            packet_writer.send(response);
            break;
        }
    }

    fn handle_disconnect(
        mut packet_reader: CodecReader<ProtocolCodec>,
        mut disconnect_events: MessageWriter<Disconnect>,
    ) {
        for packet in packet_reader.iter() {
            match packet {
                Packet::Known(packet::Packet::PlayClientboundKickDisconnect(disconnect)) => {
                    let reason = format!("{:?}", disconnect.reason);
                    disconnect_events.write(Disconnect { reason });
                }
                Packet::Known(packet::Packet::ConfigurationClientboundDisconnect(disconnect)) => {
                    let reason = format!("{:?}", disconnect.reason);
                    disconnect_events.write(Disconnect { reason });
                }
                _ => {}
            }
        }
    }
}
