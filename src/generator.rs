use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use codegen::{Function, Impl, Scope};

use crate::parser;

use super::StructNameStyle;
use super::parser::{MessageType, parse_fields_and_constants};

const CDR_RUNTIME_SOURCE: &str = include_str!("cdr.rs");

/// 消息生成配置
#[derive(Debug, Clone)]
pub struct GeneratorConfig {
    /// struct 命名风格
    pub struct_name_style: StructNameStyle,
    /// 是否在消息类型名称中包含 /msg/ 中缀（当前仅保留兼容）
    pub include_msg_suffix: bool,
}

impl Default for GeneratorConfig {
    fn default() -> Self {
        Self {
            struct_name_style: StructNameStyle::CamelCase,
            include_msg_suffix: true,
        }
    }
}

impl GeneratorConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_struct_name_style(mut self, style: StructNameStyle) -> Self {
        self.struct_name_style = style;
        self
    }

    pub fn with_include_msg_suffix(mut self, include: bool) -> Self {
        self.include_msg_suffix = include;
        self
    }
}

pub fn generate_msg_module(messages: &[&MessageType]) -> String {
    let mut scope = Scope::new();
    for message in messages {
        add_message_struct(&mut scope, message);
    }
    scope.to_string()
}

fn add_message_struct(scope: &mut Scope, message: &MessageType) {
    let struct_name = message.struct_name(StructNameStyle::CamelCase);
    let s = scope.new_struct(&struct_name);

    s.vis("pub");
    s.attr(r#"cfg_attr(feature = "serde", derive(Deserialize, Serialize))"#);
    s.derive("Clone");
    s.derive("Debug");
    s.derive("PartialEq");
    s.derive("PartialOrd");

    for field in &message.fields {
        let f = s.new_field(
            rust_field_name_for_decode(&field.name),
            field.rust_type(&message.package),
        );
        f.vis("pub");
        f.annotation("#[allow(missing_docs)]");
    }

    add_message_content(scope, message);
}

fn add_message_content(scope: &mut Scope, message: &MessageType) {
    if message.constants.is_empty() {
        return;
    }

    let struct_name = message.struct_name(StructNameStyle::CamelCase);
    let imp = scope.new_impl(&struct_name);
    add_consts_to_impl(imp, &message.constants);
}

fn add_consts_to_impl(imp: &mut Impl, consts: &[parser::Constant]) {
    for constant in consts {
        let ty = constant.const_type.as_str();
        let name = constant.name.as_str();
        let value = constant.value.as_str();
        imp.associate_const(name, ty, value, "pub");
    }
}

fn generate_srv_module(services: &[(&MessageType, &MessageType)]) -> String {
    let mut scope = Scope::new();

    for (request, response) in services {
        add_message_struct(&mut scope, request);
        add_message_content(&mut scope, request);
        add_message_struct(&mut scope, response);
        add_message_content(&mut scope, response);
    }
    scope.to_string()
}

/// 消息生成器
#[derive(Debug, Clone)]
pub struct MessageGenerator {
    output_path: String,
    config: GeneratorConfig,
}

impl MessageGenerator {
    /// 创建新的消息生成器。
    ///
    /// `output_path` 现在表示输出目录，会生成以下布局：
    /// - `src/lib.rs`
    /// - `src/msg.rs`
    /// - `src/srv.rs`
    /// - `src/decode.rs`
    pub fn new(output_path: String) -> Self {
        Self {
            output_path,
            config: GeneratorConfig::new(),
        }
    }

    pub fn default() -> Self {
        Self::new("generated".to_string())
    }

    pub fn with_config(output_path: String, config: GeneratorConfig) -> Self {
        Self {
            output_path,
            config,
        }
    }

    pub fn with_struct_name_style(mut self, style: StructNameStyle) -> Self {
        self.config.struct_name_style = style;
        self
    }

    pub fn with_include_msg_suffix(mut self, include: bool) -> Self {
        self.config.include_msg_suffix = include;
        self
    }

    pub fn generate_from_directory(
        &self,
        dir_path: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.generate_from_multiple_directories(&[dir_path.to_string()])
    }

    pub fn generate_from_env(&self, env_var: &str) -> Result<(), Box<dyn std::error::Error>> {
        let env_value = std::env::var(env_var)
            .map_err(|_| format!("Environment variable '{}' not set", env_var))?;

        let dir_paths: Vec<String> = env_value
            .split(':')
            .filter(|p| !p.is_empty())
            .map(|s| format!("{}/share", s))
            .collect();

        if dir_paths.is_empty() {
            return Err(format!("No paths found in environment variable '{}'", env_var).into());
        }

        self.generate_from_multiple_directories(&dir_paths)
    }

    pub fn generate_from_ros_env(&self) -> Result<(), Box<dyn std::error::Error>> {
        let env_vars = ["AMENT_PREFIX_PATH", "CMAKE_PREFIX_PATH", "ROS_PACKAGE_PATH"];

        for env_var in &env_vars {
            if let Ok(value) = std::env::var(env_var) {
                if !value.is_empty() {
                    return self.generate_from_env(env_var);
                }
            }
        }

        Err("No ROS environment variable found (AMENT_PREFIX_PATH, CMAKE_PREFIX_PATH, ROS_PACKAGE_PATH)".into())
    }

    pub fn generate_from_multiple_directories(
        &self,
        dir_paths: &[String],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut messages = Vec::new();
        let mut services = Vec::new();

        for dir_path in dir_paths {
            self.collect_interfaces(Path::new(dir_path), &mut messages, &mut services)?;
        }

        self.write_package_layouts(&messages, &services)?;
        Ok(())
    }

    fn collect_interfaces(
        &self,
        root: &Path,
        messages: &mut Vec<MessageType>,
        services: &mut Vec<(MessageType, MessageType)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if !root.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(root)? {
            let path = entry?.path();

            if path.is_dir() {
                self.collect_interfaces(&path, messages, services)?;
                continue;
            }

            match path.extension().and_then(|e| e.to_str()) {
                Some("msg") => {
                    if is_generated_service_message(&path) {
                        continue;
                    }
                    let msg = MessageType::from_file(&path)?;
                    messages.push(msg);
                }
                Some("srv") => {
                    let parsed = parse_srv_file(&path)?;
                    services.push(parsed);
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn write_package_layouts(
        &self,
        messages: &[MessageType],
        services: &[(MessageType, MessageType)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let root = PathBuf::from(&self.output_path);
        let mut messages_by_pkg: HashMap<&str, Vec<&MessageType>> = HashMap::new();
        let mut services_by_pkg: HashMap<&str, Vec<(&MessageType, &MessageType)>> = HashMap::new();

        for msg in messages {
            messages_by_pkg.entry(&msg.package).or_default().push(msg);
        }

        for (request, response) in services {
            services_by_pkg
                .entry(&request.package)
                .or_default()
                .push((request, response));
        }

        let mut packages: Vec<&str> = messages_by_pkg
            .keys()
            .copied()
            .chain(services_by_pkg.keys().copied())
            .collect();
        packages.sort_unstable();
        packages.dedup();

        fs::create_dir_all(&root)?;
        self.write_cdr_runtime(&root)?;

        let mut workspace_members = Vec::with_capacity(packages.len() + 1);
        workspace_members.push("cdr-runtime");
        workspace_members.extend(packages.iter().copied());

        if !try_append_workspace_members(&root, &workspace_members)? {
            fs::write(
                root.join("workspace-members.toml"),
                workspace_members_snippet(&root, &workspace_members)?,
            )?;
        }

        for package in packages {
            let crate_dir = root.join(package);
            let pkg_messages = messages_by_pkg.get(package).cloned().unwrap_or_default();
            let pkg_services = services_by_pkg.get(package).cloned().unwrap_or_default();
            self.write_layout_for_package(&crate_dir, &pkg_messages, &pkg_services)?;
        }

        Ok(())
    }

    fn write_layout_for_package(
        &self,
        crate_dir: &Path,
        messages: &[&MessageType],
        services: &[(&MessageType, &MessageType)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let src_dir = crate_dir.join("src");
        fs::create_dir_all(&src_dir)?;

        let package_name = crate_dir
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("invalid package output path")?;
        let dependencies = package_dependencies(package_name, messages, services);
        fs::write(
            crate_dir.join("Cargo.toml"),
            package_manifest(package_name, &dependencies),
        )?;

        fs::write(
            src_dir.join("lib.rs"),
            "pub mod decode;\npub mod msg;\npub mod srv;\n",
        )?;

        let mut msg_rs = String::new();
        msg_rs.push_str("#![allow(unused_imports)]\n");
        msg_rs.push_str("#[cfg(feature = \"serde\")]\n");
        msg_rs.push_str("use serde::{Deserialize, Serialize};\n\n");

        // for msg in messages {
        //     msg_rs.push_str(&msg.to_rust_struct_with_impl(
        //         self.config.struct_name_style,
        //         self.config.include_msg_suffix,
        //     ));
        //     msg_rs.push('\n');
        // }
        msg_rs.push_str(generate_msg_module(messages).as_str());

        let mut srv_rs = String::new();
        srv_rs.push_str("#![allow(unused_imports)]\n");
        srv_rs.push_str("#[cfg(feature = \"serde\")]\n");
        srv_rs.push_str("use serde::{Deserialize, Serialize};\n");
        srv_rs.push_str("use crate::msg::*;\n\n");

        // for (request, response) in services {
        //     srv_rs.push_str(&request.to_rust_struct_with_impl(
        //         self.config.struct_name_style,
        //         self.config.include_msg_suffix,
        //     ));
        //     srv_rs.push('\n');
        //     srv_rs.push_str(&response.to_rust_struct_with_impl(
        //         self.config.struct_name_style,
        //         self.config.include_msg_suffix,
        //     ));
        //     srv_rs.push('\n');
        // }
        srv_rs.push_str(generate_srv_module(services).as_str());

        let decode_rs = generate_decode_module(messages, services);
        fs::write(src_dir.join("msg.rs"), msg_rs)?;
        fs::write(src_dir.join("srv.rs"), srv_rs)?;
        fs::write(src_dir.join("decode.rs"), decode_rs)?;

        Ok(())
    }

    fn write_cdr_runtime(&self, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let runtime_dir = root.join("cdr-runtime");
        let runtime_src_dir = runtime_dir.join("src");
        fs::create_dir_all(&runtime_src_dir)?;
        fs::write(
            runtime_dir.join("Cargo.toml"),
            "[package]\nname = \"cdr-runtime\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )?;
        fs::write(runtime_src_dir.join("lib.rs"), CDR_RUNTIME_SOURCE)?;
        Ok(())
    }
}

fn is_generated_service_message(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };

    let base_name = if let Some(base) = stem.strip_suffix("_Request") {
        base
    } else if let Some(base) = stem.strip_suffix("_Response") {
        base
    } else if let Some(base) = stem.strip_suffix("_Event") {
        base
    } else {
        return false;
    };

    let Some(package_dir) = path.parent().and_then(Path::parent) else {
        return false;
    };
    package_dir
        .join("srv")
        .join(format!("{base_name}.srv"))
        .exists()
}

fn workspace_members_snippet(
    root: &Path,
    packages: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid output directory name")?;
    let mut content = String::new();
    for package in packages {
        content.push_str(&format!("\"{}/{}\",\n", root_name, package));
    }
    Ok(content)
}

fn try_append_workspace_members(
    root: &Path,
    packages: &[&str],
) -> Result<bool, Box<dyn std::error::Error>> {
    let Some(workspace_manifest) = find_workspace_manifest(root) else {
        return Ok(false);
    };

    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("invalid output directory name")?;
    let mut entries = Vec::new();
    for package in packages {
        entries.push(format!("{}/{}", root_name, package));
    }

    let content = fs::read_to_string(&workspace_manifest)?;
    let Some(updated) = append_members_to_workspace_manifest(&content, &entries) else {
        return Ok(false);
    };

    fs::write(workspace_manifest, updated)?;
    Ok(true)
}

fn find_workspace_manifest(root: &Path) -> Option<PathBuf> {
    let mut current = root.parent();

    while let Some(dir) = current {
        let manifest = dir.join("Cargo.toml");
        if let Ok(content) = fs::read_to_string(&manifest) {
            if content.contains("[workspace]") {
                return Some(manifest);
            }
        }
        current = dir.parent();
    }

    None
}

fn append_members_to_workspace_manifest(content: &str, entries: &[String]) -> Option<String> {
    let workspace_start = content.find("[workspace]")?;
    let members_start = workspace_start + content[workspace_start..].find("members")?;
    let line_start = content[..members_start]
        .rfind('\n')
        .map_or(0, |idx| idx + 1);
    let open_bracket = members_start + content[members_start..].find('[')?;
    let close_bracket = open_bracket + content[open_bracket..].find(']')?;

    let existing_raw = &content[open_bracket + 1..close_bracket];
    let mut members: Vec<String> = existing_raw
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.trim_matches('"').to_string())
        .collect();

    let mut changed = false;
    for entry in entries {
        if !members.iter().any(|existing| existing == entry) {
            members.push(entry.clone());
            changed = true;
        }
    }

    if !changed {
        return Some(content.to_string());
    }

    let mut replacement = String::from("members = [\n");
    for member in members {
        replacement.push_str(&format!("    \"{}\",\n", member));
    }
    replacement.push(']');

    let mut updated = String::new();
    updated.push_str(&content[..line_start]);
    updated.push_str(&replacement);
    updated.push_str(&content[close_bracket + 1..]);
    Some(updated)
}

fn package_manifest(package: &str, dependencies: &[String]) -> String {
    let mut content = String::new();
    content.push_str("[package]\n");
    content.push_str(&format!("name = \"{}\"\n", package));
    content.push_str("version = \"0.1.0\"\n");
    content.push_str("edition = \"2024\"\n\n");
    content.push_str("[dependencies]\n");
    content.push_str("cdr-runtime = { path = \"../cdr-runtime\" }\n");
    content.push_str("serde = { version = \"1.0\", features = [\"derive\"], optional = true }\n");
    for dep in dependencies {
        content.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
    }
    content.push_str("\n[features]\n");
    content.push_str("default = []\n");
    content.push_str("serde = [\"dep:serde\"");
    for dep in dependencies {
        content.push_str(&format!(", \"{dep}/serde\""));
    }
    content.push_str("]\n");
    content
}

fn package_dependencies(
    current_package: &str,
    messages: &[&MessageType],
    services: &[(&MessageType, &MessageType)],
) -> Vec<String> {
    let mut dependencies = Vec::new();

    for message in messages {
        collect_message_dependencies(current_package, message, &mut dependencies);
    }

    for (request, response) in services {
        collect_message_dependencies(current_package, request, &mut dependencies);
        collect_message_dependencies(current_package, response, &mut dependencies);
    }

    dependencies.sort();
    dependencies.dedup();
    dependencies
}

fn collect_message_dependencies(
    current_package: &str,
    message: &MessageType,
    dependencies: &mut Vec<String>,
) {
    for field in &message.fields {
        let rust_type = field.rust_type(current_package);
        if let Some(package) = rust_type_dependency_package(&rust_type) {
            if package != current_package {
                dependencies.push(package.to_string());
            }
        }
    }
}

fn rust_type_dependency_package(rust_type: &str) -> Option<&str> {
    if !(rust_type.contains("::msg::") || rust_type.contains("::srv::")) {
        return None;
    }

    let prefix = rust_type.split("::").next()?;
    if matches!(
        prefix,
        "bool"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
            | "String"
    ) {
        return None;
    }

    Some(prefix.trim_start_matches("Vec<").trim_start_matches('['))
}

fn generate_decode_module(
    messages: &[&MessageType],
    services: &[(&MessageType, &MessageType)],
) -> String {
    let mut scope = Scope::new();

    for msg in messages {
        add_decode_impl(&mut scope, msg);
    }

    for (request, response) in services {
        add_decode_impl(&mut scope, request);
        add_decode_impl(&mut scope, response);
    }

    let mut content = String::new();
    content.push_str("#[allow(unused_imports)]\n");
    content.push_str(
        "pub use cdr_runtime::{decode_from_bytes, CdrDecoder, DecodeCdr, Endianness, WChar16, WChar32};\n",
    );
    content.push_str("#[allow(unused_imports)]\n");
    content.push_str("use crate::msg::*;\n");
    content.push_str("#[allow(unused_imports)]\n");
    content.push_str("use crate::srv::*;\n\n");
    content.push_str(&scope.to_string());
    content
}

fn add_decode_impl(scope: &mut Scope, message: &MessageType) {
    let struct_name = message.struct_name(StructNameStyle::CamelCase);
    let imp = scope.new_impl(&struct_name);
    imp.impl_trait("DecodeCdr");
    let function = imp.new_fn("decode_cdr");
    configure_decode_function(function, message);
}

fn configure_decode_function(function: &mut Function, message: &MessageType) {
    function.arg("decoder", "&mut CdrDecoder<'_>");
    function.ret("Result<Self, std::string::String>");
    function.line("Ok(Self {");
    for field in &message.fields {
        function.line(format!(
            "    {}: {},",
            rust_field_name_for_decode(&field.name),
            decode_expression(field, &message.package)
        ));
    }
    function.line("})");
}

fn rust_field_name_for_decode(name: &str) -> String {
    match name {
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
        | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
        | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
        | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await"
        | "dyn" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "override"
        | "priv" | "typeof" | "unsized" | "virtual" | "yield" | "try" => format!("r#{}", name),
        _ => name.to_string(),
    }
}

fn decode_expression(field: &super::parser::Field, current_package: &str) -> String {
    let base_type = field.rust_type(current_package);
    if field.is_array {
        if let Some(size) = field.array_size {
            format!(
                "decoder.read_array::<{}, {}>()?",
                strip_container_type(&base_type),
                size
            )
        } else {
            format!("decoder.read_seq::<{}>()?", strip_vec_type(&base_type))
        }
    } else {
        format!("<{} as DecodeCdr>::decode_cdr(decoder)?", base_type)
    }
}

fn strip_vec_type(rust_type: &str) -> &str {
    rust_type
        .strip_prefix("Vec<")
        .and_then(|inner| inner.strip_suffix('>'))
        .unwrap_or(rust_type)
}

fn strip_container_type(rust_type: &str) -> &str {
    rust_type
        .strip_prefix('[')
        .and_then(|inner| inner.split(';').next())
        .map(str::trim)
        .unwrap_or(rust_type)
}

fn parse_srv_file(path: &Path) -> Result<(MessageType, MessageType), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut sections = content.splitn(2, "---");
    let req_content = sections.next().unwrap_or("");
    let resp_content = sections.next().unwrap_or("");

    let service_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("invalid .srv file name")?;
    let package = path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .and_then(|s| s.to_str())
        .ok_or("invalid .srv package path")?
        .to_string();
    let (req_fields, req_constants) = parse_fields_and_constants(req_content, &package);
    let (resp_fields, resp_constants) = parse_fields_and_constants(resp_content, &package);

    let req = MessageType {
        package: package.clone(),
        name: format!("{}Request", service_name),
        fields: req_fields,
        constants: req_constants,
    };

    let resp = MessageType {
        package,
        name: format!("{}Response", service_name),
        fields: resp_fields,
        constants: resp_constants,
    };

    Ok((req, resp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::process::Command;
    use tempfile::tempdir;

    #[test]
    fn generates_tree_layout_for_messages() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let std_pkg_dir = temp_dir.path().join("std_msgs");
        let std_msg_dir = std_pkg_dir.join("msg");
        fs::create_dir_all(&std_msg_dir)?;
        let mut std_file = File::create(std_msg_dir.join("Header.msg"))?;
        writeln!(std_file, "builtin_interfaces/Time stamp")?;
        writeln!(std_file, "string frame_id")?;

        let geometry_pkg_dir = temp_dir.path().join("geometry_msgs");
        let geometry_msg_dir = geometry_pkg_dir.join("msg");
        fs::create_dir_all(&geometry_msg_dir)?;
        let mut quat_file = File::create(geometry_msg_dir.join("Quaternion.msg"))?;
        writeln!(quat_file, "float64 x")?;
        writeln!(quat_file, "float64 y")?;
        writeln!(quat_file, "float64 z")?;
        writeln!(quat_file, "float64 w")?;

        let pkg_dir = temp_dir.path().join("sensor_msgs");
        let msg_dir = pkg_dir.join("msg");
        fs::create_dir_all(&msg_dir)?;

        let msg_file = msg_dir.join("Imu.msg");
        let mut file = File::create(&msg_file)?;
        writeln!(file, "std_msgs/Header header")?;
        writeln!(file, "geometry_msgs/Quaternion orientation")?;
        writeln!(file, "float64[9] orientation_covariance")?;

        let output_dir = temp_dir.path().join("generated");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let single_dir = output_dir.join("sensor_msgs");
        let members_snippet = fs::read_to_string(output_dir.join("workspace-members.toml"))?;
        assert!(members_snippet.contains("\"generated/geometry_msgs\","));
        assert!(members_snippet.contains("\"generated/sensor_msgs\","));
        assert!(members_snippet.contains("\"generated/std_msgs\","));
        assert!(!members_snippet.contains("[workspace]"));
        assert!(single_dir.join("src/lib.rs").exists());
        assert!(single_dir.join("Cargo.toml").exists());
        assert!(single_dir.join("src/msg.rs").exists());
        assert!(single_dir.join("src/srv.rs").exists());
        assert!(single_dir.join("src/decode.rs").exists());

        let package_manifest = fs::read_to_string(single_dir.join("Cargo.toml"))?;
        assert!(package_manifest.contains("geometry_msgs = { path = \"../geometry_msgs\" }"));
        assert!(package_manifest.contains("std_msgs = { path = \"../std_msgs\" }"));

        let msg_content = fs::read_to_string(single_dir.join("src/msg.rs"))?;
        assert!(msg_content.contains("pub struct Imu"));
        assert!(msg_content.contains("std_msgs::msg::Header"));
        assert!(msg_content.contains("geometry_msgs::msg::Quaternion"));

        Ok(())
    }

    #[test]
    fn appends_generated_packages_to_existing_workspace() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp_dir = tempdir()?;
        let workspace_root = temp_dir.path().join("workspace");
        fs::create_dir_all(workspace_root.join("crates/app"))?;
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\n    \"crates/app\",\n]\nresolver = \"2\"\n",
        )?;

        let std_pkg_dir = temp_dir.path().join("std_msgs");
        let std_msg_dir = std_pkg_dir.join("msg");
        fs::create_dir_all(&std_msg_dir)?;
        let mut std_file = File::create(std_msg_dir.join("Header.msg"))?;
        writeln!(std_file, "builtin_interfaces/Time stamp")?;
        writeln!(std_file, "string frame_id")?;

        let geometry_pkg_dir = temp_dir.path().join("geometry_msgs");
        let geometry_msg_dir = geometry_pkg_dir.join("msg");
        fs::create_dir_all(&geometry_msg_dir)?;
        let mut quat_file = File::create(geometry_msg_dir.join("Quaternion.msg"))?;
        writeln!(quat_file, "float64 x")?;
        writeln!(quat_file, "float64 y")?;
        writeln!(quat_file, "float64 z")?;
        writeln!(quat_file, "float64 w")?;

        let sensor_pkg_dir = temp_dir.path().join("sensor_msgs");
        let sensor_msg_dir = sensor_pkg_dir.join("msg");
        fs::create_dir_all(&sensor_msg_dir)?;
        let mut sensor_file = File::create(sensor_msg_dir.join("Imu.msg"))?;
        writeln!(sensor_file, "std_msgs/Header header")?;
        writeln!(sensor_file, "geometry_msgs/Quaternion orientation")?;

        let output_dir = workspace_root.join("ros2_msgs");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let workspace_manifest = fs::read_to_string(workspace_root.join("Cargo.toml"))?;
        assert!(workspace_manifest.contains("\"crates/app\""));
        assert!(workspace_manifest.contains("\"ros2_msgs/std_msgs\""));
        assert!(workspace_manifest.contains("\"ros2_msgs/geometry_msgs\""));
        assert!(workspace_manifest.contains("\"ros2_msgs/sensor_msgs\""));
        assert!(!output_dir.join("workspace-members.toml").exists());

        Ok(())
    }

    #[test]
    fn generated_packages_can_be_imported_from_external_workspace()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let workspace_root = temp_dir.path().join("workspace");
        let app_dir = workspace_root.join("crates/app");
        fs::create_dir_all(app_dir.join("src"))?;
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\n    \"crates/app\",\n]\nresolver = \"2\"\n",
        )?;
        fs::write(
            app_dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nsensor_msgs = { path = \"../../ros2_msgs/sensor_msgs\", features = [\"serde\"] }\n",
        )?;
        fs::write(
            app_dir.join("src/main.rs"),
            "use sensor_msgs::msg::Imu;\n\nfn main() {\n    let _ = core::mem::size_of::<Imu>();\n}\n",
        )?;

        let std_pkg_dir = temp_dir.path().join("std_msgs");
        let std_msg_dir = std_pkg_dir.join("msg");
        fs::create_dir_all(&std_msg_dir)?;
        let mut std_file = File::create(std_msg_dir.join("Header.msg"))?;
        writeln!(std_file, "builtin_interfaces/Time stamp")?;
        writeln!(std_file, "string frame_id")?;

        let builtin_pkg_dir = temp_dir.path().join("builtin_interfaces");
        let builtin_msg_dir = builtin_pkg_dir.join("msg");
        fs::create_dir_all(&builtin_msg_dir)?;
        let mut time_file = File::create(builtin_msg_dir.join("Time.msg"))?;
        writeln!(time_file, "int32 sec")?;
        writeln!(time_file, "uint32 nanosec")?;

        let geometry_pkg_dir = temp_dir.path().join("geometry_msgs");
        let geometry_msg_dir = geometry_pkg_dir.join("msg");
        fs::create_dir_all(&geometry_msg_dir)?;
        let mut quat_file = File::create(geometry_msg_dir.join("Quaternion.msg"))?;
        writeln!(quat_file, "float64 x")?;
        writeln!(quat_file, "float64 y")?;
        writeln!(quat_file, "float64 z")?;
        writeln!(quat_file, "float64 w")?;

        let sensor_pkg_dir = temp_dir.path().join("sensor_msgs");
        let sensor_msg_dir = sensor_pkg_dir.join("msg");
        fs::create_dir_all(&sensor_msg_dir)?;
        let mut sensor_file = File::create(sensor_msg_dir.join("Imu.msg"))?;
        writeln!(sensor_file, "std_msgs/Header header")?;
        writeln!(sensor_file, "geometry_msgs/Quaternion orientation")?;
        writeln!(sensor_file, "float64[9] orientation_covariance")?;

        let output_dir = workspace_root.join("ros2_msgs");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let sensor_manifest = fs::read_to_string(output_dir.join("sensor_msgs/Cargo.toml"))?;
        assert!(sensor_manifest.contains("\"geometry_msgs/serde\""));
        assert!(sensor_manifest.contains("\"std_msgs/serde\""));

        let status = Command::new("cargo")
            .arg("check")
            .arg("-p")
            .arg("app")
            .current_dir(&workspace_root)
            .env("CARGO_TARGET_DIR", workspace_root.join("target"))
            .status()?;
        assert!(status.success());

        Ok(())
    }

    #[test]
    fn generated_srv_imports_same_package_msg_types() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let workspace_root = temp_dir.path().join("workspace");
        let app_dir = workspace_root.join("crates/app");
        fs::create_dir_all(app_dir.join("src"))?;
        fs::write(
            workspace_root.join("Cargo.toml"),
            "[workspace]\nmembers = [\n    \"crates/app\",\n]\nresolver = \"2\"\n",
        )?;
        fs::write(
            app_dir.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nlifecycle_msgs = { path = \"../../ros2_msgs/lifecycle_msgs\", features = [\"serde\"] }\n",
        )?;
        fs::write(
            app_dir.join("src/main.rs"),
            "use lifecycle_msgs::srv::ChangeStateRequest;\n\nfn main() {\n    let _ = core::mem::size_of::<ChangeStateRequest>();\n}\n",
        )?;

        let pkg_dir = temp_dir.path().join("lifecycle_msgs");
        let msg_dir = pkg_dir.join("msg");
        let srv_dir = pkg_dir.join("srv");
        fs::create_dir_all(&msg_dir)?;
        fs::create_dir_all(&srv_dir)?;
        fs::write(msg_dir.join("Transition.msg"), "uint8 id\nstring label\n")?;
        fs::write(
            srv_dir.join("ChangeState.srv"),
            "Transition transition\n---\nbool success\n",
        )?;

        let output_dir = workspace_root.join("ros2_msgs");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let status = Command::new("cargo")
            .arg("check")
            .arg("-p")
            .arg("app")
            .current_dir(&workspace_root)
            .env("CARGO_TARGET_DIR", workspace_root.join("target"))
            .status()?;
        assert!(status.success());

        Ok(())
    }

    #[test]
    fn generated_string_message_uses_std_string_for_fields()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let pkg_dir = temp_dir.path().join("std_msgs");
        let msg_dir = pkg_dir.join("msg");
        fs::create_dir_all(&msg_dir)?;
        fs::write(msg_dir.join("String.msg"), "string data\n")?;

        let output_dir = temp_dir.path().join("generated");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let msg_content = fs::read_to_string(output_dir.join("std_msgs/src/msg.rs"))?;
        let decode_content = fs::read_to_string(output_dir.join("std_msgs/src/decode.rs"))?;
        assert!(msg_content.contains("pub struct String"));
        assert!(msg_content.contains("pub data: std::string::String,"));
        assert!(decode_content.contains("Result<Self, std::string::String>"));

        Ok(())
    }

    #[test]
    fn generates_request_response_for_srv() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let pkg_dir = temp_dir.path().join("example_interfaces");
        let srv_dir = pkg_dir.join("srv");
        fs::create_dir_all(&srv_dir)?;

        let srv_file = srv_dir.join("AddTwoInts.srv");
        let mut file = File::create(&srv_file)?;
        writeln!(file, "int64 a")?;
        writeln!(file, "int64 b")?;
        writeln!(file, "---")?;
        writeln!(file, "int64 sum")?;

        let output_dir = temp_dir.path().join("generated_srv");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let single_dir = output_dir.join("example_interfaces");
        let srv_content = fs::read_to_string(single_dir.join("src/srv.rs"))?;
        assert!(srv_content.contains("pub struct AddTwoIntsRequest"));
        assert!(srv_content.contains("pub struct AddTwoIntsResponse"));

        Ok(())
    }

    #[test]
    fn skips_generated_request_response_msgs_when_srv_exists()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let pkg_dir = temp_dir.path().join("example_interfaces");
        let msg_dir = pkg_dir.join("msg");
        let srv_dir = pkg_dir.join("srv");
        fs::create_dir_all(&msg_dir)?;
        fs::create_dir_all(&srv_dir)?;

        fs::write(msg_dir.join("AddTwoInts_Request.msg"), "int64 a\nint64 b\n")?;
        fs::write(msg_dir.join("AddTwoInts_Response.msg"), "int64 sum\n")?;
        fs::write(
            srv_dir.join("AddTwoInts.srv"),
            "int64 a\nint64 b\n---\nint64 sum\n",
        )?;

        let output_dir = temp_dir.path().join("generated");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let msg_content = fs::read_to_string(output_dir.join("example_interfaces/src/msg.rs"))?;
        let srv_content = fs::read_to_string(output_dir.join("example_interfaces/src/srv.rs"))?;

        assert!(!msg_content.contains("pub struct AddTwoIntsRequest"));
        assert!(!msg_content.contains("pub struct AddTwoIntsResponse"));
        assert!(srv_content.contains("pub struct AddTwoIntsRequest"));
        assert!(srv_content.contains("pub struct AddTwoIntsResponse"));

        Ok(())
    }

    #[test]
    fn parses_srv_constants_as_constants_not_fields() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = tempdir()?;
        let pkg_dir = temp_dir.path().join("slam_toolbox");
        let srv_dir = pkg_dir.join("srv");
        fs::create_dir_all(&srv_dir)?;
        fs::write(
            srv_dir.join("LoopClosure.srv"),
            "int8 UNSET=0\nint8 START_AT_FIRST_NODE = 1\n---\nbool success\n",
        )?;

        let output_dir = temp_dir.path().join("generated");
        let generator = MessageGenerator::new(output_dir.to_string_lossy().to_string());
        generator.generate_from_directory(temp_dir.path().to_str().ok_or("invalid temp dir")?)?;

        let srv_content = fs::read_to_string(output_dir.join("slam_toolbox/src/srv.rs"))?;
        assert!(srv_content.contains("pub const UNSET: i8 = 0;"));
        assert!(srv_content.contains("pub const START_AT_FIRST_NODE: i8 = 1;"));
        assert!(!srv_content.contains("pub UNSET: i8,"));
        assert!(!srv_content.contains("pub START_AT_FIRST_NODE: i8,"));

        Ok(())
    }
}
