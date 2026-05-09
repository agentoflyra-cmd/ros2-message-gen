use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

use tempfile::tempdir;

const BENCH_APP_MAIN: &str = r#"use std::hint::black_box;
use std::time::Instant;

use cdr_runtime::{BorrowDecodeCdr, CdrDecoder, CdrEncoding, CdrRepresentation, Endianness};
use sensor_msgs::borrow_decode::borrow_decode_from_bytes;
use sensor_msgs::borrowed::Imu as BorrowedImu;
use sensor_msgs::decode::{decode_from_bytes, DecodeCdr};
use sensor_msgs::encode::{encode_to_vec, CdrEncoder, EncodeCdr};
use sensor_msgs::msg::Imu;

fn sample_message() -> Imu {
    Imu {
        header: std_msgs::msg::Header {
            stamp: builtin_interfaces::msg::Time {
                sec: 1_717_171_717,
                nanosec: 123_456_789,
            },
            frame_id: "base_link".repeat(4),
        },
        orientation: geometry_msgs::msg::Quaternion {
            x: 0.1,
            y: 0.2,
            z: 0.3,
            w: 0.4,
        },
        orientation_covariance: [0.01; 9],
        angular_velocity: geometry_msgs::msg::Vector3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        angular_velocity_covariance: [0.02; 9],
        linear_acceleration: geometry_msgs::msg::Vector3 {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        },
        linear_acceleration_covariance: [0.03; 9],
        label: "imu/front".repeat(3),
        raw_bytes: (0..2048).map(|i| (i % 251) as u8).collect(),
        samples: (0..512).map(|i| i as f32 * 0.5).collect(),
    }
}

fn encode_to_vec_big_endian(value: &Imu) -> Vec<u8> {
    let mut encoder = CdrEncoder::new(CdrEncoding {
        cdr_representation: CdrRepresentation::Xcdr1,
        endianness: Endianness::Big,
    });
    value.encode_cdr(&mut encoder).expect("big-endian encode should succeed");
    encoder.data_raw
}

fn decode_from_bytes_big_endian(bytes: &[u8]) -> Imu {
    let mut decoder = CdrDecoder::new(bytes).expect("big-endian decoder should be created");
    Imu::decode_cdr(&mut decoder).expect("big-endian decode should succeed")
}

fn borrow_decode_from_bytes_big_endian(bytes: &[u8]) -> BorrowedImu<'_> {
    let mut decoder = CdrDecoder::new(bytes).expect("big-endian decoder should be created");
    BorrowedImu::borrow_decode_cdr(&mut decoder).expect("big-endian borrowed decode should succeed")
}

fn run_encode(iterations: u64, label: &str, message: &Imu, encode_fn: fn(&Imu) -> Vec<u8>) {
    let started = Instant::now();
    let mut total_bytes = 0usize;
    for _ in 0..iterations {
        let encoded = black_box(encode_fn(black_box(message)));
        total_bytes += encoded.len();
        black_box(encoded);
    }
    let elapsed = started.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / iterations as f64;
    let mib_per_s = total_bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "{label},encode,{iterations},{total_bytes},{:.3},{:.3}",
        ns_per_iter, mib_per_s
    );
}

fn run_decode_owned(iterations: u64, label: &str, bytes: &[u8], decode_fn: fn(&[u8]) -> Imu) {
    let started = Instant::now();
    let mut total_bytes = 0usize;
    for _ in 0..iterations {
        let decoded: Imu = black_box(decode_fn(black_box(bytes)));
        total_bytes += bytes.len();
        black_box(decoded);
    }
    let elapsed = started.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / iterations as f64;
    let mib_per_s = total_bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "{label},decode_owned,{iterations},{total_bytes},{:.3},{:.3}",
        ns_per_iter, mib_per_s
    );
}

fn run_decode_borrowed(
    iterations: u64,
    label: &str,
    bytes: &[u8],
    decode_fn: for<'a> fn(&'a [u8]) -> BorrowedImu<'a>,
) {
    let started = Instant::now();
    let mut total_bytes = 0usize;
    for _ in 0..iterations {
        let decoded = black_box(decode_fn(black_box(bytes)));
        total_bytes += bytes.len();
        black_box(decoded);
    }
    let elapsed = started.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / iterations as f64;
    let mib_per_s = total_bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "{label},decode_borrowed,{iterations},{total_bytes},{:.3},{:.3}",
        ns_per_iter, mib_per_s
    );
}

fn run_decode_borrowed_to_owned(
    iterations: u64,
    label: &str,
    bytes: &[u8],
    decode_fn: for<'a> fn(&'a [u8]) -> BorrowedImu<'a>,
) {
    let started = Instant::now();
    let mut total_bytes = 0usize;
    for _ in 0..iterations {
        let decoded = black_box(decode_fn(black_box(bytes)));
        let owned = black_box(decoded.to_owned());
        total_bytes += bytes.len();
        black_box(owned);
    }
    let elapsed = started.elapsed();
    let ns_per_iter = elapsed.as_nanos() as f64 / iterations as f64;
    let mib_per_s = total_bytes as f64 / elapsed.as_secs_f64() / (1024.0 * 1024.0);
    println!(
        "{label},decode_borrowed_to_owned,{iterations},{total_bytes},{:.3},{:.3}",
        ns_per_iter, mib_per_s
    );
}

fn main() {
    let iterations = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(20_000);

    let message = sample_message();
    let little_endian = encode_to_vec(&message).expect("little-endian encode should succeed");
    let big_endian = encode_to_vec_big_endian(&message);

    println!("endianness,operation,iterations,total_bytes,ns_per_iter,mib_per_s");
    println!("payload_size,little,{}", little_endian.len());
    println!("payload_size,big,{}", big_endian.len());
    run_encode(iterations, "little", &message, |msg| {
        encode_to_vec(msg).expect("little-endian encode should succeed")
    });
    run_decode_owned(iterations, "little", &little_endian, |bytes| {
        decode_from_bytes::<Imu>(bytes).expect("little-endian decode should succeed")
    });
    run_decode_borrowed(iterations, "little", &little_endian, |bytes| {
        borrow_decode_from_bytes::<BorrowedImu<'_>>(bytes)
            .expect("little-endian borrowed decode should succeed")
    });
    run_decode_borrowed_to_owned(iterations, "little", &little_endian, |bytes| {
        borrow_decode_from_bytes::<BorrowedImu<'_>>(bytes)
            .expect("little-endian borrowed decode should succeed")
    });
    run_encode(iterations, "big", &message, encode_to_vec_big_endian);
    run_decode_owned(iterations, "big", &big_endian, decode_from_bytes_big_endian);
    run_decode_borrowed(iterations, "big", &big_endian, borrow_decode_from_bytes_big_endian);
    run_decode_borrowed_to_owned(
        iterations,
        "big",
        &big_endian,
        borrow_decode_from_bytes_big_endian,
    );
}
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let iterations = parse_iterations();
    let temp_dir = tempdir()?;
    let interfaces_dir = temp_dir.path().join("interfaces");
    let workspace_dir = temp_dir.path().join("workspace");
    let generated_dir = workspace_dir.join("generated");
    let bench_dir = workspace_dir.join("bench-app");

    create_fixture_interfaces(&interfaces_dir)?;
    fs::create_dir_all(&workspace_dir)?;
    fs::write(
        workspace_dir.join("Cargo.toml"),
        "[workspace]\nmembers = [\n    \"bench-app\",\n]\nresolver = \"2\"\n",
    )?;

    let generator = ros2_message_gen::MessageGenerator::new(generated_dir.display().to_string());
    generator.generate_from_directory(
        interfaces_dir
            .to_str()
            .ok_or("fixture interfaces path is not valid utf-8")?,
    )?;

    create_bench_app(&bench_dir)?;

    let status = run_benchmark_app(&workspace_dir, iterations)?;
    if !status.success() {
        return Err(format!("benchmark app failed with status {status}").into());
    }

    Ok(())
}

fn parse_iterations() -> u64 {
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--iterations" {
            if let Some(value) = args.next() {
                if let Ok(iterations) = value.parse::<u64>() {
                    return iterations;
                }
            }
        }
    }
    20_000
}

fn create_fixture_interfaces(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root)?;

    write_interface(
        root,
        "builtin_interfaces/msg/Time.msg",
        "int32 sec\nuint32 nanosec\n",
    )?;
    write_interface(
        root,
        "std_msgs/msg/Header.msg",
        "builtin_interfaces/Time stamp\nstring frame_id\n",
    )?;
    write_interface(
        root,
        "geometry_msgs/msg/Quaternion.msg",
        "float64 x\nfloat64 y\nfloat64 z\nfloat64 w\n",
    )?;
    write_interface(
        root,
        "geometry_msgs/msg/Vector3.msg",
        "float64 x\nfloat64 y\nfloat64 z\n",
    )?;
    write_interface(
        root,
        "sensor_msgs/msg/Imu.msg",
        concat!(
            "std_msgs/Header header\n",
            "geometry_msgs/Quaternion orientation\n",
            "float64[9] orientation_covariance\n",
            "geometry_msgs/Vector3 angular_velocity\n",
            "float64[9] angular_velocity_covariance\n",
            "geometry_msgs/Vector3 linear_acceleration\n",
            "float64[9] linear_acceleration_covariance\n",
            "string label\n",
            "uint8[] raw_bytes\n",
            "float32[] samples\n",
        ),
    )?;

    Ok(())
}

fn write_interface(
    root: &Path,
    relative_path: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = root.join(relative_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
}

fn create_bench_app(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        concat!(
            "[package]\n",
            "name = \"bench-app\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2024\"\n\n",
            "[dependencies]\n",
            "cdr-runtime = { path = \"../generated/cdr-runtime\" }\n",
            "builtin_interfaces = { path = \"../generated/builtin_interfaces\" }\n",
            "geometry_msgs = { path = \"../generated/geometry_msgs\" }\n",
            "sensor_msgs = { path = \"../generated/sensor_msgs\" }\n",
            "std_msgs = { path = \"../generated/std_msgs\" }\n",
        ),
    )?;
    fs::write(root.join("src/main.rs"), BENCH_APP_MAIN)?;
    Ok(())
}

fn run_benchmark_app(
    workspace_dir: &Path,
    iterations: u64,
) -> Result<ExitStatus, Box<dyn std::error::Error>> {
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("--offline")
        .arg("--release")
        .arg("-p")
        .arg("bench-app")
        .arg("--")
        .arg(iterations.to_string())
        .current_dir(workspace_dir)
        .env("CARGO_TARGET_DIR", workspace_dir.join("target"));

    Ok(command.status()?)
}
