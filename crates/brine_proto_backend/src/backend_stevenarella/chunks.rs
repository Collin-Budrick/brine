use bevy::prelude::*;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::Cursor;

use brine_chunk::{
    decode::{Result, VarIntRead},
    palette::SectionPalette,
    BlockState, Chunk, Palette, SECTIONS_PER_CHUNK,
};
use brine_net::CodecReader;
use brine_proto::event;

use super::codec::{packet, Packet, ProtocolCodec};

/// A dummy palette for testing that performs no translation.
pub struct DummyPalette;

impl Palette for DummyPalette {
    fn id_to_block_state(&self, id: u32) -> Option<brine_chunk::BlockState> {
        Some(BlockState(id))
    }
}

/// Common representation of the different versions of ChunkData packets.
pub struct ChunkData<T> {
    pub chunk_x: i32,
    pub chunk_z: i32,
    pub full_chunk: bool,
    pub bitmask: u32,
    pub data: T,
}

impl<'d> ChunkData<&'d [u8]> {
    pub fn from_packet(packet: &'d Packet) -> Option<Self> {
        match packet {
            Packet::Known(packet::Packet::PlayClientboundMapChunk(map_chunk)) => {
                let chunk_bytes = map_chunk.chunkData.data.as_slice();

                let bitmask = match compute_section_bitmask(chunk_bytes) {
                    Ok(mask) => mask,
                    Err(err) => {
                        warn!("Failed to parse chunk data bitmask: {}", err);
                        return None;
                    }
                };

                debug!(
                    "MapChunk ({}, {}): {} bytes, {} sections",
                    map_chunk.x,
                    map_chunk.z,
                    chunk_bytes.len(),
                    bitmask.count_ones()
                );

                Some(Self {
                    chunk_x: map_chunk.x,
                    chunk_z: map_chunk.z,
                    full_chunk: true,
                    bitmask,
                    data: chunk_bytes,
                })
            }
            _ => None,
        }
    }
}

impl<T: AsRef<[u8]>> ChunkData<T> {
    pub fn decode(&self) -> Result<Chunk> {
        let mut buf = self.data.as_ref();
        Chunk::decode(
            self.chunk_x,
            self.chunk_z,
            self.full_chunk,
            self.bitmask,
            &DummyPalette,
            &mut buf,
        )
    }
}

pub fn get_chunk_from_packet(packet: &Packet) -> Result<Option<Chunk>> {
    if let Some(chunk_data) = ChunkData::from_packet(packet) {
        Ok(Some(chunk_data.decode()?))
    } else {
        Ok(None)
    }
}

pub(crate) fn build(app: &mut App) {
    app.add_systems(Update, handle_chunk_data);
}

/// System that listens for ChunkData packets and sends ChunkData events to the
/// client application.
fn handle_chunk_data(
    mut packet_reader: CodecReader<ProtocolCodec>,
    mut chunk_events: MessageWriter<event::clientbound::ChunkData>,
) {
    for packet in packet_reader.iter() {
        match get_chunk_from_packet(packet) {
            Ok(Some(chunk_data)) => {
                trace!("Chunk: {:?}", chunk_data);
                chunk_events.write(event::clientbound::ChunkData { chunk_data });
            }
            Err(e) => error!("{}", e),
            _ => {}
        }
    }
}

fn compute_section_bitmask(chunk_bytes: &[u8]) -> Result<u32> {
    let mut cursor = Cursor::new(chunk_bytes);
    let mut bitmask: u32 = 0;
    let mut section_index: u32 = 0;

    while (cursor.position() as usize) < chunk_bytes.len() {
        let start = cursor.position();

        cursor.read_i16::<BigEndian>()?;

        let mut bits_per_block = cursor.read_u8()?;
        if bits_per_block < 4 {
            bits_per_block = 4;
        }

        if bits_per_block <= SectionPalette::MAX_BITS_PER_BLOCK {
            let palette_len = cursor.read_var_i32()?;
            for _ in 0..palette_len {
                cursor.read_var_i32()?;
            }
        }

        let array_length = cursor.read_var_i32()?;
        for _ in 0..array_length {
            cursor.read_u64::<BigEndian>()?;
        }

        let consumed = cursor.position() - start;
        if consumed == 0 {
            break;
        }

        if section_index < 32 {
            bitmask |= 1 << section_index;
        }
        section_index += 1;

        if section_index >= SECTIONS_PER_CHUNK as u32 {
            break;
        }
    }

    let remaining = chunk_bytes.len().saturating_sub(cursor.position() as usize);
    if remaining > 0 {
        warn!(
            "Chunk data had {} trailing bytes after parsing {} sections",
            remaining, section_index
        );
    }

    Ok(bitmask)
}
