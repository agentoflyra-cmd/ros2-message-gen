# ROS2 Message Generator for Rust

A standalone Rust crate for generating Rust code from ROS 2 `.msg` and `.srv` files.
The generator writes one Rust crate per ROS package into the output directory, with
clean cross-package type references such as `geometry_msgs::msg::Quaternion`.

## Motivation

Most existing ROS 2 message generators in Rust are tightly coupled with the DDS/RMW
ecosystem and focus on runtime communication.

This project takes a different direction:

> treat ROS 2 messages as a wire format rather than a runtime abstraction.

The goal is to build a lightweight Rust toolchain that can:

- generate Rust structs from `.msg` and `.srv` definitions
- decode ROS 2 CDR-encoded message payloads
- encode generated message types back into ROS 2 CDR payloads
- work without a ROS 2 runtime binding

This is aimed at workflows such as:

- offline rosbag / MCAP processing
- SLAM and robotics data pipelines
- dataset conversion and analysis

## Docs

- [Performance notes](docs/performance-notes.md)
- [Benchmarking](docs/benchmarking.md)

## Current Status

This project is still evolving, but the current generator already supports:

- parsing ROS 2 `.msg` and `.srv` files
- generating one Rust crate per ROS package
- generating a shared `cdr-runtime` crate
- generating a shared `ros2-dispatch` crate
- generating `decode.rs` with automatic `DecodeCdr` impls for all generated message types
- generating `encode.rs` with automatic `EncodeCdr` impls for all generated message types

The runtime and output layout may still change, but the current path is:

- shared `cdr-runtime`
- shared `ros2-dispatch`
- generated package crates with `msg.rs`, `srv.rs`, `decode.rs`, and `encode.rs`
- explicit, inspectable encode/decode logic instead of ROS runtime bindings

## Features

- Parse ROS 2 `.msg` and `.srv` files
- Support primitive fields, arrays, constants, and nested types
- Generate one crate per ROS package under the output directory
- Cross-package type references such as `geometry_msgs::msg::Quaternion`
- Auto-generate per-package `Cargo.toml` files and local `path` dependencies
- Generate a shared `cdr-runtime` crate
- Generate a shared `ros2-dispatch` crate with schema-based decode and encode dispatch
- Generate `decode.rs` with automatic `DecodeCdr` implementations
- Generate `borrow_decode.rs` with automatic `BorrowDecodeCdr<'a>` implementations
- Generate `encode.rs` with automatic `EncodeCdr` implementations
- Configurable naming conventions
- Standalone binary tool
- Library integration

## Quick Start

### As a Binary Tool

```bash
cargo install /path/to/ros2-message-gen

# Generate Rust crates from a ROS interface tree
ros2-message-gen -d /mnt/ubuntu/opt/ros/humble/share generated_ws

# Generate from detected ROS environment variables
ros2-message-gen -r generated_ws
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
ros2-message-gen = { path = "../message-gen" }
```

Use it in your code:

```rust
use ros2_message_gen::MessageGenerator;

let generator = MessageGenerator::new("generated_ws".to_string());
generator.generate_from_directory("/mnt/ubuntu/opt/ros/humble/share")?;
```

## Generated Output Layout

The generator creates:

```text
generated_ws/
├── cdr-runtime
│   ├── Cargo.toml
│   └── src
│       └── lib.rs
├── ros2-dispatch
│   ├── Cargo.toml
│   └── src
│       └── lib.rs
├── geometry_msgs
│   ├── Cargo.toml
│   └── src
│       ├── borrow_decode.rs
│       ├── borrowed.rs
│       ├── decode.rs
│       ├── encode.rs
│       ├── lib.rs
│       ├── msg.rs
│       └── srv.rs
├── sensor_msgs
│   ├── Cargo.toml
│   └── src
│       ├── decode.rs
│       ├── encode.rs
│       ├── lib.rs
│       ├── msg.rs
│       └── srv.rs
└── workspace-members.toml
```

Notes:

- `cdr-runtime` is shared by all generated package crates.
- `ros2-dispatch` depends on generated package crates and provides schema-based dynamic decode,
  plus enum-based re-encoding after mutation.
- Each package crate gets its own `Cargo.toml`.
- Cross-package references are emitted as normal Rust crate paths.
- `decode.rs` is generated code, not a placeholder. It re-exports runtime items and adds
  `impl DecodeCdr for T` for all generated message and service request/response types.
- `borrow_decode.rs` is generated alongside `decode.rs`. It provides borrowed decode entrypoints
  and `impl BorrowDecodeCdr<'a> for T<'a>` for generated borrowed message views.
- `borrowed.rs` contains borrowed message views plus `to_owned()` conversions back to the
  normal owned `msg`/`srv` types.
- `encode.rs` is generated alongside `decode.rs`. It re-exports runtime items and adds
  `impl EncodeCdr for T` for all generated message and service request/response types.

The output directory itself is not generated as a Cargo workspace root. This avoids nested
workspace conflicts when you place generated crates inside an existing workspace.

If the generator finds an existing parent workspace, it appends generated package paths to
that workspace's `members` list automatically. If no parent workspace is found, it writes
`workspace-members.toml` with ready-to-paste member entries.

## Generated Code Example

```rust
#[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct Imu {
    #[allow(missing_docs)]
    pub header: std_msgs::msg::Header,

    #[allow(missing_docs)]
    pub orientation: geometry_msgs::msg::Quaternion,

    #[allow(missing_docs)]
    pub orientation_covariance: [f64; 9],
}
```

Generated decode code is emitted separately in `decode.rs`, for example:

```rust
pub use cdr_runtime::{decode_from_bytes, CdrDecoder, DecodeCdr, Endianness, WChar16, WChar32};

impl DecodeCdr for Imu {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        Ok(Self {
            header: <std_msgs::msg::Header as DecodeCdr>::decode_cdr(decoder)?,
            orientation: <geometry_msgs::msg::Quaternion as DecodeCdr>::decode_cdr(decoder)?,
            orientation_covariance: decoder.read_array::<f64, 9>()?,
        })
    }
}
```

Generated encode code is emitted separately in `encode.rs`, for example:

```rust
pub use cdr_runtime::{encode_to_vec, CdrEncoder, EncodeCdr, Endianness, WChar16, WChar32};

impl EncodeCdr for Imu {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        <std_msgs::msg::Header as EncodeCdr>::encode_cdr(&self.header, encoder)?;
        <geometry_msgs::msg::Quaternion as EncodeCdr>::encode_cdr(
            &self.orientation,
            encoder,
        )?;
        encoder.write_array::<f64, _>(&self.orientation_covariance)?;
        Ok(())
    }
}
```

Generated borrowed decode code is emitted separately in `borrowed.rs` and
`borrow_decode.rs`, for example:

```rust
#[derive(Clone, Debug, PartialEq, PartialOrd)]
pub struct Header<'a> {
    pub frame_id: &'a str,
}

impl<'a> Header<'a> {
    pub fn to_owned(&self) -> crate::msg::Header {
        crate::msg::Header {
            frame_id: self.frame_id.to_string(),
        }
    }
}

impl<'a> BorrowDecodeCdr<'a> for Header<'a> {
    fn borrow_decode_cdr(decoder: &mut CdrDecoder<'a>) -> CdrResult<Self> {
        Ok(Self {
            frame_id: decoder.read_str()?,
        })
    }
}
```

## Borrowed Decode

Borrowed decode is the project's current answer to the original "zerocopy" goal:
keep lifetimes from the input payload instead of forcing every field to allocate
immediately.

The generated crates now expose both:

- owned decode via `decode::decode_from_bytes::<T>()`
- borrowed decode via `borrow_decode::borrow_decode_from_bytes::<T>()`

### Current Borrowed Coverage

Fields that currently decode as borrowed views:

- `string` -> `&'a str`
- `uint8[]` / `byte[]` -> `&'a [u8]`
- dynamic primitive sequences such as `float32[]`, `int32[]`, `uint64[]`
  -> `cdr_runtime::PrimitiveSeq<'a, T>`
- fixed primitive arrays such as `float64[9]`
  -> `cdr_runtime::PrimitiveArray<'a, T, N>`
- nested non-array messages recurse into their borrowed variants

Fields that are still decoded as owned values inside borrowed structs:

- `wstring`
- non-byte dynamic arrays of complex nested message types
- fixed arrays of nested message types

### Minimal Borrowed Example

```rust
use sensor_msgs::borrow_decode::borrow_decode_from_bytes;
use sensor_msgs::borrowed::Imu as BorrowedImu;

let borrowed = borrow_decode_from_bytes::<BorrowedImu<'_>>(&payload)?;

// borrowed fields
let frame_id: &str = borrowed.header.frame_id;
let raw_bytes: &[u8] = borrowed.raw_bytes;
let samples = borrowed.samples.iter();

// convert back to the normal owned generated type only when needed
let owned = borrowed.to_owned();
```

## Performance

The project now has three decode modes in the benchmark harness:

- `decode_owned`
- `decode_borrowed`
- `decode_borrowed_to_owned`

On the current benchmark fixture, the latest measured result is:

```text
little,decode_owned,1000,4492000,199.818,21439.035
little,decode_borrowed,1000,4492000,84.288,50824.614
little,decode_borrowed_to_owned,1000,4492000,202.064,21200.734
big,decode_owned,1000,4492000,270.516,15836.050
big,decode_borrowed,1000,4492000,92.547,46288.967
big,decode_borrowed_to_owned,1000,4492000,232.337,18438.325
```

That means:

- borrowed decode is now materially faster than owned decode
- borrowed decode followed by `to_owned()` is roughly on par with owned decode for
  little-endian payloads and faster for the current big-endian fixture
- the "lifetime-based zerocopy" direction is paying off in measurable terms

For more detail, see [Benchmarking](docs/benchmarking.md) and
[Performance notes](docs/performance-notes.md).

Generated schema dispatch code is emitted in `ros2-dispatch/src/lib.rs`, for example:

```rust
#[derive(Clone, Debug)]
pub enum DecodedMessage {
    SensorMsgsImu(sensor_msgs::msg::Imu),
    LifecycleMsgsChangeStateRequest(lifecycle_msgs::srv::ChangeStateRequest),
}

impl DecodedMessage {
    pub fn schema_name(&self) -> &'static str {
        match self {
            Self::SensorMsgsImu(_) => "sensor_msgs/msg/Imu",
            Self::LifecycleMsgsChangeStateRequest(_) => {
                "lifecycle_msgs/srv/ChangeState_Request"
            }
        }
    }

    pub fn encode_to_vec(&self) -> Result<Vec<u8>, String> {
        match self {
            Self::SensorMsgsImu(msg) => sensor_msgs::encode::encode_to_vec(msg),
            Self::LifecycleMsgsChangeStateRequest(msg) => {
                lifecycle_msgs::encode::encode_to_vec(msg)
            }
        }
    }
}

pub fn decode_message_by_schema(
    schema_name: &str,
    data: &[u8],
) -> Result<DecodedMessage, String> {
    match schema_name {
        "sensor_msgs/msg/Imu" => Ok(DecodedMessage::SensorMsgsImu(
            sensor_msgs::decode::decode_from_bytes::<sensor_msgs::msg::Imu>(data)?,
        )),
        "lifecycle_msgs/srv/ChangeState_Request" => {
            Ok(DecodedMessage::LifecycleMsgsChangeStateRequest(
                lifecycle_msgs::decode::decode_from_bytes::<
                    lifecycle_msgs::srv::ChangeStateRequest,
                >(data)?,
            ))
        }
        _ => Err(format!("unknown schema: {schema_name}")),
    }
}
```

## Minimal Integration Example

### 1. Generate Package Crates

```bash
ros2-message-gen -d /mnt/ubuntu/opt/ros/humble/share generated_interfaces
```

### 2. Generated Package Manifest

For example, `generated_interfaces/sensor_msgs/Cargo.toml`:

```toml
[package]
name = "sensor_msgs"
version = "0.1.0"
edition = "2024"

[dependencies]
cdr-runtime = { path = "../cdr-runtime" }
serde = { version = "1.0", features = ["derive"], optional = true }
geometry_msgs = { path = "../geometry_msgs" }
std_msgs = { path = "../std_msgs" }

[features]
default = []
serde = ["dep:serde", "geometry_msgs/serde", "std_msgs/serde"]
```

### 3. Depend on It from Main Project

In your main crate `Cargo.toml`:

```toml
[dependencies]
sensor_msgs = { path = "../generated_interfaces/sensor_msgs", features = ["serde"] }
geometry_msgs = { path = "../generated_interfaces/geometry_msgs", features = ["serde"] }
std_msgs = { path = "../generated_interfaces/std_msgs", features = ["serde"] }
```

If you want schema-based dynamic decode, also depend on the generated dispatch crate:

```toml
[dependencies]
ros2-dispatch = { path = "../generated_interfaces/ros2-dispatch" }
```

### 4. Use Generated Types

```rust
use sensor_msgs::msg::Imu;

fn handle_imu(msg: Imu) {
    let _frame = msg.header.frame_id;
    let _qx = msg.orientation.x;
}
```

### 5. Decode a CDR Payload

```rust
use sensor_msgs::decode::decode_from_bytes;
use sensor_msgs::msg::Imu;

fn parse_imu(bytes: &[u8]) -> Result<Imu, String> {
    decode_from_bytes::<Imu>(bytes)
}
```

### 6. Decode by Schema Name

```rust
use ros2_dispatch::{decode_message_by_schema, DecodedMessage};

fn parse_dynamic(schema_name: &str, bytes: &[u8]) -> Result<(), String> {
    let message = decode_message_by_schema(schema_name, bytes)?;

    match &message {
        DecodedMessage::SensorMsgsImu(msg) => {
            let _ = &msg.orientation;
        }
        _ => {}
    }

    let _schema = message.schema_name();
    Ok(())
}
```

Currently `ros2-dispatch` includes:

- `.msg` schema names such as `sensor_msgs/msg/Imu`
- `.srv` request schema names such as `lifecycle_msgs/srv/ChangeState_Request`
- `.srv` response schema names such as `lifecycle_msgs/srv/ChangeState_Response`

### 7. Decode, Mutate, and Re-encode Dynamically

```rust
use ros2_dispatch::{decode_message_by_schema, DecodedMessage};

fn patch_and_reencode(schema_name: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut message = decode_message_by_schema(schema_name, bytes)?;

    match &mut message {
        DecodedMessage::SensorMsgsImu(msg) => {
            msg.orientation_covariance[0] = 1.0;
        }
        _ => {}
    }

    message.encode_to_vec()
}
```

### 8. Encode a Generated Message

```rust
use sensor_msgs::encode::encode_to_vec;
use sensor_msgs::msg::Imu;

fn encode_imu(msg: &Imu) -> Result<Vec<u8>, String> {
    encode_to_vec(msg)
}
```

### 9. Add Generated Packages to an Existing Workspace

If `generated_interfaces/` lives inside an existing Cargo workspace, the generator will try
to append entries like these to the top-level workspace `Cargo.toml` automatically:

```toml
members = [
    "crates/app",
    "generated_interfaces/cdr-runtime",
    "generated_interfaces/ros2-dispatch",
    "generated_interfaces/std_msgs",
    "generated_interfaces/geometry_msgs",
    "generated_interfaces/sensor_msgs",
]
```

If no parent workspace is found automatically, the same entries are written to
`generated_interfaces/workspace-members.toml` as raw member lines that you can paste into
your existing `members = [ ... ]` array.

Then crates inside that workspace can depend on the generated packages normally:

```toml
[dependencies]
sensor_msgs = { path = "../generated_interfaces/sensor_msgs", features = ["serde"] }
```

## Type Mapping Notes

### Arrays

- Dynamic arrays: `type[]` -> `Vec<type>`
- Fixed arrays: `type[N]` -> `[type; N]`

### Built-in Types

- `string` -> `std::string::String`
- `wstring` -> `std::string::String`
- `builtin_interfaces/Time`, `builtin_interfaces/Duration`, and `std_msgs/Header` are
  resolved as normal cross-package message references
- Custom message types are referenced by module paths instead of being duplicated

### Constants

- ROS constants are emitted as associated constants in an `impl` block
- String constants are emitted as `&'static str`
- Inline comments in `.msg` / `.srv` constant definitions are stripped during parsing

## Workspace Integration

This crate itself can live inside a normal Rust workspace:

```toml
[workspace]
members = [
    "your-main-crate",
    "message-gen",
]
resolver = "2"
```

And your main crate can use it as a generator dependency:

```toml
[dependencies]
ros2-message-gen = { path = "../message-gen" }
```

## Command Line Interface

```bash
ros2-message-gen [OPTIONS] <output_dir>
```

### Arguments

- `--dir <dir>`: Directory containing interfaces; searched recursively
- `--env <var>`: Environment variable containing ROS install prefixes
- `--ros-env`: Auto-detect from ROS environment variables
- `output_dir`: Target directory that will contain generated package crates

### Examples

```bash
# Generate from a single directory tree
ros2-message-gen -d /mnt/ubuntu/opt/ros/humble/share generated_ws

# Generate from ROS environment
ros2-message-gen -r generated_ws
```

## Development

### Building

```bash
cargo build
cargo build --release
```

### Testing

```bash
cargo test
cargo run --bin ros2-message-gen -- --help
```

### Code Quality

```bash
cargo fmt
cargo clippy
```

## License

MIT
