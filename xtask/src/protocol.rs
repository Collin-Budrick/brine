use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct PacketIndex {
    pub minecraft_version: String,
    pub protocol_version: i32,
    pub states: Vec<StatePackets>,
}

#[derive(Debug, Serialize)]
pub struct StatePackets {
    pub state: String,
    pub directions: Vec<DirectionPackets>,
}

#[derive(Debug, Serialize)]
pub struct DirectionPackets {
    pub direction: DirectionKind,
    pub packets: Vec<PacketSummary>,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
pub enum DirectionKind {
    Clientbound,
    Serverbound,
}

#[derive(Debug, Serialize)]
pub struct PacketSummary {
    pub id: i32,
    pub name: String,
    pub r#type: String,
    pub rust_struct: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<PacketField>,
}

#[derive(Debug, Serialize)]
pub struct PacketField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: Value,
}

#[derive(Default)]
struct HelperCollector {
    containers: BTreeMap<String, ContainerHelper>,
    enums: BTreeMap<String, EnumHelper>,
    mappers: BTreeMap<String, MapperHelper>,
}

#[derive(Clone)]
struct ContainerHelper {
    name: String,
    fields: Vec<ContainerFieldDef>,
}

#[derive(Clone)]
struct ContainerFieldDef {
    name: String,
    ty: Value,
}

#[derive(Clone)]
struct EnumHelper {
    name: String,
    tag_type: String,
    variants: Vec<EnumVariant>,
}

#[derive(Clone)]
struct EnumVariant {
    tag: i32,
    name: String,
    ty: String,
}

#[derive(Clone)]
struct MapperHelper {
    tag_type: String,
    mappings: Vec<MapperEntry>,
}

#[derive(Clone)]
struct MapperEntry {
    tag: i32,
    name: String,
}

impl HelperCollector {
    fn register_container(&mut self, value: &Value, hint: &str) -> String {
        let key = value.to_string();
        if let Some(existing) = self.containers.get(&key) {
            return existing.name.clone();
        }
        let name = format!(
            "{}Container{}",
            to_pascal_case(hint),
            self.containers.len() + 1
        );
        let Some(fields) = value.get(1).and_then(|v| v.as_array()) else {
            return "Vec<u8>".to_string();
        };
        let mut parsed = Vec::new();
        for raw in fields {
            let Some(name_raw) = raw.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(ty) = raw.get("type") else {
                continue;
            };
            parsed.push(ContainerFieldDef {
                name: name_raw.to_string(),
                ty: ty.clone(),
            });
        }
        self.containers.insert(
            key,
            ContainerHelper {
                name: name.clone(),
                fields: parsed,
            },
        );
        name
    }

    fn mapper_key(owner: &str, field: &str) -> String {
        format!("{owner}::{field}")
    }

    fn register_mapper(
        &mut self,
        owner: &str,
        field_name: Option<&str>,
        spec: &MapperSpec,
    ) -> String {
        let tag_type = map_mapper_tag_type(spec);
        if let Some(field) = field_name {
            let key = Self::mapper_key(owner, field);
            self.mappers.entry(key).or_insert_with(|| MapperHelper {
                tag_type: tag_type.clone(),
                mappings: spec
                    .mappings
                    .iter()
                    .map(|(tag, name)| MapperEntry {
                        tag: *tag,
                        name: name.clone(),
                    })
                    .collect(),
            });
        }
        let name = type_name(owner, field_name);
        let variants = spec
            .mappings
            .iter()
            .map(|(tag, name)| EnumVariant {
                tag: *tag,
                name: to_pascal_case(name),
                ty: "()".to_string(),
            })
            .collect();
        self.enums.entry(name.clone()).or_insert(EnumHelper {
            name: name.clone(),
            tag_type,
            variants,
        });
        name
    }

    fn register_switch(
        &mut self,
        value: &Value,
        owner: &str,
        field_name: Option<&str>,
        spec: &SwitchSpec,
    ) -> String {
        let Some(field_name) = field_name else {
            return "Vec<u8>".to_string();
        };
        let key = Self::mapper_key(owner, &spec.compare_to);
        let Some(mapper) = self.mappers.get(&key).cloned() else {
            return "Vec<u8>".to_string();
        };
        let enum_name = type_name(owner, Some(field_name));
        let Some(fields) = value
            .get(1)
            .and_then(|v| v.get("fields"))
            .and_then(|v| v.as_object())
        else {
            return "Vec<u8>".to_string();
        };
        let mut variants = Vec::new();
        for mapping in &mapper.mappings {
            let Some(target_value) = fields.get(&mapping.name) else {
                continue;
            };
            let variant_ty = map_type(target_value, self, owner, Some(&mapping.name));
            variants.push(EnumVariant {
                tag: mapping.tag,
                name: to_pascal_case(&mapping.name),
                ty: variant_ty,
            });
        }
        self.enums.insert(
            enum_name.clone(),
            EnumHelper {
                name: enum_name.clone(),
                tag_type: mapper.tag_type,
                variants,
            },
        );
        enum_name
    }

    fn render(&mut self, output: &mut String) -> Result<()> {
        let keys: Vec<String> = self.containers.keys().cloned().collect();
        for key in keys {
            let Some(helper) = self.containers.get(&key).cloned() else {
                continue;
            };
            writeln!(output, "#[derive(Default, Debug, Clone, PartialEq)]")?;
            writeln!(output, "pub struct {} {{", helper.name)?;
            for field in &helper.fields {
                let ty = map_type(&field.ty, self, &helper.name, Some(&field.name));
                writeln!(output, "    pub {}: {},", field.name, ty)?;
            }
            writeln!(output, "}}")?;
            writeln!(output)?;
            writeln!(output, "impl Serializable for {} {{", helper.name)?;
            writeln!(
                output,
                "    fn read_from<R: io::Read>(buf: &mut R) -> Result<Self, Error> {{"
            )?;
            writeln!(output, "        Ok(Self {{")?;
            for field in &helper.fields {
                writeln!(
                    output,
                    "            {}: {}::read_from(buf)?,",
                    field.name,
                    map_type(&field.ty, self, &helper.name, Some(&field.name))
                )?;
            }
            writeln!(output, "        }})")?;
            writeln!(output, "    }}")?;
            writeln!(
                output,
                "    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), Error> {{"
            )?;
            for field in &helper.fields {
                writeln!(output, "        self.{}.write_to(buf)?;", field.name)?;
            }
            writeln!(output, "        Ok(())")?;
            writeln!(output, "    }}")?;
            writeln!(output, "}}")?;
            writeln!(output)?;
        }
        let enum_keys: Vec<String> = self.enums.keys().cloned().collect();
        for key in enum_keys {
            let Some(helper) = self.enums.get(&key).cloned() else {
                continue;
            };
            writeln!(output, "#[derive(Debug, Clone, PartialEq)]")?;
            writeln!(output, "pub enum {} {{", helper.name)?;
            for variant in &helper.variants {
                if variant.ty == "()" {
                    writeln!(output, "    {},", variant.name)?;
                } else {
                    writeln!(output, "    {}({}),", variant.name, variant.ty)?;
                }
            }
            writeln!(output, "}}")?;
            writeln!(output)?;

            writeln!(output, "impl Serializable for {} {{", helper.name)?;
            writeln!(
                output,
                "    fn read_from<R: io::Read>(buf: &mut R) -> Result<Self, Error> {{"
            )?;
            writeln!(
                output,
                "        let tag = {}::read_from(buf)?;",
                helper.tag_type
            )?;
            writeln!(output, "        let tag_value: i64 = match tag {{")?;
            writeln!(output, "            VarInt(v) => v as i64,")?;
            writeln!(output, "            VarLong(v) => v,")?;
            writeln!(output, "            value => value as i64,")?;
            writeln!(output, "        };")?;
            writeln!(output, "        match tag_value {{")?;
            for variant in &helper.variants {
                if variant.ty == "()" {
                    writeln!(
                        output,
                        "            {tag} => Ok(Self::{name}),",
                        tag = variant.tag,
                        name = variant.name
                    )?;
                } else {
                    writeln!(
                        output,
                        "            {tag} => Ok(Self::{name}({ty}::read_from(buf)?)),",
                        tag = variant.tag,
                        name = variant.name,
                        ty = variant.ty
                    )?;
                }
            }
            writeln!(
                output,
                "            other => Err(io::Error::new(io::ErrorKind::InvalidData, format!(\"unknown {name} tag {{{{}}}}\", other)).into()),",
                name = helper.name
            )?;
            writeln!(output, "        }}")?;
            writeln!(output, "    }}")?;
            writeln!(
                output,
                "    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), Error> {{"
            )?;
            writeln!(output, "        match self {{")?;
            for variant in &helper.variants {
                if variant.ty == "()" {
                    writeln!(
                        output,
                        "            Self::{name} => {{ {tag}.write_to(buf)?; }},",
                        name = variant.name,
                        tag = render_tag_value(&helper.tag_type, variant.tag)
                    )?;
                } else {
                    writeln!(
                        output,
                        "            Self::{name}(value) => {{ {tag}.write_to(buf)?; value.write_to(buf)?; }},",
                        name = variant.name,
                        tag = render_tag_value(&helper.tag_type, variant.tag)
                    )?;
                }
            }
            writeln!(output, "        }")?;
            writeln!(output, "        Ok(())")?;
            writeln!(output, "    }}")?;
            writeln!(output, "}}")?;
            writeln!(output)?;
        }
        Ok(())
    }
}

const STATE_KEYS: &[&str] = &["handshaking", "status", "login", "configuration", "play"];

pub fn build_packet_index(
    proto_path: &Path,
    minecraft_version: &str,
    protocol_version: i32,
) -> Result<PacketIndex> {
    let contents = fs::read_to_string(proto_path)
        .with_context(|| format!("failed to read {}", proto_path.display()))?;
    let value: Value = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", proto_path.display()))?;
    let global_types_value = value
        .get("types")
        .and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("protocol.json missing top-level types map"))?;
    let mut states = Vec::new();
    for key in STATE_KEYS {
        let Some(state_value) = value.get(*key) else {
            continue;
        };
        states.push(parse_state(key, state_value, global_types_value)?);
    }

    Ok(PacketIndex {
        minecraft_version: minecraft_version.to_string(),
        protocol_version,
        states,
    })
}

pub fn write_version_table(index: &PacketIndex, out_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)?;
    let module_name = version_module_name(&index.minecraft_version);
    let file_path = out_dir.join(format!("{module_name}.rs"));
    let mut output = String::new();
    writeln!(
        &mut output,
        "// @generated by xtask::generate-protocol for Minecraft {}, protocol {}",
        index.minecraft_version, index.protocol_version
    )?;
    writeln!(&mut output, "// Do not edit by hand.")?;
    writeln!(&mut output)?;
    writeln!(&mut output, "protocol_packet_ids!(")?;
    for state in &index.states {
        let state_ident = state_rust_name(&state.state);
        let state_label = state_macro_label(&state.state);
        writeln!(
            &mut output,
            "    {state_label} {state_ident} {{",
            state_label = state_label,
            state_ident = state_ident
        )?;
        for direction in &state.directions {
            let dir_ident = direction_rust_name(direction.direction);
            writeln!(
                &mut output,
                "        {dir_label} {dir_ident} {{",
                dir_label = direction_label(direction.direction),
                dir_ident = dir_ident
            )?;
            for packet in &direction.packets {
                writeln!(
                    &mut output,
                    "            {id} => {name}",
                    id = format_packet_id(packet.id),
                    name = packet.rust_struct
                )?;
            }
            writeln!(&mut output, "        }}")?;
        }
        writeln!(&mut output, "    }}")?;
    }
    writeln!(&mut output, ");")?;
    fs::write(&file_path, output)?;
    Ok(file_path)
}

pub fn write_state_packets_stub(index: &PacketIndex, out_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(out_dir)?;
    let file_path = out_dir.join("packet.rs");
    let mut output = String::new();
    let mut helpers = HelperCollector::default();
    writeln!(
        &mut output,
        "// @generated by xtask::generate-protocol for Minecraft {}, protocol {}",
        index.minecraft_version, index.protocol_version
    )?;
    writeln!(
        &mut output,
        "// Sketch of state_packets! for codegen; not meant to compile as-is."
    )?;
    writeln!(
        &mut output,
        "// Contains helper newtypes so the packets can compile in isolation."
    )?;
    writeln!(&mut output)?;
    output.push_str(HELPERS_PRELUDE);
    helpers.render(&mut output)?;
    writeln!(&mut output)?;
    writeln!(&mut output, "state_packets!(")?;
    for state in &index.states {
        let state_ident = state_rust_name(&state.state);
        let state_label = state_macro_label(&state.state);
        writeln!(
            &mut output,
            "    {state_label} {state_ident} {{",
            state_label = state_label,
            state_ident = state_ident
        )?;
        for direction in &state.directions {
            let dir_ident = direction_rust_name(direction.direction);
            writeln!(
                &mut output,
                "        {dir_label} {dir_ident} {{",
                dir_label = direction_label(direction.direction),
                dir_ident = dir_ident
            )?;
            for packet in &direction.packets {
                writeln!(&mut output, "            packet {} {{", packet.rust_struct)?;
                if packet.fields.is_empty() {
                    writeln!(&mut output, "            }}")?;
                    continue;
                }
                for field in &packet.fields {
                    let type_hint =
                        format!("{}{}", packet.rust_struct, to_pascal_case(&field.name));
                    let ty = map_type(
                        &field.ty,
                        &mut helpers,
                        &packet.rust_struct,
                        Some(&field.name),
                    );
                    writeln!(
                        &mut output,
                        "                field {}: {} =,",
                        field.name, ty
                    )?;
                }
                writeln!(&mut output, "            }}")?;
            }
            writeln!(&mut output, "        }}")?;
        }
        writeln!(&mut output, "    }}")?;
    }
    writeln!(&mut output, ");")?;
    fs::write(&file_path, output)?;
    Ok(file_path)
}

fn parse_state(
    state: &str,
    value: &Value,
    global_types: &serde_json::Map<String, Value>,
) -> Result<StatePackets> {
    let mut directions = Vec::new();
    if let Some(to_server) = value.get("toServer") {
        directions.push(DirectionPackets {
            direction: DirectionKind::Serverbound,
            packets: parse_direction(to_server, global_types)?,
        });
    }
    if let Some(to_client) = value.get("toClient") {
        directions.push(DirectionPackets {
            direction: DirectionKind::Clientbound,
            packets: parse_direction(to_client, global_types)?,
        });
    }

    Ok(StatePackets {
        state: state.to_string(),
        directions,
    })
}

fn parse_direction(
    value: &Value,
    global_types: &serde_json::Map<String, Value>,
) -> Result<Vec<PacketSummary>> {
    let Some(types) = value.get("types") else {
        return Ok(Vec::new());
    };
    let Some(types_map) = types.as_object() else {
        bail!("types map must be an object");
    };
    let packet_type = types_map
        .get("packet")
        .or_else(|| global_types.get("packet"))
        .ok_or_else(|| anyhow!("missing packet base type definition"))?;
    let packet_expr = TypeExpr::parse(packet_type)?;
    let TypeExpr::Container(fields) = packet_expr else {
        bail!("packet definition must be a container");
    };

    let mapper = fields
        .iter()
        .find(|f| f.name == "name")
        .context("packet container missing name mapper")?;
    let params = fields
        .iter()
        .find(|f| f.name == "params")
        .context("packet container missing params switch")?;
    let TypeExpr::Mapper(mapper_spec) = &mapper.ty else {
        bail!("packet name field must be a mapper");
    };
    let TypeExpr::Switch(switch_spec) = &params.ty else {
        bail!("packet params field must be a switch");
    };

    let mut out = Vec::with_capacity(mapper_spec.mappings.len());
    for (id, name) in &mapper_spec.mappings {
        let Some(target) = switch_spec.fields.get(name) else {
            bail!("switch spec missing entry for packet {name}");
        };
        let r#type = match target {
            TypeExpr::Named(n) => n.clone(),
            other => {
                bail!(
                    "packet {name} must resolve to a named type, found {:?}",
                    other.kind()
                );
            }
        };
        let fields = if is_payloadless_packet(&r#type) {
            Vec::new()
        } else {
            extract_packet_fields(types_map, global_types, &r#type)
                .with_context(|| format!("failed to parse packet {name} fields"))?
        };
        out.push(PacketSummary {
            id: *id,
            name: name.clone(),
            r#type,
            rust_struct: to_pascal_case(name),
            fields,
        });
    }
    out.sort_by_key(|p| p.id);
    Ok(out)
}

fn to_pascal_case(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut out = String::new();
                    out.extend(first.to_uppercase());
                    out.extend(chars.flat_map(|c| c.to_lowercase()));
                    out
                }
                None => String::new(),
            }
        })
        .collect()
}

#[derive(Debug)]
struct ContainerField {
    name: String,
    ty: TypeExpr,
}

#[derive(Debug)]
struct MapperSpec {
    tag_type: Option<String>,
    mappings: Vec<(i32, String)>,
}

#[derive(Debug)]
struct SwitchSpec {
    #[allow(dead_code)]
    compare_to: String,
    fields: BTreeMap<String, TypeExpr>,
    #[allow(dead_code)]
    default: Option<Box<TypeExpr>>,
}

#[derive(Debug)]
enum TypeExpr {
    Named(String),
    Container(Vec<ContainerField>),
    Mapper(MapperSpec),
    Switch(SwitchSpec),
    Unsupported,
}

impl TypeExpr {
    fn kind(&self) -> &'static str {
        match self {
            TypeExpr::Named(_) => "named",
            TypeExpr::Container(_) => "container",
            TypeExpr::Mapper(_) => "mapper",
            TypeExpr::Switch(_) => "switch",
            TypeExpr::Unsupported => "unsupported",
        }
    }

    fn parse(value: &Value) -> Result<Self> {
        match value {
            Value::String(name) => Ok(TypeExpr::Named(name.clone())),
            Value::Array(items) => {
                let Some(kind) = items.get(0).and_then(|v| v.as_str()) else {
                    bail!("type array missing discriminator");
                };
                match kind {
                    "container" => parse_container(items.get(1)),
                    "mapper" => parse_mapper(items.get(1)),
                    "switch" => parse_switch(items.get(1)),
                    _ => Ok(TypeExpr::Unsupported),
                }
            }
            other => bail!("unsupported type expression: {other:?}"),
        }
    }
}

fn parse_container(value: Option<&Value>) -> Result<TypeExpr> {
    let Some(fields) = value.and_then(|v| v.as_array()) else {
        bail!("container definition missing field array");
    };
    let mut out = Vec::with_capacity(fields.len());
    for raw in fields {
        let Some(name) = raw.get("name").and_then(|v| v.as_str()) else {
            bail!("container field missing name");
        };
        let Some(ty) = raw.get("type") else {
            bail!("container field {name} missing type");
        };
        out.push(ContainerField {
            name: name.to_string(),
            ty: TypeExpr::parse(ty)?,
        });
    }
    Ok(TypeExpr::Container(out))
}

fn parse_mapper(value: Option<&Value>) -> Result<TypeExpr> {
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        bail!("mapper definition missing object body");
    };
    let tag_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let Some(mappings_value) = obj.get("mappings").and_then(|v| v.as_object()) else {
        bail!("mapper definition missing mappings");
    };
    let mut entries = Vec::with_capacity(mappings_value.len());
    for (key, value) in mappings_value {
        let id = parse_packet_id(key)?;
        let Some(name) = value.as_str() else {
            bail!("mapper mapping target for {key} is not a string");
        };
        entries.push((id, name.to_string()));
    }
    entries.sort_by_key(|(id, _)| *id);
    Ok(TypeExpr::Mapper(MapperSpec {
        tag_type,
        mappings: entries,
    }))
}

fn parse_switch(value: Option<&Value>) -> Result<TypeExpr> {
    let Some(obj) = value.and_then(|v| v.as_object()) else {
        bail!("switch definition missing object body");
    };
    let compare_to = obj
        .get("compareTo")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("switch missing compareTo field"))?
        .to_string();
    let Some(fields_value) = obj.get("fields").and_then(|v| v.as_object()) else {
        bail!("switch missing fields map");
    };
    let mut fields = BTreeMap::new();
    for (key, value) in fields_value {
        fields.insert(key.clone(), TypeExpr::parse(value)?);
    }
    let default = if let Some(default_value) = obj.get("default") {
        Some(Box::new(TypeExpr::parse(default_value)?))
    } else {
        None
    };
    Ok(TypeExpr::Switch(SwitchSpec {
        compare_to,
        fields,
        default,
    }))
}

fn parse_packet_id(raw: &str) -> Result<i32> {
    if let Some(stripped) = raw.strip_prefix("0x") {
        i32::from_str_radix(stripped, 16).with_context(|| format!("invalid packet id {raw}"))
    } else {
        raw.parse::<i32>()
            .with_context(|| format!("invalid packet id {raw}"))
    }
}

fn is_payloadless_packet(name: &str) -> bool {
    name == "void"
}

fn state_rust_name(state: &str) -> &'static str {
    match state {
        "handshaking" => "Handshaking",
        "status" => "Status",
        "login" => "Login",
        "configuration" => "Configuration",
        "play" => "Play",
        other => panic!("unsupported state {other}"),
    }
}

fn state_macro_label(state: &str) -> &'static str {
    match state {
        "handshaking" => "handshake",
        "status" => "status",
        "login" => "login",
        "configuration" => "configuration",
        "play" => "play",
        other => panic!("unsupported state {other}"),
    }
}

fn direction_rust_name(direction: DirectionKind) -> &'static str {
    match direction {
        DirectionKind::Clientbound => "Clientbound",
        DirectionKind::Serverbound => "Serverbound",
    }
}

fn direction_label(direction: DirectionKind) -> &'static str {
    match direction {
        DirectionKind::Clientbound => "clientbound",
        DirectionKind::Serverbound => "serverbound",
    }
}

fn format_packet_id(id: i32) -> String {
    format!("{id:#04x}")
}

fn version_module_name(version: &str) -> String {
    let mut out = String::from("v");
    for ch in version.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if ch == '.' || ch == '-' {
            out.push('_');
        }
    }
    out
}

fn map_type(
    value: &Value,
    helpers: &mut HelperCollector,
    owner: &str,
    field_name: Option<&str>,
) -> String {
    if let Some(name) = value.as_str() {
        if let Some(mapped) = map_simple_type(name) {
            return mapped.to_string();
        }
        // Unknown named types default to raw bytes for now.
        return String::from("Vec<u8>");
    }
    if let Some(array) = value.as_array() {
        if let Some(kind) = array.get(0).and_then(|v| v.as_str()) {
            match kind {
                "array" => {
                    let count_ty = array.get(1).and_then(|v| v.get("countType"));
                    let inner = array.get(1).and_then(|v| v.get("type"));
                    let count_rust = count_ty
                        .and_then(|v| v.as_str())
                        .map(map_count_type)
                        .unwrap_or_else(|| "VarInt".to_string());
                    let inner_rust = inner
                        .map(|v| map_type(v, helpers, owner, field_name))
                        .unwrap_or_else(|| "Vec<u8>".to_string());
                    return format!("CountedArray<{inner_rust}, {count_rust}>");
                }
                "option" => {
                    let inner = array
                        .get(1)
                        .map(|v| map_type(v, helpers, owner, field_name))
                        .unwrap_or_else(|| "Vec<u8>".to_string());
                    return format!("OptionFlag<{inner}>");
                }
                "buffer" => {
                    let count_ty = array.get(1).and_then(|v| v.get("countType"));
                    let count_rust = count_ty
                        .and_then(|v| v.as_str())
                        .map(map_count_type)
                        .unwrap_or_else(|| "VarInt".to_string());
                    return format!("PrefixedBytes<{count_rust}>");
                }
                "container" => {
                    return helpers.register_container(value, &type_name(owner, field_name));
                }
                "mapper" => {
                    if let Ok(TypeExpr::Mapper(spec)) = TypeExpr::parse(value) {
                        return helpers.register_mapper(owner, field_name, &spec);
                    }
                }
                "switch" => {
                    if let Ok(TypeExpr::Switch(spec)) = TypeExpr::parse(value) {
                        return helpers.register_switch(value, owner, field_name, &spec);
                    }
                }
                _ => {}
            }
        }
    }
    String::from("Vec<u8>")
}

fn map_simple_type(name: &str) -> Option<&'static str> {
    match name {
        "varint" | "optvarint" => Some("VarInt"),
        "varlong" => Some("VarLong"),
        "u8" => Some("u8"),
        "u16" => Some("u16"),
        "u32" => Some("u32"),
        "u64" => Some("u64"),
        "i8" => Some("i8"),
        "i16" => Some("i16"),
        "i32" => Some("i32"),
        "i64" => Some("i64"),
        "bool" => Some("bool"),
        "f32" => Some("f32"),
        "f64" => Some("f64"),
        "UUID" => Some("UUID"),
        "string" => Some("String"),
        "void" => Some("()"),
        "buffer" | "ByteArray" | "restBuffer" => Some("Vec<u8>"),
        "anonymousNbt" | "anonOptionalNbt" => Some("nbt::NamedTag"),
        _ => None,
    }
}

fn map_count_type(name: &str) -> String {
    match name {
        "varint" => "VarInt".to_string(),
        "varlong" => "VarLong".to_string(),
        "u8" => "u8".to_string(),
        "u16" => "u16".to_string(),
        "u32" => "u32".to_string(),
        other => other.to_string(),
    }
}

fn type_name(owner: &str, field_name: Option<&str>) -> String {
    let mut name = to_pascal_case(owner);
    if let Some(field) = field_name {
        name.push_str(&to_pascal_case(field));
    }
    name
}

fn map_mapper_tag_type(spec: &MapperSpec) -> String {
    spec.tag_type
        .as_deref()
        .and_then(map_simple_type)
        .unwrap_or("VarInt")
        .to_string()
}

fn render_tag_value(tag_type: &str, tag: i32) -> String {
    match tag_type {
        "VarInt" => format!("VarInt({tag})"),
        "VarLong" => format!("VarLong({tag} as i64)"),
        "u8" => format!("{tag}u8"),
        "u16" => format!("{tag}u16"),
        "u32" => format!("{tag}u32"),
        "u64" => format!("{tag}u64"),
        "i8" => format!("{tag}i8"),
        "i16" => format!("{tag}i16"),
        "i32" => format!("{tag}i32"),
        "i64" => format!("{tag}i64"),
        _ => tag.to_string(),
    }
}

const HELPERS_PRELUDE: &str = r#"
use crate::protocol::*;
use std::io;

// Helper types emitted by the generator; these should be moved or replaced with
// hand-tuned implementations as needed.
#[derive(Debug, Clone, PartialEq)]
pub struct CountedArray<T, Count = VarInt> {
    pub values: Vec<T>,
    pub _phantom: std::marker::PhantomData<Count>,
}

impl<T, Count> Serializable for CountedArray<T, Count>
where
    T: Serializable,
    Count: Serializable + Into<i32> + From<VarInt>,
    VarInt: From<Count>,
{
    fn read_from<R: io::Read>(buf: &mut R) -> Result<Self, Error> {
        let count_varint = Count::read_from(buf)?;
        let count: i32 = VarInt::from(count_varint).0;
        let mut values = Vec::with_capacity(count as usize);
        for _ in 0..count {
            values.push(T::read_from(buf)?);
        }
        Ok(Self {
            values,
            _phantom: std::marker::PhantomData,
        })
    }

    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), Error> {
        let count = VarInt(self.values.len() as i32);
        count.write_to(buf)?;
        for v in &self.values {
            v.write_to(buf)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OptionFlag<T> {
    pub value: Option<T>,
}

impl<T> Serializable for OptionFlag<T>
where
    T: Serializable,
{
    fn read_from<R: io::Read>(buf: &mut R) -> Result<Self, Error> {
        let present = bool::read_from(buf)?;
        if present {
            Ok(Self {
                value: Some(T::read_from(buf)?),
            })
        } else {
            Ok(Self { value: None })
        }
    }

    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), Error> {
        match &self.value {
            Some(v) => {
                true.write_to(buf)?;
                v.write_to(buf)
            }
            None => false.write_to(buf),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PrefixedBytes<Count = VarInt> {
    pub data: Vec<u8>,
    pub _phantom: std::marker::PhantomData<Count>,
}

impl<Count> Serializable for PrefixedBytes<Count>
where
    Count: Serializable + Into<i32> + From<VarInt>,
    VarInt: From<Count>,
{
    fn read_from<R: io::Read>(buf: &mut R) -> Result<Self, Error> {
        let len_varint = Count::read_from(buf)?;
        let len: i32 = VarInt::from(len_varint).0;
        let mut data = vec![0u8; len as usize];
        buf.read_exact(&mut data)?;
        Ok(Self {
            data,
            _phantom: std::marker::PhantomData,
        })
    }

    fn write_to<W: io::Write>(&self, buf: &mut W) -> Result<(), Error> {
        let len = VarInt(self.data.len() as i32);
        len.write_to(buf)?;
        buf.write_all(&self.data)?;
        Ok(())
    }
}
"#;

fn extract_packet_fields(
    local_types: &serde_json::Map<String, Value>,
    global_types: &serde_json::Map<String, Value>,
    packet_type: &str,
) -> Result<Vec<PacketField>> {
    let definition = resolve_type_reference(local_types, global_types, packet_type)?;
    parse_container_fields(definition)
}

fn resolve_type_reference<'a>(
    local: &'a serde_json::Map<String, Value>,
    global: &'a serde_json::Map<String, Value>,
    mut name: &'a str,
) -> Result<&'a Value> {
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert(name.to_string()) {
            bail!("cyclic type alias detected for {name}");
        }
        if let Some(value) = local.get(name) {
            match value {
                Value::String(next) => {
                    name = next;
                    continue;
                }
                other => return Ok(other),
            }
        }
        if let Some(value) = global.get(name) {
            match value {
                Value::String(next) => {
                    name = next;
                    continue;
                }
                other => return Ok(other),
            }
        }
        bail!("missing type definition for {name}");
    }
}

fn parse_container_fields(type_value: &Value) -> Result<Vec<PacketField>> {
    let Some(array) = type_value.as_array() else {
        bail!("packet type definition must be an array");
    };
    let Some(kind) = array.get(0).and_then(|v| v.as_str()) else {
        bail!("type array missing discriminator");
    };
    if kind != "container" {
        bail!("packet type definition must be a container, found {kind}");
    }
    let Some(fields_value) = array.get(1).and_then(|v| v.as_array()) else {
        bail!("container definition missing field entries");
    };
    let mut fields = Vec::with_capacity(fields_value.len());
    for entry in fields_value {
        let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
            bail!("container field missing name");
        };
        let Some(ty) = entry.get("type") else {
            bail!("container field {name} missing type");
        };
        fields.push(PacketField {
            name: name.to_string(),
            ty: ty.clone(),
        });
    }
    Ok(fields)
}
