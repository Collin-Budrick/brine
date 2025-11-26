use bevy::prelude::*;

use brine_net::{codec::DummyCodec, NetworkEvent, NetworkPlugin, NetworkResource};

const SERVER: &str = "google.com:80";

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_plugins(NetworkPlugin::<DummyCodec>::default())
        .add_systems(Startup, connect)
        .add_systems(Update, read_net_events)
        .run();
}

fn connect(mut net_resource: ResMut<NetworkResource<DummyCodec>>) {
    net_resource.connect(SERVER.to_string());
}

fn read_net_events(mut reader: MessageReader<NetworkEvent<DummyCodec>>) {
    for event in reader.read() {
        println!("NetworkEvent: {:?}", event);
    }
}
