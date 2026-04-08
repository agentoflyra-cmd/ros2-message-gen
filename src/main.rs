use ros2_message_gen::{GeneratorConfig, MessageGenerator, StructNameStyle};
use std::env;
use std::process;

fn print_usage(program: &str) {
    eprintln!("ROS2 Message Generator for Rust");
    eprintln!();
    eprintln!("USAGE:");
    eprintln!("  {} [OPTIONS] <output_dir>", program);
    eprintln!();
    eprintln!("OPTIONS:");
    eprintln!("  -d, --dir <dir>        Generate from specific directory");
    eprintln!("  -e, --env <var>        Generate from environment variable");
    eprintln!("  -r, --ros-env          Auto-detect ROS environment variables");
    eprintln!("  --snake-case           Use snake_case struct names");
    eprintln!("  --no-msg-suffix         Don't include /msg/ in type names");
    eprintln!("  -h, --help            Show this help message");
    eprintln!();
    eprintln!("EXAMPLES:");
    eprintln!(
        "  {} -d /mnt/ubuntu/opt/ros/humble/share generated_pkg",
        program
    );
    eprintln!("  {} -e AMENT_PREFIX_PATH generated_pkg", program);
    eprintln!("  {} -r generated_pkg", program);
    eprintln!(
        "  {} --snake-case --no-msg-suffix -d /mnt/ubuntu/opt/ros/humble/share custom_pkg",
        program
    );
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args.contains(&"-h".to_string()) || args.contains(&"--help".to_string()) {
        print_usage(&args[0]);
        process::exit(0);
    }

    let mut output_dir = String::new();
    let mut source_dir = None;
    let mut env_var = None;
    let mut use_ros_env = false;
    let mut config = GeneratorConfig::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--dir" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --dir requires a directory argument");
                    process::exit(1);
                }
                source_dir = Some(args[i + 1].clone());
                i += 2;
            }
            "-e" | "--env" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --env requires a variable name");
                    process::exit(1);
                }
                env_var = Some(args[i + 1].clone());
                i += 2;
            }
            "-r" | "--ros-env" => {
                use_ros_env = true;
                i += 1;
            }
            "--snake-case" => {
                config = config.with_struct_name_style(StructNameStyle::SnakeCase);
                i += 1;
            }
            "--no-msg-suffix" => {
                config = config.with_include_msg_suffix(false);
                i += 1;
            }
            arg if arg.starts_with('-') => {
                eprintln!("Error: Unknown option {}", arg);
                print_usage(&args[0]);
                process::exit(1);
            }
            arg => {
                if output_dir.is_empty() {
                    output_dir = arg.to_string();
                } else {
                    eprintln!("Error: Multiple output directories specified");
                    print_usage(&args[0]);
                    process::exit(1);
                }
                i += 1;
            }
        }
    }

    if output_dir.is_empty() {
        eprintln!("Error: Output directory not specified");
        print_usage(&args[0]);
        process::exit(1);
    }

    let generator = MessageGenerator::with_config(output_dir, config);

    let result = if let Some(dir) = source_dir {
        println!("Generating messages from directory: {}", dir);
        generator.generate_from_directory(&dir)
    } else if let Some(var) = env_var {
        println!("Generating messages from environment variable: {}", var);
        generator.generate_from_env(&var)
    } else if use_ros_env {
        println!("Auto-detecting ROS environment variables...");
        generator.generate_from_ros_env()
    } else {
        eprintln!("Error: No source specified. Use --dir, --env, or --ros-env");
        print_usage(&args[0]);
        process::exit(1);
    };

    match result {
        Ok(_) => {
            println!("Successfully generated Rust message definitions!");
        }
        Err(e) => {
            eprintln!("Error generating messages: {}", e);
            process::exit(1);
        }
    }
}
