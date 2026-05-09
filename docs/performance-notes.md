# Performance Notes

This document records the current performance assessment of the generated code and
the CDR runtime, with emphasis on whether a zero-copy approach is appropriate.

## Summary

There is clear room for optimization, but `zerocopy` is not the primary lever for
this project in the "derive `FromBytes` for everything" sense.

The current cost profile is dominated by:

- owned allocation during decode for `String` and variable-length byte sequences
- per-element decode/encode for arrays and sequences
- temporary `Vec<u8>` allocation during encode for primitive values

Because ROS 2 CDR payloads contain alignment, endianness, variable-length fields,
and nested message composition, a whole-message `zerocopy` design would only
apply to a narrow subset of types.

## Current Hotspots

### Decode path

- `CdrDecoder::read_string` copies into a new `String` via `bytes.to_vec()`
- `CdrDecoder::read_octet_seq` copies into an owned `Vec<u8>`
- `CdrDecoder::read_array` builds a temporary `Vec<T>` before converting to `[T; N]`
- generated decode code always uses generic per-element `read_seq::<T>()` for dynamic arrays

### Encode path

- primitive writers call `to_vec()` on byte arrays before appending to the output buffer
- `write_string` copies `value.as_bytes()` into a temporary `Vec<u8>`
- `write_bytes_raw` currently takes ownership of a `Vec<u8>`, encouraging avoidable allocation

## Why `zerocopy` Is Not The Main Optimization

`zerocopy` is a good fit for POD-like layouts with:

- fixed-size fields
- stable in-memory layout
- no owned strings
- no variable-length sequences
- minimal endianness handling at field boundaries

The generated ROS 2 message model does not generally have those properties.
Typical generated types contain `String`, `Vec<T>`, nested messages, and fields that
must respect CDR alignment rules during traversal.

As a result:

- full-message `zerocopy` is not realistic for the general case
- partial `zerocopy` may still help for small internal helpers or tightly scoped primitive blocks
- a borrowed decode model is likely to provide better return on complexity

That borrowed decode model is now implemented and should be considered the practical
form of "zerocopy" for this project.

## Recommended Optimization Order

### 1. Remove avoidable encode allocations

Highest-confidence, lowest-risk changes:

- change `write_bytes_raw` to accept `&[u8]`
- replace primitive `to_vec()` calls with direct `extend_from_slice`
- make `write_string` write borrowed bytes directly

### 2. Add generated fast paths for byte arrays

Special-case ROS `byte[]` and `uint8[]`:

- decode with `read_octet_seq`
- encode with a dedicated byte-sequence writer

This should help common bag-processing workloads significantly.

### 3. Improve fixed-array decoding

Avoid the current `Vec<T> -> [T; N]` conversion path for fixed-size arrays when possible.

### 4. Add borrowed decode support

Instead of forcing every decoded message to be fully owned, add a borrowed view layer such as:

- `&'a [u8]` for byte payloads
- `Cow<'a, str>` or borrowed string views where valid
- generated `FooBorrowed<'a>` message variants

This is the most promising path toward practical near-zero-copy processing for offline
rosbag and MCAP workflows.

### 5. Re-evaluate limited `zerocopy` usage

Only after the above should `zerocopy` be considered, and then only for narrow cases:

- primitive fixed-layout helper types
- carefully validated contiguous numeric blocks

It should not drive the main generated message model unless the project scope narrows
to a much more restricted subset of ROS message shapes.

## Optimization Timeline

The benchmark fixture evolved alongside the implementation, but the numbers below
capture the main progression on the same representative generated `Imu` payload.

### Early baseline

This was the first generated benchmark run before the specialization and borrowed
decode work:

```text
encode,1000,4492000,5843.447,733.113
decode,1000,4492000,4213.239,1016.772
```

This baseline approximates the earlier manually assembled / fully owned decode path
before the runtime and generator were specialized.

### Runtime allocation cleanup and byte fast path

After removing avoidable encode allocations and specializing `uint8[]` / `byte[]`:

```text
encode,1000,4492000,1208.809,3543.906
decode,1000,4492000,987.523,4338.031
```

### Primitive specialization and SIMD fast paths

After adding primitive sequence/array specialization plus x86 and ARM-oriented SIMD
copy / byteswap paths:

```text
little,encode,1000,4492000,191.511,22368.976
little,decode_owned,1000,4492000,181.520,23600.182
big,encode,1000,4492000,214.921,19932.464
big,decode_owned,1000,4492000,279.607,15321.165
```

### Borrowed decode introduction

After introducing borrowed message views for `string`, `byte[]`, nested messages,
dynamic primitive sequences, and fixed primitive arrays:

```text
little,decode_owned,1000,4492000,199.818,21439.035
little,decode_borrowed,1000,4492000,84.288,50824.614
little,decode_borrowed_to_owned,1000,4492000,202.064,21200.734
big,decode_owned,1000,4492000,270.516,15836.050
big,decode_borrowed,1000,4492000,92.547,46288.967
big,decode_borrowed_to_owned,1000,4492000,232.337,18438.325
```

The important takeaway is not just that borrowed decode is faster, but that
`borrowed -> to_owned()` is now close to, or better than, the older direct owned
decode path on the benchmark fixture.

### Cyclone DDS comparison

The benchmark harness can also compare the generated runtime against Cyclone DDS
when `CYCLONEDDS_HOME` is set.

This comparison intentionally uses Cyclone DDS' generated low-level `m_ops`
stream API instead of participant/topic/reader/writer entities, so the numbers
reflect sample-to-CDR and CDR-to-sample cost rather than discovery or transport.

On the current fixture, a representative run produced:

```text
generated,little,encode,100,449200,210.430,20357.863
generated,little,decode_owned,100,449200,218.070,19644.633
generated,little,decode_borrowed,100,449200,89.970,47614.816
generated,big,encode,100,449200,233.390,18355.135
generated,big,decode_owned,100,449200,296.400,14453.121
generated,big,decode_borrowed,100,449200,121.200,35345.751
cyclonedds,little,encode,100,448800,131.140,32637.565
cyclonedds,little,decode_owned,100,448800,250.080,17114.885
cyclonedds,big,encode,100,448800,249.400,17161.549
cyclonedds,big,decode_owned,100,448800,504.240,8488.201
```

The current read of those numbers is:

- Cyclone DDS is faster on the little-endian encode path for this fixture.
- The generated owned decode path is faster than Cyclone DDS on both little-endian
  and big-endian decode.
- The generated borrowed decode path is materially faster than both owned paths.
- Big-endian decode is currently a particularly strong case for the generated
  runtime relative to the Cyclone low-level stream path.

There is also a small payload-size mismatch in the printed `payload` rows:

- the generated Rust runtime reports the payload including the 4-byte
  encapsulation header
- the Cyclone DDS stream helper reports only the raw stream body size

That difference does not affect the latency comparison, but it means `payload`
rows should not be compared too literally across implementations.
