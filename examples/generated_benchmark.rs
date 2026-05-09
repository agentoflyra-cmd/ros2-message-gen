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
        "generated,{label},encode,{iterations},{total_bytes},{:.3},{:.3}",
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
        "generated,{label},decode_owned,{iterations},{total_bytes},{:.3},{:.3}",
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
        "generated,{label},decode_borrowed,{iterations},{total_bytes},{:.3},{:.3}",
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
        "generated,{label},decode_borrowed_to_owned,{iterations},{total_bytes},{:.3},{:.3}",
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

    println!("implementation,endianness,operation,iterations,total_bytes,ns_per_iter,mib_per_s");
    println!("generated,payload,little,0,{},0,0", little_endian.len());
    println!("generated,payload,big,0,{},0,0", big_endian.len());
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

const CYCLONE_IDL: &str = r#"module benchmark {
  module builtin_interfaces {
    struct Time {
      long sec;
      unsigned long nanosec;
    };
  };

  module std_msgs {
    struct Header {
      benchmark::builtin_interfaces::Time stamp;
      string frame_id;
    };
  };

  module geometry_msgs {
    struct Quaternion {
      double x;
      double y;
      double z;
      double w;
    };

    struct Vector3 {
      double x;
      double y;
      double z;
    };
  };

  module sensor_msgs {
    struct Imu {
      benchmark::std_msgs::Header header;
      benchmark::geometry_msgs::Quaternion orientation;
      double orientation_covariance[9];
      benchmark::geometry_msgs::Vector3 angular_velocity;
      double angular_velocity_covariance[9];
      benchmark::geometry_msgs::Vector3 linear_acceleration;
      double linear_acceleration_covariance[9];
      string label;
      sequence<octet> raw_bytes;
      sequence<float> samples;
    };
  };
};
"#;

const CYCLONE_BENCH_C: &str = r#"#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "dds/ddsi/ddsi_cdrstream.h"
#include "benchmark.h"

static benchmark_sensor_msgs_Imu sample_message(void) {
  benchmark_sensor_msgs_Imu msg;
  memset(&msg, 0, sizeof(msg));

  msg.header.stamp.sec = 1717171717;
  msg.header.stamp.nanosec = 123456789;
  msg.header.frame_id = strdup("base_linkbase_linkbase_linkbase_link");

  msg.orientation.x = 0.1;
  msg.orientation.y = 0.2;
  msg.orientation.z = 0.3;
  msg.orientation.w = 0.4;
  for (uint32_t i = 0; i < 9; i++) {
    msg.orientation_covariance[i] = 0.01;
    msg.angular_velocity_covariance[i] = 0.02;
    msg.linear_acceleration_covariance[i] = 0.03;
  }

  msg.angular_velocity.x = 1.0;
  msg.angular_velocity.y = 2.0;
  msg.angular_velocity.z = 3.0;
  msg.linear_acceleration.x = 4.0;
  msg.linear_acceleration.y = 5.0;
  msg.linear_acceleration.z = 6.0;

  msg.label = strdup("imu/frontimu/frontimu/front");

  msg.raw_bytes._length = 2048;
  msg.raw_bytes._maximum = 2048;
  msg.raw_bytes._release = true;
  msg.raw_bytes._buffer = dds_sequence_octet_allocbuf(2048);
  for (uint32_t i = 0; i < 2048; i++) {
    msg.raw_bytes._buffer[i] = (uint8_t)(i % 251);
  }

  msg.samples._length = 512;
  msg.samples._maximum = 512;
  msg.samples._release = true;
  msg.samples._buffer = dds_sequence_float_allocbuf(512);
  for (uint32_t i = 0; i < 512; i++) {
    msg.samples._buffer[i] = (float)i * 0.5f;
  }

  return msg;
}

static uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return ((uint64_t)ts.tv_sec * 1000000000ull) + (uint64_t)ts.tv_nsec;
}

static void run_encode(uint64_t iterations, const char *label, const benchmark_sensor_msgs_Imu *message, bool little_endian) {
  uint64_t started = now_ns();
  size_t total_bytes = 0;
  for (uint64_t i = 0; i < iterations; i++) {
    if (little_endian) {
      dds_ostreamLE_t os;
      dds_ostreamLE_init(&os, 8192, 1);
      dds_stream_writeLE(&os, (const char *) message, benchmark_sensor_msgs_Imu_desc.m_ops);
      total_bytes += os.x.m_index;
      dds_ostreamLE_fini(&os);
    } else {
      dds_ostreamBE_t os;
      dds_ostreamBE_init(&os, 8192, 1);
      dds_stream_writeBE(&os, (const char *) message, benchmark_sensor_msgs_Imu_desc.m_ops);
      total_bytes += os.x.m_index;
      dds_ostreamBE_fini(&os);
    }
  }
  uint64_t elapsed = now_ns() - started;
  double ns_per_iter = (double) elapsed / (double) iterations;
  double mib_per_s = (double) total_bytes / ((double) elapsed / 1000000000.0) / (1024.0 * 1024.0);
  printf("cyclonedds,%s,encode,%llu,%zu,%.3f,%.3f\n", label, (unsigned long long) iterations, total_bytes, ns_per_iter, mib_per_s);
}

static void encode_once(const benchmark_sensor_msgs_Imu *message, bool little_endian, unsigned char **buffer, uint32_t *size) {
  if (little_endian) {
    dds_ostreamLE_t os;
    dds_ostreamLE_init(&os, 8192, 1);
    dds_stream_writeLE(&os, (const char *) message, benchmark_sensor_msgs_Imu_desc.m_ops);
    *size = os.x.m_index;
    *buffer = malloc(*size);
    memcpy(*buffer, os.x.m_buffer, *size);
    dds_ostreamLE_fini(&os);
  } else {
    dds_ostreamBE_t os;
    dds_ostreamBE_init(&os, 8192, 1);
    dds_stream_writeBE(&os, (const char *) message, benchmark_sensor_msgs_Imu_desc.m_ops);
    *size = os.x.m_index;
    *buffer = malloc(*size);
    memcpy(*buffer, os.x.m_buffer, *size);
    dds_ostreamBE_fini(&os);
  }
}

static void run_decode_owned(uint64_t iterations, const char *label, const unsigned char *buffer, uint32_t size, bool needs_normalize) {
  uint64_t started = now_ns();
  size_t total_bytes = 0;
  for (uint64_t i = 0; i < iterations; i++) {
    benchmark_sensor_msgs_Imu decoded;
    memset(&decoded, 0, sizeof(decoded));
    unsigned char *decode_buffer = (unsigned char *) buffer;
    if (needs_normalize) {
      decode_buffer = malloc(size);
      memcpy(decode_buffer, buffer, size);
      uint32_t off = 0;
      const uint32_t *normalized = dds_stream_normalize_data(
        (char *) decode_buffer, &off, size, true, 1, benchmark_sensor_msgs_Imu_desc.m_ops
      );
      if (normalized == NULL || off != size) {
        fprintf(stderr, "cyclonedds normalize failed\n");
        abort();
      }
    }
    dds_istream_t is;
    dds_istream_init(&is, size, decode_buffer, 1);
    dds_stream_read(&is, (char *) &decoded, benchmark_sensor_msgs_Imu_desc.m_ops);
    total_bytes += size;
    dds_stream_free_sample(&decoded, benchmark_sensor_msgs_Imu_desc.m_ops);
    dds_istream_fini(&is);
    if (needs_normalize) {
      free(decode_buffer);
    }
  }
  uint64_t elapsed = now_ns() - started;
  double ns_per_iter = (double) elapsed / (double) iterations;
  double mib_per_s = (double) total_bytes / ((double) elapsed / 1000000000.0) / (1024.0 * 1024.0);
  printf("cyclonedds,%s,decode_owned,%llu,%zu,%.3f,%.3f\n", label, (unsigned long long) iterations, total_bytes, ns_per_iter, mib_per_s);
}

int main(int argc, char **argv) {
  uint64_t iterations = 20000;
  if (argc > 1) {
    iterations = strtoull(argv[1], NULL, 10);
  }

  benchmark_sensor_msgs_Imu message = sample_message();
  unsigned char *little = NULL;
  unsigned char *big = NULL;
  uint32_t little_size = 0;
  uint32_t big_size = 0;

  encode_once(&message, true, &little, &little_size);
  encode_once(&message, false, &big, &big_size);

  printf("cyclonedds,payload,little,0,%u,0,0\n", little_size);
  printf("cyclonedds,payload,big,0,%u,0,0\n", big_size);
  run_encode(iterations, "little", &message, true);
  run_decode_owned(iterations, "little", little, little_size, false);
  run_encode(iterations, "big", &message, false);
  run_decode_owned(iterations, "big", big, big_size, true);

  free(little);
  free(big);
  benchmark_sensor_msgs_Imu_free(&message, DDS_FREE_CONTENTS);
  return 0;
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

    if let Some(cyclonedds_home) = env::var_os("CYCLONEDDS_HOME") {
        let cyclone_status =
            run_cyclonedds_benchmark(&workspace_dir, Path::new(&cyclonedds_home), iterations)?;
        if !cyclone_status.success() {
            return Err(format!("cyclonedds benchmark failed with status {cyclone_status}").into());
        }
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

fn run_cyclonedds_benchmark(
    workspace_dir: &Path,
    cyclonedds_home: &Path,
    iterations: u64,
) -> Result<ExitStatus, Box<dyn std::error::Error>> {
    let cyclone_dir = workspace_dir.join("cyclonedds-bench");
    let generated_dir = cyclone_dir.join("generated");
    fs::create_dir_all(&generated_dir)?;
    fs::write(cyclone_dir.join("benchmark.idl"), CYCLONE_IDL)?;
    fs::write(cyclone_dir.join("bench.c"), CYCLONE_BENCH_C)?;

    let idlc_status = Command::new(cyclonedds_home.join("bin/idlc"))
        .arg("-l")
        .arg("c")
        .arg("-x")
        .arg("final")
        .arg("-o")
        .arg(&generated_dir)
        .arg(cyclone_dir.join("benchmark.idl"))
        .current_dir(&cyclone_dir)
        .status()?;
    if !idlc_status.success() {
        return Err(format!("idlc failed with status {idlc_status}").into());
    }

    let binary = cyclone_dir.join("cyclone-bench");
    let lib_dir = cyclonedds_home.join("lib");
    let include_dir = cyclonedds_home.join("include");
    let cc_status = Command::new("cc")
        .arg("-std=gnu11")
        .arg("-O3")
        .arg("-pthread")
        .arg("-I")
        .arg(&include_dir)
        .arg("-I")
        .arg(&generated_dir)
        .arg(cyclone_dir.join("bench.c"))
        .arg(generated_dir.join("benchmark.c"))
        .arg("-L")
        .arg(&lib_dir)
        .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
        .arg("-lddsc")
        .arg("-o")
        .arg(&binary)
        .current_dir(&cyclone_dir)
        .status()?;
    if !cc_status.success() {
        return Err(format!("cc failed with status {cc_status}").into());
    }

    Ok(Command::new(binary)
        .arg(iterations.to_string())
        .current_dir(&cyclone_dir)
        .status()?)
}
