//! System params exposed by this crate.

use std::marker::PhantomData;

use bevy::{ecs::system::SystemParam, prelude::*};

/// Newtype around some packet type `T` from some codec `U`.
///
/// Exists in tandem with [`Write<T, U>`] to ensure that there are two distinct
/// event channels for codec packets, even if `<Codec as Decode>::Item` and
/// `<Codec as Encode>::Item` are the same type, or if two codecs use the same
/// packet type.
///
/// Users of this crate should never have to interact with this type or even
/// understand that it exists.
pub struct Read<T, U>(pub(crate) T, pub(crate) PhantomData<U>);

impl<T, U> Message for Read<T, U>
where
    T: Send + Sync + 'static,
    U: Send + Sync + 'static,
{
}

/// A Bevy system param similar to [`EventReader`] that reads network packets.
///
/// For convenience, you probably want to use the
/// [`CodecReader`][crate::CodecReader] type alias in the crate root.
#[derive(SystemParam)]
pub struct CodecReader<'w, 's, Packet: Send + Sync + 'static, Codec: Resource> {
    event_reader: MessageReader<'w, 's, Read<Packet, Codec>>,
}

impl<'w, 's, Packet: Send + Sync + 'static, Codec: Resource> CodecReader<'w, 's, Packet, Codec> {
    /// Iterates over the packets this [`CodecReader`] has not seen yet. This
    /// updates the [`CodecReader`]'s event counter, which means subsequent
    /// packet reads will not include packets that happened before now.
    pub fn iter(&mut self) -> impl Iterator<Item = &Packet> {
        self.event_reader.read().map(|event| &event.0)
    }
}

/// Newtype around some packet type `T` from some codec `U`.
///
/// Exists in tandem with [`Read<T, U>`] to ensure that there are two distinct
/// event channels for codec packets, even if `<Codec as Decode>::Item` and
/// `<Codec as Encode>::Item` are the same type, or if two codecs use the same
/// packet type.
///
/// Users of this crate should never have to interact with this type or even
/// understand that it exists.
pub struct Write<T, U>(pub(crate) T, pub(crate) PhantomData<U>);

impl<T, U> Message for Write<T, U>
where
    T: Send + Sync + 'static,
    U: Send + Sync + 'static,
{
}

/// A Bevy system param similar to [`EventWriter`] that writes network packets.
///
/// For convenience, you probably want to use the
/// [`CodecWriter`][crate::CodecWriter] type alias in the crate root.
#[derive(SystemParam)]
pub struct CodecWriter<'w, Packet: Send + Sync + 'static, Codec: Resource> {
    event_writer: MessageWriter<'w, Write<Packet, Codec>>,
}

impl<'w, Packet: Send + Sync + 'static, Codec: Resource> CodecWriter<'w, Packet, Codec> {
    pub fn send(&mut self, packet: Packet) {
        self.event_writer.write(Write(packet, PhantomData));
    }
}
