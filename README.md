
# ROS2 Message Generator for Rust

A standalone Rust crate for generating Rust code from ROS2 `.msg` and `.srv` files.
The generator writes one Rust crate per ROS package into the output directory, with clean
cross-package type references such as
`geometry_msgs::msg::Quaternion`.



### Motivation

Most existing ROS 2 message generators in Rust are tightly coupled with the DDS/RMW ecosystem, focusing on enabling runtime communication.

This project explores a different direction:

> treating ROS 2 messages as a **wire format** rather than a runtime abstraction.

The goal is to build a lightweight Rust toolchain that can:

* generate Rust structs from `.msg` definitions
* decode ROS 2 CDR-encoded message payloads
* work without requiring a full ROS 2 installation

This is particularly motivated by use cases such as:

* offline rosbag / MCAP processing
* SLAM and robotics data pipelines
* dataset conversion and analysis

---

### Current Status

⚠️ This project is in an early stage.

At the moment, it is focused on:

* parsing `.msg` definitions
* generating Rust struct representations
* experimenting with a minimal CDR decoding layer

Many features are incomplete or subject to change.

---

### Design Direction (WIP)

The long-term direction is to:

* decouple message handling from ROS 2 runtime
* provide deterministic, inspectable decoding
* enable integration with non-ROS data pipelines



## Features

- Parse ROS2 `.msg` and `.srv` files
- Support for arrays and complex types  
- Generate one crate per ROS package under the output directory
- Cross-package type references (e.g. `geometry_msgs::msg::Quaternion`)
- Auto-generate per-package `Cargo.toml` files and cross-package `path` dependencies
- Placeholder `decode.rs` for future backend integration
- Configurable naming conventions
- Standalone binary tool
- Library integration

## Quick Start

### As a Binary Tool

```bash
# Install from source or add to your Rust project
cargo install /path/to/ros2-message-gen

# Generate Rust code into an output directory
ros2-message-gen -d /mnt/ubuntu/opt/ros/humble/share generated_ws

# Generate from ROS environment variables
ros2-message-gen -r generated_ws
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
<!-- ros2-message-gen = "0.1.0" -->
serde = { version = "1.0", features = ["derive"] }
cdr-encoding = "0.10"
byteorder = "1.5"
```

Use in your code:

```rust
use ros2_message_gen::{MessageGenerator, StructNameStyle};

// Create a generator
let generator = MessageGenerator::new("generated_ws".to_string());

// Generate from a directory containing .msg files
generator.generate_from_directory("/mnt/ubuntu/opt/ros/humble/share")?;

// Or generate from ROS environment variables
generator.generate_from_ros_env()?;
```

## Generated Output Layout

The generator creates:

```text
generated_ws/
├── workspace-members.toml
├── geometry_msgs
│   ├── Cargo.toml
│   └── src
│       ├── lib.rs
│       ├── decode.rs
│       ├── msg.rs
│       └── srv.rs
└── sensor_msgs
    ├── Cargo.toml
    └── src
        ├── lib.rs
        ├── srv.rs
        ├── msg.rs
        └── decode.rs
```

`decode.rs` contains a placeholder trait for deserialization backend integration and does
not bind to a concrete CDR backend yet.

Each package crate gets its own `Cargo.toml`, and any referenced ROS package is added as a
local `path` dependency.

The output directory itself is not generated as a Cargo workspace root. This avoids nested
workspace conflicts when you place the generated packages inside an existing workspace.
If the generator finds an existing parent workspace, it appends the generated package paths
to that workspace's `members` list automatically. If no parent workspace is found, it
writes `workspace-members.toml` with ready-to-paste member entries.

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
serde = { version = "1.0", features = ["derive"], optional = true }
geometry_msgs = { path = "../geometry_msgs" }
std_msgs = { path = "../std_msgs" }

[features]
default = []
serde = ["dep:serde"]
```

### 3. Depend on It from Main Project

In your main crate `Cargo.toml`:

```toml
[dependencies]
sensor_msgs = { path = "../generated_interfaces/sensor_msgs", features = ["serde"] }
geometry_msgs = { path = "../generated_interfaces/geometry_msgs", features = ["serde"] }
std_msgs = { path = "../generated_interfaces/std_msgs", features = ["serde"] }
```

### 4. Use Generated Types

```rust
use sensor_msgs::msg::Imu;

fn handle_imu(msg: Imu) {
    let _frame = msg.header.frame_id;
    let _qx = msg.orientation.x;
}
```

### 5. Add Generated Packages to an Existing Workspace

If `generated_interfaces/` lives inside an existing Cargo workspace, add the generated
ROS packages to the top-level workspace `Cargo.toml`:

```toml
members = [
    "crates/app",
    "generated_interfaces/std_msgs",
    "generated_interfaces/geometry_msgs",
    "generated_interfaces/sensor_msgs",
]
```

If no parent workspace is found automatically, the same entries are written to
`generated_interfaces/workspace-members.toml`.

Then crates inside that workspace can depend on the generated packages normally:

```toml
[dependencies]
sensor_msgs = { path = "../generated_interfaces/sensor_msgs", features = ["serde"] }
```

## Configuration Options

```rust
use ros2_message_gen::{MessageGenerator, GeneratorConfig, StructNameStyle};

let config = GeneratorConfig::new()
    .with_struct_name_style(StructNameStyle::CamelCase)
    .with_include_msg_suffix(true);

let generator = MessageGenerator::with_config("generated_ws".to_string(), config);
```

### Struct Naming Styles

- `CamelCase`: `Point`, `RobotStatus` (default)
- `SnakeCase`: `point`, `robot_status`  
- `PascalCase`: Same as CamelCase

## Supported ROS2 Types

### Primitive Types
- `bool`, `int8`, `uint8`, `int16`, `uint16`, `int32`, `uint32`, `int64`, `uint64`
- `float32`, `float64`, `string`, `wstring`, `byte`, `char`

### Arrays
- Dynamic arrays: `type[]` → `Vec<type>`
- Fixed arrays: `type[N]` → `[type; N]`

### Built-in Types
- `builtin_interfaces/Time`, `builtin_interfaces/Duration`, `std_msgs/Header`
- Custom message types are referenced by module paths instead of being duplicated

## Workspace Integration

This crate is designed to work as part of a Rust workspace. Add it to your workspace `Cargo.toml`:

```toml
[workspace]
members = [
    "your-main-crate",
    "message-gen",  # This crate
]
```

And use it in your main crate:

```toml
[dependencies]
ros2-message-gen = { path = "../message-gen" }
```

## Command Line Interface

```bash
ros2-message-gen [OPTIONS] <output_dir>
```

### Arguments
- `--dir <dir>`: Directory containing interfaces (recursively searched)
- `--env <var>`: Environment variable containing ROS install prefixes
- `--ros-env`: Auto-detect from ROS environment variables
- `output_dir`: Target directory that will contain generated package crates

### Examples

```bash
# Generate from single directory
ros2-message-gen -d /mnt/ubuntu/opt/ros/humble/share generated_ws

# Generate from detected ROS environment
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

MIT License - see LICENSE file for details.

## Contributing

Contributions are welcome! Please submit pull requests or file issues on the project repository.
