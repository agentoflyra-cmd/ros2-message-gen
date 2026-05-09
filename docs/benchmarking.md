# Benchmarking

This project includes a benchmark harness for measuring the performance of generated
encode and decode code before and after runtime optimizations.

## Goal

The benchmark is intended to measure generated code, not only helper functions in
this repository.

The harness does the following:

1. creates a temporary ROS interface fixture
2. generates Rust crates with `ros2-message-gen`
3. creates a temporary benchmark app that depends on those generated crates
4. builds the benchmark app in release mode
5. runs repeated encode and decode loops on a representative generated `sensor_msgs::msg::Imu`

This gives a reproducible baseline for validating changes in the generated runtime.

## Run

```bash
cargo run --release --example generated_benchmark
```

You can override the loop count:

```bash
cargo run --release --example generated_benchmark -- --iterations 50000
```

## Output

The benchmark prints CSV-like rows for both little-endian and big-endian cases,
including owned and borrowed decode variants:

```text
endianness,operation,iterations,total_bytes,ns_per_iter,mib_per_s
payload_size,little,3120
payload_size,big,3120
little,encode,20000,62400000,....
little,decode_owned,20000,62400000,....
little,decode_borrowed,20000,62400000,....
little,decode_borrowed_to_owned,20000,62400000,....
big,encode,20000,62400000,....
big,decode_owned,20000,62400000,....
big,decode_borrowed,20000,62400000,....
big,decode_borrowed_to_owned,20000,62400000,....
```

Fields:

- `endianness`: `little` or `big`
- `iterations`: loop count
- `total_bytes`: aggregate processed payload bytes
- `ns_per_iter`: average latency per operation
- `mib_per_s`: approximate throughput

## Notes

- The fixture intentionally includes nested structs, fixed-size arrays, `string`,
  `uint8[]`, `float32[]`, and fixed-size numeric arrays.
- This benchmark is best used for relative comparison across revisions on the same machine.
- First-run compile time is not part of the benchmark numbers; only the inner app's
  encode/decode loops are timed.

## Reading The Results

The decode lines are meant to answer three different questions:

- `decode_owned`: how fast the classic generated owned decode path is
- `decode_borrowed`: how fast the borrowed lifetime-preserving decode path is
- `decode_borrowed_to_owned`: the cost of decoding borrowed first and converting
  back into the normal owned generated type

For this project, the last line is important because it measures whether the
borrowed model is only useful for fully borrowed pipelines, or whether it still
helps when a downstream stage eventually needs an owned message.
