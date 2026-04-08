use std::fs;
use std::path::Path;

/// struct 命名风格
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StructNameStyle {
    /// 驼峰命名 (Point, PkgA) - 默认
    #[default]
    CamelCase,
    /// 蛇形命名 (point, pkg_a)
    SnakeCase,
    /// 帕斯卡命名 (Point, PkgA) - 同 CamelCase
    PascalCase,
}

/// ROS数据类型到Rust类型的映射
const TYPE_MAPPING: &[(&str, &str)] = &[
    ("bool", "bool"),
    ("byte", "u8"),
    ("char", "u8"),
    ("float32", "f32"),
    ("float64", "f64"),
    ("int8", "i8"),
    ("uint8", "u8"),
    ("int16", "i16"),
    ("uint16", "u16"),
    ("int32", "i32"),
    ("uint32", "u32"),
    ("int64", "i64"),
    ("uint64", "u64"),
    ("string", "std::string::String"),
    ("wstring", "std::string::String"),
];

/// ROS内置消息类型映射
const BUILTIN_TYPES: &[(&str, &str)] = &[
    ("builtin_interfaces/Time", "builtin_interfaces::msg::Time"),
    (
        "builtin_interfaces/Duration",
        "builtin_interfaces::msg::Duration",
    ),
    ("std_msgs/Header", "std_msgs::msg::Header"),
];

/// 消息字段定义
///
/// # 示例
///
/// ```
/// use ros2_message_gen::parser::Field;
///
/// let field = Field::new("int32".to_string(), "data".to_string());
/// assert_eq!(field.field_type, "int32");
/// assert_eq!(field.name, "data");
/// assert!(!field.is_array);
/// ```
#[derive(Debug, Clone)]
pub struct Field {
    pub field_type: String,
    pub name: String,
    pub is_array: bool,
    pub array_size: Option<usize>,
}

/// 常量定义
#[derive(Debug, Clone)]
pub struct Constant {
    pub const_type: String,
    pub name: String,
    pub value: String,
}

/// ROS消息类型定义
///
/// # 示例
///
/// ```
/// use ros2_message_gen::parser::MessageType;
/// use std::path::Path;
///
/// let content = "int32 data\nstring name\nfloat64[] values";
/// let path = Path::new("/tmp/test_pkg/msg/Test.msg");
/// let msg_type = MessageType::from_content(path, content).unwrap();
///
/// assert_eq!(msg_type.package, "test_pkg");
/// assert_eq!(msg_type.name, "Test");
/// assert_eq!(msg_type.fields.len(), 3);
/// ```
#[derive(Debug, Clone)]
pub struct MessageType {
    pub package: String,
    pub name: String,
    pub fields: Vec<Field>,
    pub constants: Vec<Constant>,
}

impl Field {
    pub fn new(field_type: String, name: String) -> Self {
        // 检查是否是数组类型
        let (base_type, is_array, array_size) = if field_type.contains('[') {
            let parts: Vec<&str> = field_type.split('[').collect();
            let base_type = parts[0].to_string();
            let array_part = parts[1].trim_end_matches(']');
            if array_part.is_empty() {
                (base_type, true, None) // 动态数组
            } else {
                let size = array_part.parse::<usize>().ok();
                (base_type, true, size) // 固定大小数组
            }
        } else {
            (field_type, false, None)
        };

        Self {
            field_type: base_type,
            name,
            is_array,
            array_size,
        }
    }

    pub fn rust_type(&self, current_package: &str) -> String {
        let base_rust_type = self.map_ros_type_to_rust(&self.field_type, current_package);

        if self.is_array {
            if let Some(size) = self.array_size {
                format!("[{}; {}]", base_rust_type, size)
            } else {
                format!("Vec<{}>", base_rust_type)
            }
        } else {
            base_rust_type
        }
    }

    fn map_ros_type_to_rust(&self, ros_type: &str, current_package: &str) -> String {
        // 首先检查是否是内置类型
        for (ros_builtin, rust_type) in BUILTIN_TYPES {
            if *ros_builtin == ros_type {
                return rust_type.to_string();
            }
        }

        // 然后检查基本类型映射
        for (ros_basic, rust_basic) in TYPE_MAPPING {
            if *ros_basic == ros_type {
                return rust_basic.to_string();
            }
        }

        // 如果都没有匹配，假设它是另一个自定义消息类型
        // 使用 crate 路径来引用，避免重复生成依赖类型
        self.map_custom_type_path(ros_type, current_package)
    }

    fn map_custom_type_path(&self, ros_type: &str, current_package: &str) -> String {
        let parts: Vec<&str> = ros_type.split('/').collect();

        match parts.as_slice() {
            // package/msg/Type
            [package, "msg", ty] => {
                if *package == current_package {
                    (*ty).to_string()
                } else {
                    format!("{}::msg::{}", package, ty)
                }
            }
            // package/srv/Type
            [package, "srv", ty] => {
                if *package == current_package {
                    (*ty).to_string()
                } else {
                    format!("{}::srv::{}", package, ty)
                }
            }
            // package/Type (默认视为 msg)
            [package, ty] => {
                if *package == current_package {
                    (*ty).to_string()
                } else {
                    format!("{}::msg::{}", package, ty)
                }
            }
            // 当前包内类型
            [ty] => (*ty).to_string(),
            // 其它路径，保持原样
            _ => ros_type.to_string(),
        }
    }

    #[allow(dead_code)]
    fn snake_to_camel(&self, snake_str: &str) -> String {
        let mut camel_case = String::new();
        let mut capitalize_next = true;

        for ch in snake_str.chars() {
            if ch == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                camel_case.push(ch.to_uppercase().next().unwrap());
                capitalize_next = false;
            } else {
                camel_case.push(ch);
            }
        }

        camel_case
    }
}

impl MessageType {
    pub fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let content = fs::read_to_string(path)?;
        Self::from_content(path, &content)
    }

    pub fn from_content(path: &Path, content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let file_name = path.file_stem().unwrap().to_str().unwrap();
        let parent = path.parent().unwrap();
        let package_name = parent
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();

        let (fields, constants) = parse_fields_and_constants(content, package_name);

        Ok(MessageType {
            package: package_name.to_string(),
            name: file_name.to_string(),
            fields,
            constants,
        })
    }

    pub fn struct_name(&self, style: StructNameStyle) -> String {
        match style {
            StructNameStyle::CamelCase | StructNameStyle::PascalCase => {
                self.snake_to_camel(&self.name)
            }
            StructNameStyle::SnakeCase => self.name.clone(),
        }
    }

    /// 获取完整的 ROS 消息类型名称 (例如 "geometry_msgs/msg/Point")
    pub fn message_type_name(&self, include_msg_suffix: bool) -> String {
        if include_msg_suffix {
            format!("{}/msg/{}", self.package, self.name)
        } else {
            format!("{}/{}", self.package, self.name)
        }
    }

    fn snake_to_camel(&self, snake_str: &str) -> String {
        let mut camel_case = String::new();
        let mut capitalize_next = true;

        for ch in snake_str.chars() {
            if ch == '_' {
                capitalize_next = true;
            } else if capitalize_next {
                camel_case.push(ch.to_uppercase().next().unwrap());
                capitalize_next = false;
            } else {
                camel_case.push(ch);
            }
        }

        camel_case
    }

    /// 生成完整的 Rust 结构体代码
    ///
    /// # 配置参数
    ///
    /// - `struct_name_style`: 控制 struct 命名风格 (CamelCase 或 SnakeCase)
    /// - `include_msg_suffix`: 是否在消息类型名称中包含 /msg/ 中缀
    ///
    /// # 示例输出
    ///
    /// ```ignore
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Debug, Clone, Serialize, Deserialize)]
    /// pub struct Point {
    ///     pub x: f64,
    ///     pub y: f64,
    ///     pub z: f64,
    /// }
    ///
    /// ```
    pub fn to_rust_struct_with_impl(
        &self,
        struct_name_style: StructNameStyle,
        include_msg_suffix: bool,
    ) -> String {
        let struct_name = self.struct_name(struct_name_style);
        let mut output = String::new();

        // 生成结构体
        output.push_str("#[cfg_attr(feature = \"serde\", derive(Deserialize, Serialize))]\n");
        output.push_str("#[derive(Clone, Debug, PartialEq, PartialOrd)]\n");
        output.push_str(&format!("pub struct {} {{\n", struct_name));

        for field in &self.fields {
            let field_name = rust_identifier(&field.name);
            output.push_str("    #[allow(missing_docs)]\n");
            output.push_str(&format!(
                "    pub {}: {},\n\n",
                field_name,
                field.rust_type(&self.package)
            ));
        }

        output.push_str("}\n\n");

        // 生成常量定义
        if !self.constants.is_empty() {
            output.push_str(&format!("impl {} {{\n", struct_name));
            for constant in &self.constants {
                // 解析常量值和类型
                let (const_value, const_type) =
                    self.parse_constant_value(&constant.value, &constant.const_type);
                output.push_str(&format!(
                    "    pub const {}: {} = {};\n",
                    constant.name, const_type, const_value
                ));
            }
            output.push_str("}\n\n");
        }

        let _ = include_msg_suffix;

        output
    }

    /// 解析常量值并推断类型
    fn parse_constant_value(&self, value_str: &str, declared_type: &str) -> (String, String) {
        if declared_type == "String" || declared_type == "std::string::String" {
            return (
                format!("\"{}\"", escape_rust_string(value_str)),
                "&'static str".to_string(),
            );
        }

        if !declared_type.is_empty() {
            return (value_str.to_string(), declared_type.to_string());
        }

        // 尝试解析为不同类型
        if let Ok(int_val) = value_str.parse::<i64>() {
            // 根据值大小选择合适的类型
            if int_val >= i8::MIN as i64 && int_val <= i8::MAX as i64 {
                (int_val.to_string(), "i8".to_string())
            } else if int_val >= i16::MIN as i64 && int_val <= i16::MAX as i64 {
                (int_val.to_string(), "i16".to_string())
            } else if int_val >= i32::MIN as i64 && int_val <= i32::MAX as i64 {
                (int_val.to_string(), "i32".to_string())
            } else {
                (int_val.to_string(), "i64".to_string())
            }
        } else if let Ok(uint_val) = value_str.parse::<u64>() {
            // 无符号整数
            if uint_val <= u8::MAX as u64 {
                (uint_val.to_string(), "u8".to_string())
            } else if uint_val <= u16::MAX as u64 {
                (uint_val.to_string(), "u16".to_string())
            } else if uint_val <= u32::MAX as u64 {
                (uint_val.to_string(), "u32".to_string())
            } else {
                (uint_val.to_string(), "u64".to_string())
            }
        } else if let Ok(float_val) = value_str.parse::<f64>() {
            // 浮点数
            if float_val >= f32::MIN as f64 && float_val <= f32::MAX as f64 {
                (float_val.to_string(), "f32".to_string())
            } else {
                (float_val.to_string(), "f64".to_string())
            }
        } else {
            // 字符串或其他类型，默认为字符串
            (format!("\"{}\"", value_str), "String".to_string())
        }
    }

    /// 生成仅包含结构体的代码（不含宏调用）
    pub fn to_rust_struct(&self, style: StructNameStyle) -> String {
        let struct_name = self.struct_name(style);
        let mut output = String::new();

        output.push_str("#[cfg_attr(feature = \"serde\", derive(Deserialize, Serialize))]\n");
        output.push_str("#[derive(Clone, Debug, PartialEq, PartialOrd)]\n");
        output.push_str(&format!("pub struct {} {{\n", struct_name));

        for field in &self.fields {
            let field_name = rust_identifier(&field.name);
            output.push_str("    #[allow(missing_docs)]\n");
            output.push_str(&format!(
                "    pub {}: {},\n\n",
                field_name,
                field.rust_type(&self.package)
            ));
        }

        output.push_str("}\n\n");

        output
    }
}

pub(crate) fn parse_fields_and_constants(
    content: &str,
    package_name: &str,
) -> (Vec<Field>, Vec<Constant>) {
    let mut fields = Vec::new();
    let mut constants = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();

        if line.is_empty() {
            continue;
        }

        if line == "---" {
            break;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        if line.contains('=') && parts.len() >= 2 {
            let equal_pos = line.find('=').unwrap();
            let left_part = line[..equal_pos].trim();
            let right_part = line[equal_pos + 1..].trim();
            let left_parts: Vec<&str> = left_part.split_whitespace().collect();

            if left_parts.len() >= 2 {
                let field_type = left_parts[0];
                let name_part = left_parts[1];
                let is_constant_name = name_part.chars().next().map_or(false, |c| c.is_uppercase())
                    && !name_part.contains('[')
                    && !name_part.contains(']')
                    && !name_part.chars().any(|c| c.is_lowercase() && c != '_');

                if is_constant_name {
                    constants.push(Constant {
                        const_type: Field::new(field_type.to_string(), "_".to_string())
                            .rust_type(package_name),
                        name: name_part.to_string(),
                        value: right_part.to_string(),
                    });
                    continue;
                }

                fields.push(Field::new(field_type.to_string(), name_part.to_string()));
            }
            continue;
        }

        if parts.len() >= 2 {
            fields.push(Field::new(parts[0].to_string(), parts[1].to_string()));
        }
    }

    (fields, constants)
}

fn escape_rust_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn rust_identifier(name: &str) -> &str {
    match name {
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
        | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
        | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
        | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await"
        | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override"
        | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "try" => match name {
            "as" => "r#as",
            "break" => "r#break",
            "const" => "r#const",
            "continue" => "r#continue",
            "crate" => "r#crate",
            "else" => "r#else",
            "enum" => "r#enum",
            "extern" => "r#extern",
            "false" => "r#false",
            "fn" => "r#fn",
            "for" => "r#for",
            "if" => "r#if",
            "impl" => "r#impl",
            "in" => "r#in",
            "let" => "r#let",
            "loop" => "r#loop",
            "match" => "r#match",
            "mod" => "r#mod",
            "move" => "r#move",
            "mut" => "r#mut",
            "pub" => "r#pub",
            "ref" => "r#ref",
            "return" => "r#return",
            "self" => "r#self",
            "Self" => "r#Self",
            "static" => "r#static",
            "struct" => "r#struct",
            "super" => "r#super",
            "trait" => "r#trait",
            "true" => "r#true",
            "type" => "r#type",
            "unsafe" => "r#unsafe",
            "use" => "r#use",
            "where" => "r#where",
            "while" => "r#while",
            "async" => "r#async",
            "await" => "r#await",
            "dyn" => "r#dyn",
            "abstract" => "r#abstract",
            "become" => "r#become",
            "box" => "r#box",
            "do" => "r#do",
            "final" => "r#final",
            "macro" => "r#macro",
            "override" => "r#override",
            "priv" => "r#priv",
            "typeof" => "r#typeof",
            "unsized" => "r#unsized",
            "virtual" => "r#virtual",
            "yield" => "r#yield",
            "try" => "r#try",
            _ => name,
        },
        _ => name,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_field_parsing() {
        let field = Field::new("int32".to_string(), "data".to_string());
        assert_eq!(field.field_type, "int32");
        assert_eq!(field.name, "data");
        assert!(!field.is_array);
        assert_eq!(field.rust_type("test"), "i32");

        let array_field = Field::new("float64[]".to_string(), "values".to_string());
        assert!(array_field.is_array);
        assert_eq!(array_field.array_size, None);
        assert_eq!(array_field.rust_type("test"), "Vec<f64>");

        let fixed_array_field = Field::new("uint8[16]".to_string(), "uuid".to_string());
        assert!(fixed_array_field.is_array);
        assert_eq!(fixed_array_field.array_size, Some(16));
        assert_eq!(fixed_array_field.rust_type("test"), "[u8; 16]");
    }

    #[test]
    fn test_message_type_parsing() {
        let content = "int32 data\nstring name\nfloat64[] values";
        let path = std::path::Path::new("/tmp/test_pkg/msg/Test.msg");
        let msg_type = MessageType::from_content(path, content).unwrap();

        assert_eq!(msg_type.package, "test_pkg");
        assert_eq!(msg_type.name, "Test");
        assert_eq!(msg_type.fields.len(), 3);
    }

    #[test]
    fn test_constant_parsing_uses_declared_type() {
        let content = "uint64 DEPTH_RECURSIVE=1\nstring[] prefixes\nuint64 depth";
        let path = std::path::Path::new("/tmp/rcl_interfaces/msg/ListParameters_Request.msg");
        let msg_type = MessageType::from_content(path, content).unwrap();

        assert_eq!(msg_type.name, "ListParameters_Request");
        assert_eq!(msg_type.constants.len(), 1);
        assert_eq!(msg_type.constants[0].name, "DEPTH_RECURSIVE");
        assert_eq!(msg_type.constants[0].const_type, "u64");
        assert_eq!(msg_type.fields.len(), 2);
        assert_eq!(msg_type.fields[0].name, "prefixes");
        assert_eq!(msg_type.fields[1].name, "depth");

        let generated = msg_type.to_rust_struct_with_impl(StructNameStyle::CamelCase, true);
        assert!(generated.contains("pub struct ListParametersRequest"));
        assert!(generated.contains("pub const DEPTH_RECURSIVE: u64 = 1;"));
        assert!(!generated.contains("pub DEPTH_RECURSIVE: u64,"));
    }

    #[test]
    fn test_request_response_suffixes_are_preserved() {
        let req_path = std::path::Path::new("/tmp/example_interfaces/msg/AddTwoInts_Request.msg");
        let resp_path = std::path::Path::new("/tmp/example_interfaces/msg/AddTwoInts_Response.msg");

        let req = MessageType::from_content(req_path, "int64 a\nint64 b").unwrap();
        let resp = MessageType::from_content(resp_path, "int64 sum").unwrap();

        assert_eq!(req.name, "AddTwoInts_Request");
        assert_eq!(resp.name, "AddTwoInts_Response");
        assert_eq!(
            req.struct_name(StructNameStyle::CamelCase),
            "AddTwoIntsRequest"
        );
        assert_eq!(
            resp.struct_name(StructNameStyle::CamelCase),
            "AddTwoIntsResponse"
        );
    }

    #[test]
    fn test_constant_parsing_accepts_spaced_equals() {
        let content = "uint8 PENDING = 0\nuint8 ACTIVE = 1\nstring text";
        let path = std::path::Path::new("/tmp/actionlib_msgs/msg/GoalStatus.msg");
        let msg_type = MessageType::from_content(path, content).unwrap();

        assert_eq!(msg_type.constants.len(), 2);
        assert_eq!(msg_type.fields.len(), 1);

        let generated = msg_type.to_rust_struct_with_impl(StructNameStyle::CamelCase, true);
        assert!(generated.contains("pub const PENDING: u8 = 0;"));
        assert!(generated.contains("pub const ACTIVE: u8 = 1;"));
        assert!(!generated.contains("pub PENDING: u8,"));
        assert!(!generated.contains("pub ACTIVE: u8,"));
    }

    #[test]
    fn test_string_constants_are_quoted() {
        let content = "string DISABLE_HEARTBEAT_TIMEOUT_PARAM=/bond_disable_heartbeat_timeout";
        let path = std::path::Path::new("/tmp/bond/msg/Constants.msg");
        let msg_type = MessageType::from_content(path, content).unwrap();

        let generated = msg_type.to_rust_struct_with_impl(StructNameStyle::CamelCase, true);
        assert!(
            generated.contains(
                "pub const DISABLE_HEARTBEAT_TIMEOUT_PARAM: &'static str = \"/bond_disable_heartbeat_timeout\";"
            )
        );
    }

    #[test]
    fn test_rust_keywords_are_escaped_in_field_names() {
        let content = "uint8 fn\nstring type";
        let path = std::path::Path::new("/tmp/unitree_hg/msg/BmsState.msg");
        let msg_type = MessageType::from_content(path, content).unwrap();

        let generated = msg_type.to_rust_struct_with_impl(StructNameStyle::CamelCase, true);
        assert!(generated.contains("pub r#fn: u8,"));
        assert!(generated.contains("pub r#type: std::string::String,"));
    }
}
