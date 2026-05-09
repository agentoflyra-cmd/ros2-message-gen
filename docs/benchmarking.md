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
including owned and borrowed decode variants. When `CYCLONEDDS_HOME` points to
an installed Cyclone DDS tree, it also builds a temporary Cyclone benchmark and
prints a direct comparison for the same message shape.

```text
implementation,endianness,operation,iterations,total_bytes,ns_per_iter,mib_per_s
generated,payload,little,0,4492,0,0
generated,payload,big,0,4492,0,0
generated,little,encode,20000,89840000,....
generated,little,decode_owned,20000,89840000,....
generated,little,decode_borrowed,20000,89840000,....
generated,little,decode_borrowed_to_owned,20000,89840000,....
generated,big,encode,20000,89840000,....
generated,big,decode_owned,20000,89840000,....
generated,big,decode_borrowed,20000,89840000,....
generated,big,decode_borrowed_to_owned,20000,89840000,....
cyclonedds,payload,little,0,4492,0,0
cyclonedds,payload,big,0,4492,0,0
cyclonedds,little,encode,20000,89840000,....
cyclonedds,little,decode_owned,20000,89840000,....
cyclonedds,big,encode,20000,89840000,....
cyclonedds,big,decode_owned,20000,89840000,....
```

Fields:

- `implementation`: `generated` or `cyclonedds`
- `endianness`: `little` or `big`
- `operation`: `payload`, `encode`, `decode_owned`, `decode_borrowed`, or
  `decode_borrowed_to_owned`
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

## Cyclone DDS Comparison

If Cyclone DDS is installed locally, set `CYCLONEDDS_HOME` before running the
benchmark:

```bash
export CYCLONEDDS_HOME=/path/to/cyclonedds/install
cargo run --release --example generated_benchmark -- --iterations 50000
```

The harness will:

1. generate the Rust benchmark app as usual
2. generate an equivalent IDL for Cyclone DDS
3. run `idlc` to emit C type support
4. compile a small C benchmark against `libddsc`
5. print both `generated` and `cyclonedds` results in the same output stream

The Cyclone path intentionally uses the generated `m_ops` stream API rather than
participant/topic/reader/writer entities. That keeps the comparison focused on
sample-to-CDR and CDR-to-sample cost instead of discovery or transport overhead.

You may notice a small payload-size mismatch between `generated` and
`cyclonedds`. In the current setup, the generated Rust runtime reports the CDR
payload including the 4-byte encapsulation header, while the Cyclone low-level
stream helper reports the raw stream body size. Compare the timings directly,
but don't over-interpret the `payload` rows across implementations.
