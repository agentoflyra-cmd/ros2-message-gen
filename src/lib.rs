//! ROS2 Message Code Generator for Rust
//!
//! This crate provides utilities to generate Rust code from ROS2 `.msg` and `.srv` files.
//! The generator writes one Rust crate per ROS package under the output directory, with
//! type references like `geometry_msgs::msg::Quaternion`.
//!
//! # Features
//!
//! - Parse ROS2 `.msg` and `.srv` files
//! - Support for arrays and complex types
//! - Generate one crate per ROS package
//! - Auto-generate package `Cargo.toml` files and local `path` dependencies
//! - Generate a shared `ros2-dispatch` crate for schema-based decode dispatch
//! - Generate `decode.rs` with `DecodeCdr` implementations
//! - Configurable naming conventions
//!
//! # Quick Start
//!
//! ```ignore
//! use ros2_message_gen::{MessageGenerator, StructNameStyle};
//!
//! // Create a generator
//! let generator = MessageGenerator::new("generated_ws".to_string());
//!
//! // Generate from a directory containing .msg files
//! generator.generate_from_directory("/mnt/ubuntu/opt/ros/humble/share")?;
//!
//! // Or generate from ROS environment variables
//! generator.generate_from_ros_env()?;
//! ```
//!
//! # Generated Code Example
//!
//! The generator produces package code like this:
//!
//! ```ignore
//! #[cfg_attr(feature = "serde", derive(Deserialize, Serialize))]
//! #[derive(Clone, Debug, PartialEq, PartialOrd)]
//! pub struct Imu {
//!     #[allow(missing_docs)]
//!     pub header: std_msgs::msg::Header,
//!
//!     #[allow(missing_docs)]
//!     pub orientation: geometry_msgs::msg::Quaternion,
//! }
//! ```

pub mod parser;

pub use parser::{Field, MessageType, StructNameStyle};

mod generator;
pub use generator::{GeneratorConfig, MessageGenerator};

/// Re-export of commonly used items
pub mod prelude {
    pub use crate::{Field, GeneratorConfig, MessageGenerator, MessageType, StructNameStyle};

    pub use serde::{Deserialize, Serialize};
}
