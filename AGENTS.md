# Brine Agent Handbook

## What this project is
- WIP Minecraft client written in Rust on top of Bevy 0.17 (`bevy`, `bevy_mesh`, `bevy_flycam`, `bevy-inspector-egui`).
- Goal: protocol-abstracted client; core Minecraft logic is expressed in `brine_proto`, while concrete networking is in `brine_proto_backend` (currently Java Edition via stevenarella’s `steven_protocol`).
- Rendering uses voxel mesh builders in `brine_voxel_v1`; assets and block data come from `brine_asset` and `brine_data`.
- Default target version: Minecraft 1.21.4 (protocol version auto-negotiated during status ping).

## Workspace map (key crates)
- Root `brine` binary (`src/main.rs`): wires Bevy plugins, loads assets from `assets/1.21.4`, connects to server or serves local chunk files.
- `crates/brine_proto`: defines protocol-agnostic clientbound/serverbound event types + `ProtocolPlugin`.
- `crates/brine_proto_backend`: stevenarella-backed codec + login/play state machines + chunk decoding; exposes `ProtocolBackendPlugin`.
- `crates/brine_chunk`: chunk data types + decoding (currently 1.21.4).
- `crates/brine_voxel_v1`: chunk builders (VisibleFaces default, GreedyQuads optional, NaiveBlocks debug) that turn `ChunkData` events into renderable meshes.
- `crates/brine_asset`: loads Minecraft assets/resource packs using `minecraft-assets` API.
- `crates/brine_data`: baked Minecraft data from `minecraft-data-rs`.
- `crates/brine_net`: thin Bevy TCP protocol helper used by backend codec.
- `crates/brine_render`: texture + chunk baking utilities (meshing view helpers).
- Tools: `xtask` automation (assets + minecraft-data fetch + protocol generation); `src/bin/chunktool` (print/save/view chunk dumps).

## Runtime data you must have
- Vanilla assets for the target version: `assets/1.21.4/{assets,data,pack.mcmeta}`. Fetch with `cargo xtask fetch-assets --version 1.21.4` (use `--force` to refresh).
- PrismarineJS `minecraft-data` checkout inside `third_party/minecraft-data-rs/minecraft-data`. Refresh with `cargo xtask fetch-minecraft-data --reference master`.
- One-shot setup (does both): `cargo xtask setup --version 1.21.4 --reference master`.

## Building and running
- Rust 1.70+ recommended (edition 2021). Bevy is built with `dynamic_linking`; ensure graphics deps for WGPU are available.
- Default log filter: `wgpu_core=warn,naga=warn`; raise verbosity with `RUST_LOG=info` or `RUST_LOG=trace,brine_proto_backend::backend_stevenarella::chunks=trace`.
- Run the client against a server:  
  `cargo run --release -- --server host:port --username user123`
- Run with built-in fake server that replays chunk dumps:  
  `cargo run --release -- --chunk_dir path/to/chunk_dumps/`
- Enable debug helpers (wireframe, inspector, frame diagnostics, polygon-line mode): add `--debug`.
- Utility binaries:
  - `cargo run --bin chunktool -- print <chunk.dump>` (inspect), `save` (capture packets to dumps), `view` (render chunks with chosen builder).
  - `cargo run --bin rust_out.exe` appears to be legacy; primary entry is `brine`.

## Networking/login flow (important behaviors)
- Two-phase login: status ping discovers server protocol version, then reconnect for login (`Login` event triggers connect).
- Configuration phase is acknowledged; client sends `ConfigurationServerboundSettings`, echoes `SelectKnownPacks`, then `ConfigurationServerboundFinishConfiguration` and play-state settings.
- Keep-alives (configuration + play) and pings are auto-responded.
- Position packets trigger teleport confirm + echo position to finish teleport.
- Chunk batches: on `PlayClientboundChunkBatchFinished`, client acknowledges with `ChunkBatchReceived { chunksPerTick: 5.0 }`.
- Chunk data packets are decoded to `brine_proto::event::clientbound::ChunkData` and fed into `ChunkBuilderPlugin` for meshing.

## Rendering pipeline (high level)
- `ChunkBuilderPlugin::<VisibleFacesChunkBuilder>` listens for `ChunkData` events, spawns tasks to mesh chunks, then spawns `BuiltChunkSection` entities positioned by section Y.
- Camera is a fly-cam; startup transform is set in `set_up_camera` (see `src/main.rs`).
- Wireframe toggle: `EnableWireframe` component (spawned at startup) controls global wireframe when debug flag used.

## Logs and where to look
- Console stdout/stderr (or redirect to `client-run.log` / `client-run.err`).
- Chunk receipt traces live in `brine_proto_backend::backend_stevenarella::chunks` at TRACE level (`trace!("Chunk: {:?}", chunk_data);`).
- Network errors surface via `NetworkEvent::Error` log in `ProtocolBackendPlugin`.
- Disconnect reasons are logged and, when `LoginPlugin::exit_on_disconnect()` is used (default), will exit the app.

## How to test (AI-run pipeline)
Goal: launch the game, run 20 seconds, auto-close, then confirm the client cleanly reaches and stays in Play state; if not, troubleshoot until it does.

1) Prepare run command (captures logs):  
   PowerShell example from repo root:  
   ```
   $env:RUST_LOG="info,brine_proto_backend::backend_stevenarella::login=debug"
   $args = "run --release -- --server localhost:25565 --username user"
   $proc = Start-Process cargo -ArgumentList $args -PassThru -NoNewWindow `
     -RedirectStandardOutput "client-run.log" -RedirectStandardError "client-run.err"
   Start-Sleep -Seconds 20
   Stop-Process -Id $proc.Id -Force
   ```
   (Use `--chunk_dir <dir>` instead of `--server ...` when testing offline with recorded data.)

2) Inspect logs to ensure the session entered Play and stayed healthy:  
   ```
   Get-Content client-run.err | Select-String "Login successful"
   Get-Content client-run.err | Select-String "Configuration.*Finish"
   Get-Content client-run.err | Select-String "KeepAlive"
   Get-Content client-run.err | Select-String "Network error"
   ```
   Expected: `Login successful` and configuration-finish lines, no disconnects or network errors.

3) If healthy Play-state evidence is missing, troubleshoot until it appears:  
   - Verify assets exist at `assets/1.21.4` and are readable.  
   - Confirm the target server is reachable and accepts the login (or use `--chunk_dir` for offline smoke tests).  
   - Raise logging (`$env:RUST_LOG="trace"`) and re-run to see protocol state transitions.  
   - Look for disconnect reasons in `client-run.err`; fix network/auth issues or server address/port.  
   - After any fix, repeat step 1 for another 20-second run and re-check logs.

4) Success criteria: logs show `Login successful` and configuration completion, no errors/disconnects during the 20-second window. Document findings in the task at hand.

## Quick reference commands
- Setup data: `cargo xtask setup --version 1.21.4 --reference master`
- Run client (release): `cargo run --release -- --server localhost:25565 --username user`
- Run with fake chunks: `cargo run --release -- --chunk_dir .\\chunks\\`
- Generate protocol tables: `cargo xtask generate-protocol --version 1.21.4`
- Chunk viewer: `cargo run --bin chunktool -- view ./path/to/chunk.dump`

Keep this file updated when behaviors or required assets change.***
