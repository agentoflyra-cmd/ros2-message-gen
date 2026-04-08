//! 消息宏定义
//!
//! 此模块提供用于实现 ROS2 消息解析的宏。
//!
//! # 使用方法
//!
//! 在你的项目中，首先导入必要的类型和宏：
//!
//! ```ignore
//! use ros2_message_gen::{impl_ros_message_default, cdr_encoding, byteorder, ParseError, RosMessage};
//! ```
//!
//! 然后为你的消息类型实现 RosMessage trait：

//! ```ignore
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! pub struct Point {
//!     pub x: f64,
//!     pub y: f64,
//!     pub z: f64,
//! }
//!
//! impl_ros_message_default!(Point, "geometry_msgs/msg/Point");
//! ```

use std::any::Any;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Unknown message type: {0}")]
    UnknownType(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("CDR deserialization error: {0}")]
    CdrError(String),
}

pub trait RosMessage: Send + Sync + Any {
    fn message_type() -> &'static str
    where
        Self: Sized;
    fn parse_from_cdr(data: &[u8]) -> Result<Self, ParseError>
    where
        Self: Sized;
    fn parse_from_cdr_with_endianness(
        data: &[u8],
        is_little_endian: bool,
    ) -> Result<Self, ParseError>
    where
        Self: Sized;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub trait MessageRegistry: Send + Sync {
    fn register<M: RosMessage + 'static>(&mut self);
    fn parse(&self, message_type: &str, data: &[u8]) -> Result<Box<dyn RosMessage>, ParseError>;
    fn parse_with_endianness(
        &self,
        message_type: &str,
        data: &[u8],
        is_little_endian: bool,
    ) -> Result<Box<dyn RosMessage>, ParseError>;
    fn get_type_info(&self, message_type: &str) -> Option<&'static str>;
}

#[derive(Debug, Default)]
pub struct SimpleRegistry {
    parsers: HashMap<&'static str, fn(&[u8]) -> Result<Box<dyn RosMessage>, ParseError>>,
    parsers_with_endianness:
        HashMap<&'static str, fn(&[u8], bool) -> Result<Box<dyn RosMessage>, ParseError>>,
    type_info: HashMap<&'static str, &'static str>,
}

impl SimpleRegistry {
    #[inline]
    pub fn new() -> Self {
        Self {
            parsers: HashMap::new(),
            parsers_with_endianness: HashMap::new(),
            type_info: HashMap::new(),
        }
    }

    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            parsers: HashMap::with_capacity(capacity),
            parsers_with_endianness: HashMap::with_capacity(capacity),
            type_info: HashMap::with_capacity(capacity),
        }
    }
}

impl MessageRegistry for SimpleRegistry {
    fn register<M: RosMessage + 'static>(&mut self) {
        let type_name = M::message_type();
        let type_info = std::any::type_name::<M>();

        self.parsers.insert(type_name, |data| {
            let msg = M::parse_from_cdr(data)?;
            Ok(Box::new(msg))
        });

        self.parsers_with_endianness
            .insert(type_name, |data, is_little_endian| {
                let msg = M::parse_from_cdr_with_endianness(data, is_little_endian)?;
                Ok(Box::new(msg))
            });

        self.type_info.insert(type_name, type_info);
    }

    fn parse(&self, message_type: &str, data: &[u8]) -> Result<Box<dyn RosMessage>, ParseError> {
        let parser = self
            .parsers
            .get(message_type)
            .ok_or_else(|| ParseError::UnknownType(message_type.to_string()))?;
        parser(data)
    }

    fn parse_with_endianness(
        &self,
        message_type: &str,
        data: &[u8],
        is_little_endian: bool,
    ) -> Result<Box<dyn RosMessage>, ParseError> {
        let parser = self
            .parsers_with_endianness
            .get(message_type)
            .ok_or_else(|| ParseError::UnknownType(message_type.to_string()))?;
        parser(data, is_little_endian)
    }

    fn get_type_info(&self, message_type: &str) -> Option<&'static str> {
        self.type_info.get(message_type).copied()
    }
}

#[macro_export]
macro_rules! impl_ros_message_default {
    ($type:ty, $message_type:expr) => {
        impl $crate::macros::RosMessage for $type {
            #[inline]
            fn message_type() -> &'static str
            where
                Self: Sized,
            {
                $message_type
            }

            #[inline]
            fn parse_from_cdr(data: &[u8]) -> Result<Self, $crate::macros::ParseError>
            where
                Self: Sized,
            {
                cdr_encoding::from_bytes::<Self, byteorder::LittleEndian>(data)
                    .map(|(msg, _)| msg)
                    .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
            }

            #[inline]
            fn parse_from_cdr_with_endianness(
                data: &[u8],
                is_little_endian: bool,
            ) -> Result<Self, $crate::macros::ParseError>
            where
                Self: Sized,
            {
                if is_little_endian {
                    cdr_encoding::from_bytes::<Self, byteorder::LittleEndian>(data)
                        .map(|(msg, _)| msg)
                        .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
                } else {
                    cdr_encoding::from_bytes::<Self, byteorder::BigEndian>(data)
                        .map(|(msg, _)| msg)
                        .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
                }
            }

            #[inline]
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            #[inline]
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}

#[macro_export]
macro_rules! impl_ros_message {
    ($type:ty, $message_type:expr) => {
        impl $crate::macros::RosMessage for $type {
            #[inline]
            fn message_type() -> &'static str
            where
                Self: Sized,
            {
                $message_type
            }

            #[inline]
            fn parse_from_cdr(data: &[u8]) -> Result<Self, $crate::macros::ParseError>
            where
                Self: Sized,
            {
                cdr_encoding::from_bytes::<Self, byteorder::LittleEndian>(data)
                    .map(|(msg, _)| msg)
                    .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
            }

            #[inline]
            fn parse_from_cdr_with_endianness(
                data: &[u8],
                is_little_endian: bool,
            ) -> Result<Self, $crate::macros::ParseError>
            where
                Self: Sized,
            {
                if is_little_endian {
                    cdr_encoding::from_bytes::<Self, byteorder::LittleEndian>(data)
                        .map(|(msg, _)| msg)
                        .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
                } else {
                    cdr_encoding::from_bytes::<Self, byteorder::BigEndian>(data)
                        .map(|(msg, _)| msg)
                        .map_err(|e| $crate::macros::ParseError::CdrError(e.to_string()))
                }
            }

            #[inline]
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            #[inline]
            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                self
            }
        }
    };
}

#[macro_export]
macro_rules! ros_message {
    (
        $(#[$meta:meta])*
        pub enum $enum_name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant_name:ident($message_type:expr, $ty:ty),
            )+
        }
    ) => {
        $(#[$meta])*
        pub enum $enum_name {
            $(
                $(#[$variant_meta])*
                $variant_name($ty),
            )+
        }

        impl $crate::macros::RosMessage for $enum_name {
            fn message_type() -> &'static str
            where
                Self: Sized,
            {
                unreachable!("This should not be called on enum directly")
            }

            fn parse_from_cdr(data: &[u8]) -> Result<Self, $crate::macros::ParseError>
            where
                Self: Sized,
            {
                unreachable!("Use parse_variant instead")
            }

            fn as_any(&self) -> &dyn std::any::Any {
                match self {
                    $(
                        $variant_name(msg) => msg.as_any(),
                    )+
                }
            }

            fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
                match self {
                    $(
                        $variant_name(msg) => msg.as_any_mut(),
                    )+
                }
            }
        }

        impl $enum_name {
            #[inline]
            pub fn parse_variant(message_type: &str, data: &[u8]) -> Result<Self, $crate::macros::ParseError> {
                match message_type {
                    $(
                        $message_type => {
                            let msg = <$ty>::parse_from_cdr(data)?;
                            Ok(Self::$variant_name(msg))
                        }
                    )+
                    _ => Err($crate::macros::ParseError::UnknownType(message_type.to_string())),
                }
            }

            #[inline]
            pub fn message_type(&self) -> &'static str {
                match self {
                    $(
                        $variant_name(msg) => <$ty>::message_type(),
                    )+
                }
            }
        }
    };
}

#[macro_export]
macro_rules! register_messages {
    ($registry:expr, { $($ty:ty),+ $(,)? }) => {
        $(
            $registry.register::<$ty>();
        )+
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct TestPoint {
        pub x: f64,
        pub y: f64,
    }

    impl_ros_message_default!(TestPoint, "geometry_msgs/msg/Point");

    #[test]
    fn test_impl_ros_message_default() {
        let test_data = vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ];

        let result = TestPoint::parse_from_cdr(&test_data);
        assert!(result.is_ok());

        let point = result.unwrap();
        assert_eq!(point.x, 1.0);
        assert_eq!(point.y, 0.0);
    }
}
