use std::char;
use std::fmt;
use std::mem::size_of;
use std::ptr;

pub type CdrResult<T> = Result<T, CdrError>;

#[derive(Debug)]
pub enum CdrError {
    InputTooShort { context: &'static str, min_len: usize, actual_len: usize },
    LengthOverflow { context: &'static str },
    NotEnoughBytes { context: &'static str, requested: usize, remaining: usize },
    OddByteLength { context: &'static str, len: usize },
    InvalidRepresentation(u16),
    InvalidUtf8(std::string::FromUtf8Error),
    InvalidUtf8Slice(std::str::Utf8Error),
    InvalidUtf16(std::char::DecodeUtf16Error),
    InvalidUnicodeScalar { context: &'static str, value: u32 },
    InvalidBool(u8),
    NonAsciiChar(char),
    FixedArrayLength { expected: usize, actual: usize },
    UnknownSchema(String),
}

impl fmt::Display for CdrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooShort {
                context,
                min_len,
                actual_len,
            } => write!(
                f,
                "{context}: input too short, expected at least {min_len} bytes, got {actual_len}"
            ),
            Self::LengthOverflow { context } => write!(f, "{context}: length overflow"),
            Self::NotEnoughBytes {
                context,
                requested,
                remaining,
            } => write!(
                f,
                "{context}: not enough bytes, requested {requested}, remaining {remaining}"
            ),
            Self::OddByteLength { context, len } => {
                write!(f, "{context}: expected an even byte length, got {len}")
            }
            Self::InvalidRepresentation(value) => {
                write!(f, "CdrEncoding::parse: invalid representation value {value:#06x}")
            }
            Self::InvalidUtf8(err) => write!(f, "CdrDecoder::read_string: invalid utf8: {err}"),
            Self::InvalidUtf8Slice(err) => {
                write!(f, "CdrDecoder::read_str: invalid utf8: {err}")
            }
            Self::InvalidUtf16(err) => {
                write!(f, "CdrDecoder::read_wstring: invalid utf16: {err}")
            }
            Self::InvalidUnicodeScalar { context, value } => {
                write!(f, "{context}: invalid unicode scalar value {value:#x}")
            }
            Self::InvalidBool(value) => {
                write!(f, "CdrDecoder::read_bool: invalid boolean discriminant {value}")
            }
            Self::NonAsciiChar(value) => {
                write!(f, "CdrEncoder::write_char: non-ascii char {value:?}")
            }
            Self::FixedArrayLength { expected, actual } => write!(
                f,
                "CdrDecoder::read_array: failed to build fixed-size array, expected {expected} items, got {actual}"
            ),
            Self::UnknownSchema(schema_name) => write!(f, "unknown schema: {schema_name}"),
        }
    }
}

impl std::error::Error for CdrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidUtf8(err) => Some(err),
            Self::InvalidUtf8Slice(err) => Some(err),
            Self::InvalidUtf16(err) => Some(err),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdrRepresentation {
    Xcdr1,
    Xcdr2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endianness {
    Big,
    Little,
}

pub struct CdrDecoder<'a> {
    pub pos: usize,
    pub payload: &'a [u8],
    pub endianness: Endianness,
}

pub trait DecodeCdr: Sized {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self>;
}

pub trait BorrowDecodeCdr<'a>: Sized {
    fn borrow_decode_cdr(decoder: &mut CdrDecoder<'a>) -> CdrResult<Self>;
}

pub trait PrimitiveValue: Copy + Clone + fmt::Debug + PartialEq + PartialOrd + 'static {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self;
}

#[derive(Clone)]
pub struct PrimitiveSeq<'a, T> {
    bytes: &'a [u8],
    endianness: Endianness,
    _marker: std::marker::PhantomData<T>,
}

#[derive(Clone)]
pub struct PrimitiveArray<'a, T, const N: usize> {
    bytes: &'a [u8],
    endianness: Endianness,
    _marker: std::marker::PhantomData<T>,
}

impl<'a, T: PrimitiveValue> PrimitiveSeq<'a, T> {
    pub fn len(&self) -> usize {
        self.bytes.len() / size_of::<T>()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn iter(&self) -> PrimitiveSeqIter<'_, T> {
        PrimitiveSeqIter {
            chunks: self.bytes.chunks_exact(size_of::<T>()),
            endianness: self.endianness,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn to_vec(&self) -> Vec<T> {
        let len = self.len();
        let byte_len = self.bytes.len();
        let mut items = Vec::<T>::with_capacity(len);
        unsafe {
            items.set_len(len);
            copy_bytes_into_simd(self.bytes.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        if !native_endianness_matches(self.endianness) {
            let bytes =
                unsafe { std::slice::from_raw_parts_mut(items.as_mut_ptr() as *mut u8, byte_len) };
            swap_endian_in_place(bytes, size_of::<T>());
        }
        items
    }
}

impl<'a, T: PrimitiveValue, const N: usize> PrimitiveArray<'a, T, N> {
    pub fn len(&self) -> usize {
        N
    }

    pub fn is_empty(&self) -> bool {
        N == 0
    }

    pub fn iter(&self) -> PrimitiveSeqIter<'_, T> {
        PrimitiveSeqIter {
            chunks: self.bytes.chunks_exact(size_of::<T>()),
            endianness: self.endianness,
            _marker: std::marker::PhantomData,
        }
    }

    pub fn to_array(&self) -> [T; N] {
        let byte_len = self.bytes.len();
        let mut items = Vec::<T>::with_capacity(N);
        unsafe {
            items.set_len(N);
            copy_bytes_into_simd(self.bytes.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        if !native_endianness_matches(self.endianness) {
            let bytes =
                unsafe { std::slice::from_raw_parts_mut(items.as_mut_ptr() as *mut u8, byte_len) };
            swap_endian_in_place(bytes, size_of::<T>());
        }
        items.try_into().ok().expect("primitive array length should match")
    }
}

impl<T: PrimitiveValue> fmt::Debug for PrimitiveSeq<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PrimitiveValue, const N: usize> fmt::Debug for PrimitiveArray<'_, T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: PrimitiveValue> PartialEq for PrimitiveSeq<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<T: PrimitiveValue, const N: usize> PartialEq for PrimitiveArray<'_, T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<T: PrimitiveValue> PartialOrd for PrimitiveSeq<'_, T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

impl<T: PrimitiveValue, const N: usize> PartialOrd for PrimitiveArray<'_, T, N> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.iter().partial_cmp(other.iter())
    }
}

pub struct PrimitiveSeqIter<'a, T> {
    chunks: std::slice::ChunksExact<'a, u8>,
    endianness: Endianness,
    _marker: std::marker::PhantomData<T>,
}

impl<T: PrimitiveValue> Iterator for PrimitiveSeqIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.chunks
            .next()
            .map(|chunk| T::decode_chunk(chunk, self.endianness))
    }
}

pub struct CdrEncoder {
    pub data_raw: Vec<u8>,
    pub endianness: Endianness,
}

pub trait EncodeCdr: Sized {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar16(pub char);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar32(pub char);

pub struct CdrEncoding {
    pub cdr_representation: CdrRepresentation,
    pub endianness: Endianness,
}

fn native_endianness_matches(endianness: Endianness) -> bool {
    cfg!(target_endian = "little") && matches!(endianness, Endianness::Little)
        || cfg!(target_endian = "big") && matches!(endianness, Endianness::Big)
}

impl CdrEncoding {
    pub fn serialize(value: &Self) -> Vec<u8> {
        let rep = match (&value.cdr_representation, &value.endianness) {
            (CdrRepresentation::Xcdr1, Endianness::Big) => 0x0000u16,
            (CdrRepresentation::Xcdr1, Endianness::Little) => 0x0001u16,
            (CdrRepresentation::Xcdr2, Endianness::Big) => 0x0002u16,
            (CdrRepresentation::Xcdr2, Endianness::Little) => 0x0003u16,
        };

        let mut bytes = Vec::with_capacity(4);
        bytes.extend_from_slice(&rep.to_be_bytes());
        bytes.extend_from_slice(&0u16.to_be_bytes());
        bytes
    }

    pub fn parse(data: &[u8]) -> CdrResult<(Self, &[u8])> {
        if data.len() < 4 {
            return Err(CdrError::InputTooShort {
                context: "CdrEncoding::parse",
                min_len: 4,
                actual_len: data.len(),
            });
        }

        let rep = u16::from_be_bytes([data[0], data[1]]);
        let _options = u16::from_be_bytes([data[2], data[3]]);

        match rep {
            0x0000 => Ok((
                Self {
                    cdr_representation: CdrRepresentation::Xcdr1,
                    endianness: Endianness::Big,
                },
                &data[4..],
            )),
            0x0001 => Ok((
                Self {
                    cdr_representation: CdrRepresentation::Xcdr1,
                    endianness: Endianness::Little,
                },
                &data[4..],
            )),
            0x0002 => Ok((
                Self {
                    cdr_representation: CdrRepresentation::Xcdr2,
                    endianness: Endianness::Big,
                },
                &data[4..],
            )),
            0x0003 => Ok((
                Self {
                    cdr_representation: CdrRepresentation::Xcdr2,
                    endianness: Endianness::Little,
                },
                &data[4..],
            )),
            _ => Err(CdrError::InvalidRepresentation(rep)),
        }
    }
}

impl<'a> CdrDecoder<'a> {
    pub fn new(data: &'a [u8]) -> CdrResult<Self> {
        if data.len() < 4 {
            return Err(CdrError::InputTooShort {
                context: "CdrDecoder::new",
                min_len: 4,
                actual_len: data.len(),
            });
        }
        let (encoding, payload) = CdrEncoding::parse(data)?;
        Ok(Self {
            pos: 0,
            payload,
            endianness: encoding.endianness,
        })
    }

    fn align_to(&mut self, align_length: usize) {
        debug_assert!(align_length > 0);
        let padding_length = (align_length - (self.pos % align_length)) % align_length;
        self.pos += padding_length;
    }

    fn read_bytes_raw(&mut self, length: usize) -> CdrResult<&'a [u8]> {
        if length == 0 {
            return Ok(&[]);
        }

        let end = self
            .pos
            .checked_add(length)
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_bytes_raw",
            })?;

        if end > self.payload.len() {
            return Err(CdrError::NotEnoughBytes {
                context: "CdrDecoder::read_bytes_raw",
                requested: length,
                remaining: self.payload.len().saturating_sub(self.pos),
            });
        }

        let bytes = &self.payload[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    fn native_endianness_matches(&self) -> bool {
        native_endianness_matches(self.endianness)
    }

    fn read_native_primitive_seq<T: Copy>(&mut self) -> CdrResult<Vec<T>> {
        let length = self.read_u32()? as usize;
        self.align_to(size_of::<T>());
        let byte_len = length
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_native_primitive_seq",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        let mut items = Vec::<T>::with_capacity(length);
        unsafe {
            items.set_len(length);
            copy_bytes_into_simd(bytes.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        Ok(items)
    }

    fn read_swapped_primitive_seq<T: Copy>(&mut self) -> CdrResult<Vec<T>> {
        let length = self.read_u32()? as usize;
        self.align_to(size_of::<T>());
        let byte_len = length
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_swapped_primitive_seq",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        let mut swapped = bytes.to_vec();
        swap_endian_in_place(&mut swapped, size_of::<T>());
        let mut items = Vec::<T>::with_capacity(length);
        unsafe {
            items.set_len(length);
            copy_bytes_into_simd(swapped.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        Ok(items)
    }

    fn read_native_primitive_array<T: Copy, const N: usize>(&mut self) -> CdrResult<[T; N]> {
        self.align_to(size_of::<T>());
        let byte_len = N
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_native_primitive_array",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        let mut items = Vec::<T>::with_capacity(N);
        unsafe {
            items.set_len(N);
            copy_bytes_into_simd(bytes.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        items
            .try_into()
            .map_err(|items: Vec<T>| CdrError::FixedArrayLength {
                expected: N,
                actual: items.len(),
            })
    }

    fn read_swapped_primitive_array<T: Copy, const N: usize>(&mut self) -> CdrResult<[T; N]> {
        self.align_to(size_of::<T>());
        let byte_len = N
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_swapped_primitive_array",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        let mut swapped = bytes.to_vec();
        swap_endian_in_place(&mut swapped, size_of::<T>());
        let mut items = Vec::<T>::with_capacity(N);
        unsafe {
            items.set_len(N);
            copy_bytes_into_simd(swapped.as_ptr(), items.as_mut_ptr() as *mut u8, byte_len);
        }
        items
            .try_into()
            .map_err(|items: Vec<T>| CdrError::FixedArrayLength {
                expected: N,
                actual: items.len(),
            })
    }

    pub fn read_bytes(&mut self, length: usize) -> CdrResult<Vec<u8>> {
        let bytes = self.read_bytes_raw(length)?;
        Ok(bytes.to_vec())
    }

    pub fn read_octet_seq(&mut self) -> CdrResult<Vec<u8>> {
        let length = self.read_u32()?;
        self.read_bytes(length as usize)
    }

    pub fn read_octet_slice(&mut self) -> CdrResult<&'a [u8]> {
        let length = self.read_u32()? as usize;
        self.read_bytes_raw(length)
    }

    pub fn read_byte_array<const N: usize>(&mut self) -> CdrResult<[u8; N]> {
        let bytes = self.read_bytes_raw(N)?;
        match bytes.try_into() {
            Ok(array) => Ok(array),
            Err(_) => Err(CdrError::FixedArrayLength {
                expected: N,
                actual: bytes.len(),
            }),
        }
    }

    pub fn read_string(&mut self) -> CdrResult<String> {
        let length = self.read_u32()?;
        let length = length as usize;

        if length == 0 {
            return Ok(String::new());
        }
        let bytes = self.read_bytes_raw(length)?;

        let bytes = if bytes.last() == Some(&0) {
            &bytes[..bytes.len() - 1]
        } else {
            bytes
        };

        String::from_utf8(bytes.to_vec()).map_err(CdrError::InvalidUtf8)
    }

    pub fn read_str(&mut self) -> CdrResult<&'a str> {
        let length = self.read_u32()? as usize;

        if length == 0 {
            return Ok("");
        }

        let bytes = self.read_bytes_raw(length)?;
        let bytes = if bytes.last() == Some(&0) {
            &bytes[..bytes.len() - 1]
        } else {
            bytes
        };

        std::str::from_utf8(bytes).map_err(CdrError::InvalidUtf8Slice)
    }

    pub fn read_char(&mut self) -> CdrResult<char> {
        let u8_unit = self.read_u8()?;
        Ok(u8_unit as char)
    }

    pub fn read_wchar_u16(&mut self) -> CdrResult<char> {
        let u16_unit = self.read_u16()? as u32;
        char::from_u32(u16_unit).ok_or(CdrError::InvalidUnicodeScalar {
            context: "CdrDecoder::read_wchar_u16",
            value: u16_unit,
        })
    }

    pub fn read_wchar_u32(&mut self) -> CdrResult<char> {
        let u32_unit = self.read_u32()?;
        char::from_u32(u32_unit).ok_or(CdrError::InvalidUnicodeScalar {
            context: "CdrDecoder::read_wchar_u32",
            value: u32_unit,
        })
    }

    pub fn read_wstring(&mut self) -> CdrResult<String> {
        let code_unit_length = self.read_u32()? as usize;
        let length = code_unit_length
            .checked_mul(2)
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_wstring",
            })?;

        let bytes = self.read_bytes_raw(length)?;

        let bytes = if bytes.last_chunk::<2>() == Some(&[0, 0]) {
            &bytes[..bytes.len() - 2]
        } else {
            bytes
        };

        if bytes.len() % 2 != 0 {
            return Err(CdrError::OddByteLength {
                context: "CdrDecoder::read_wstring",
                len: bytes.len(),
            });
        }

        let mut utf16_units = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            let unit = match self.endianness {
                Endianness::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
                Endianness::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
            };
            utf16_units.push(unit);
        }
        char::decode_utf16(utf16_units)
            .map(|r| r.map_err(CdrError::InvalidUtf16))
            .collect()
    }

    pub fn read_bool(&mut self) -> CdrResult<bool> {
        let raw = self.read_u8()?;
        if raw == 0 {
            Ok(false)
        } else if raw == 1 {
            Ok(true)
        } else {
            Err(CdrError::InvalidBool(raw))
        }
    }

    pub fn read_u64(&mut self) -> CdrResult<u64> {
        self.align_to(size_of::<u64>());
        let bytes = self.read_bytes_raw(size_of::<u64>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(u64::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u64: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(u64::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u32: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_u32(&mut self) -> CdrResult<u32> {
        self.align_to(size_of::<u32>());
        let bytes = self.read_bytes_raw(size_of::<u32>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(u32::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(u32::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_u16(&mut self) -> CdrResult<u16> {
        self.align_to(size_of::<u16>());
        let bytes = self.read_bytes_raw(size_of::<u16>())?;

        match self.endianness {
            Endianness::Big => {
                Ok(u16::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u16: transfer bytes [u8; 2] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(u16::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u16: transfer bytes [u8; 2] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_u8(&mut self) -> CdrResult<u8> {
        let bytes = self.read_bytes_raw(size_of::<u8>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(u8::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u8: transfer bytes [u8; 1] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(u8::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u16: transfer bytes [u8; 1] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_i64(&mut self) -> CdrResult<i64> {
        self.align_to(size_of::<i64>());
        let bytes = self.read_bytes_raw(size_of::<i64>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(i64::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i64: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(i64::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u32: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_i32(&mut self) -> CdrResult<i32> {
        self.align_to(size_of::<i32>());
        let bytes = self.read_bytes_raw(size_of::<i32>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(i32::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(i32::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_i16(&mut self) -> CdrResult<i16> {
        self.align_to(size_of::<i16>());
        let bytes = self.read_bytes_raw(size_of::<i16>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(i16::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i16: transfer bytes [u8; 2] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(i16::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i16: transfer bytes [u8; 2] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_i8(&mut self) -> CdrResult<i8> {
        let bytes = self.read_bytes_raw(size_of::<i8>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(i8::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_i8: transfer bytes [u8; 1] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(i8::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_u16: transfer bytes [u8; 1] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_f64(&mut self) -> CdrResult<f64> {
        self.align_to(size_of::<f64>());
        let bytes = self.read_bytes_raw(size_of::<f64>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(f64::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_f64: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(f64::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_f64: transfer bytes [u8; 8] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_f32(&mut self) -> CdrResult<f32> {
        self.align_to(size_of::<f32>());
        let bytes = self.read_bytes_raw(size_of::<f32>())?;
        match self.endianness {
            Endianness::Big => {
                Ok(f32::from_be_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_f32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
            Endianness::Little => {
                Ok(f32::from_le_bytes(bytes.try_into().expect(
                    "CdrDecoder: read_f32: transfer bytes [u8; 4] to [u8; _] failed",
                )))
            }
        }
    }

    pub fn read_seq<T>(&mut self) -> CdrResult<Vec<T>>
    where
        T: DecodeCdr,
    {
        let length = self.read_u32()? as usize;
        let mut items = Vec::with_capacity(length);
        for _ in 0..length {
            items.push(T::decode_cdr(self)?);
        }
        Ok(items)
    }

    pub fn read_borrow_seq<T>(&mut self) -> CdrResult<Vec<T>>
    where
        T: BorrowDecodeCdr<'a>,
    {
        let length = self.read_u32()? as usize;
        let mut items = Vec::with_capacity(length);
        for _ in 0..length {
            items.push(T::borrow_decode_cdr(self)?);
        }
        Ok(items)
    }

    pub fn read_array<T, const N: usize>(&mut self) -> CdrResult<[T; N]>
    where
        T: DecodeCdr,
    {
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::decode_cdr(self)?);
        }
        items
            .try_into()
            .map_err(|items: Vec<T>| CdrError::FixedArrayLength {
                expected: N,
                actual: items.len(),
            })
    }

    pub fn read_borrow_array<T, const N: usize>(&mut self) -> CdrResult<[T; N]>
    where
        T: BorrowDecodeCdr<'a>,
    {
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::borrow_decode_cdr(self)?);
        }
        items
            .try_into()
            .map_err(|items: Vec<T>| CdrError::FixedArrayLength {
                expected: N,
                actual: items.len(),
            })
    }

    pub fn read_primitive_seq_borrowed<T: PrimitiveValue>(&mut self) -> CdrResult<PrimitiveSeq<'a, T>> {
        let length = self.read_u32()? as usize;
        self.align_to(size_of::<T>());
        let byte_len = length
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_primitive_seq_borrowed",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        Ok(PrimitiveSeq {
            bytes,
            endianness: self.endianness,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn read_primitive_array_borrowed<T: PrimitiveValue, const N: usize>(
        &mut self,
    ) -> CdrResult<PrimitiveArray<'a, T, N>> {
        self.align_to(size_of::<T>());
        let byte_len = N
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrDecoder::read_primitive_array_borrowed",
            })?;
        let bytes = self.read_bytes_raw(byte_len)?;
        Ok(PrimitiveArray {
            bytes,
            endianness: self.endianness,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn read_u16_seq(&mut self) -> CdrResult<Vec<u16>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<u16>();
        }
        self.read_swapped_primitive_seq::<u16>()
    }

    pub fn read_u32_seq(&mut self) -> CdrResult<Vec<u32>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<u32>();
        }
        self.read_swapped_primitive_seq::<u32>()
    }

    pub fn read_u64_seq(&mut self) -> CdrResult<Vec<u64>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<u64>();
        }
        self.read_swapped_primitive_seq::<u64>()
    }

    pub fn read_i16_seq(&mut self) -> CdrResult<Vec<i16>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<i16>();
        }
        self.read_swapped_primitive_seq::<i16>()
    }

    pub fn read_i32_seq(&mut self) -> CdrResult<Vec<i32>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<i32>();
        }
        self.read_swapped_primitive_seq::<i32>()
    }

    pub fn read_i64_seq(&mut self) -> CdrResult<Vec<i64>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<i64>();
        }
        self.read_swapped_primitive_seq::<i64>()
    }

    pub fn read_f32_seq(&mut self) -> CdrResult<Vec<f32>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<f32>();
        }
        self.read_swapped_primitive_seq::<f32>()
    }

    pub fn read_f64_seq(&mut self) -> CdrResult<Vec<f64>> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_seq::<f64>();
        }
        self.read_swapped_primitive_seq::<f64>()
    }

    pub fn read_u16_array<const N: usize>(&mut self) -> CdrResult<[u16; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<u16, N>();
        }
        self.read_swapped_primitive_array::<u16, N>()
    }

    pub fn read_u32_array<const N: usize>(&mut self) -> CdrResult<[u32; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<u32, N>();
        }
        self.read_swapped_primitive_array::<u32, N>()
    }

    pub fn read_u64_array<const N: usize>(&mut self) -> CdrResult<[u64; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<u64, N>();
        }
        self.read_swapped_primitive_array::<u64, N>()
    }

    pub fn read_i16_array<const N: usize>(&mut self) -> CdrResult<[i16; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<i16, N>();
        }
        self.read_swapped_primitive_array::<i16, N>()
    }

    pub fn read_i32_array<const N: usize>(&mut self) -> CdrResult<[i32; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<i32, N>();
        }
        self.read_swapped_primitive_array::<i32, N>()
    }

    pub fn read_i64_array<const N: usize>(&mut self) -> CdrResult<[i64; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<i64, N>();
        }
        self.read_swapped_primitive_array::<i64, N>()
    }

    pub fn read_f32_array<const N: usize>(&mut self) -> CdrResult<[f32; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<f32, N>();
        }
        self.read_swapped_primitive_array::<f32, N>()
    }

    pub fn read_f64_array<const N: usize>(&mut self) -> CdrResult<[f64; N]> {
        if self.native_endianness_matches() {
            return self.read_native_primitive_array::<f64, N>();
        }
        self.read_swapped_primitive_array::<f64, N>()
    }
}

impl CdrEncoder {
    pub fn new(cdr_encoding: CdrEncoding) -> Self {
        let data_raw = CdrEncoding::serialize(&cdr_encoding);
        Self {
            data_raw,
            endianness: cdr_encoding.endianness,
        }
    }

    pub fn align_to(&mut self, align_length: usize) {
        debug_assert!(align_length > 0);
        let pos = self.data_raw.len().saturating_sub(4);
        let padding_length = (align_length - pos % align_length) % align_length;
        self.data_raw
            .resize(self.data_raw.len() + padding_length, 0);
    }

    fn native_endianness_matches(&self) -> bool {
        native_endianness_matches(self.endianness)
    }

    fn write_native_primitive_slice<T: Copy>(&mut self, value: &[T], write_len: bool) -> CdrResult<()> {
        if write_len {
            self.write_u32(value.len() as u32);
        }
        self.align_to(size_of::<T>());
        let byte_len = value
            .len()
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrEncoder::write_native_primitive_slice",
            })?;
        let old_len = self.data_raw.len();
        self.data_raw.resize(old_len + byte_len, 0);
        unsafe {
            copy_bytes_into_simd(
                value.as_ptr() as *const u8,
                self.data_raw.as_mut_ptr().add(old_len),
                byte_len,
            );
        }
        Ok(())
    }

    fn write_swapped_primitive_slice<T: Copy>(
        &mut self,
        value: &[T],
        write_len: bool,
    ) -> CdrResult<()> {
        if write_len {
            self.write_u32(value.len() as u32);
        }
        self.align_to(size_of::<T>());
        let byte_len = value
            .len()
            .checked_mul(size_of::<T>())
            .ok_or(CdrError::LengthOverflow {
                context: "CdrEncoder::write_swapped_primitive_slice",
            })?;
        let old_len = self.data_raw.len();
        self.data_raw.resize(old_len + byte_len, 0);
        unsafe {
            copy_bytes_into_simd(
                value.as_ptr() as *const u8,
                self.data_raw.as_mut_ptr().add(old_len),
                byte_len,
            );
        }
        swap_endian_in_place(&mut self.data_raw[old_len..old_len + byte_len], size_of::<T>());
        Ok(())
    }

    pub fn write_wstring(&mut self, value: &str) {
        let utf16: Vec<u16> = value.encode_utf16().collect();
        self.write_u32((utf16.len() + 1) as u32);
        for unit in utf16 {
            self.write_u16(unit);
        }
        self.write_u16(0);
    }

    pub fn write_string(&mut self, value: &str) {
        let bytes = value.as_bytes();
        self.write_u32((bytes.len() + 1) as u32);
        self.write_bytes_raw(bytes);
        self.write_u8(0);
    }

    pub fn write_wchar32(&mut self, value: char) {
        self.write_u32(value as u32);
    }

    pub fn write_wchar16(&mut self, value: char) {
        let mut buffer = [0; 2];
        let wchar16 = value.encode_utf16(&mut buffer)[0];
        self.write_u16(wchar16);
    }

    pub fn write_char_u8(&mut self, value: u8) {
        self.write_u8(value);
    }

    pub fn write_char(&mut self, value: char) -> CdrResult<()> {
        if !value.is_ascii() {
            return Err(CdrError::NonAsciiChar(value));
        }
        self.write_u8(value as u8);
        Ok(())
    }

    pub fn write_u16(&mut self, value: u16) {
        self.align_to(size_of::<u16>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_u8(&mut self, value: u8) {
        self.data_raw.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    pub fn write_i8(&mut self, value: i8) {
        self.write_bytes_raw(&value.to_ne_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.align_to(size_of::<u32>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_u64(&mut self, value: u64) {
        self.align_to(size_of::<u64>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_i16(&mut self, value: i16) {
        self.align_to(size_of::<i16>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_i32(&mut self, value: i32) {
        self.align_to(size_of::<i32>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_i64(&mut self, value: i64) {
        self.align_to(size_of::<i64>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_f32(&mut self, value: f32) {
        self.align_to(size_of::<f32>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_f64(&mut self, value: f64) {
        self.align_to(size_of::<f64>());
        match self.endianness {
            Endianness::Big => self.write_bytes_raw(&value.to_be_bytes()),
            Endianness::Little => self.write_bytes_raw(&value.to_le_bytes()),
        }
    }

    pub fn write_bytes_raw(&mut self, value: &[u8]) {
        self.data_raw.extend_from_slice(value);
    }

    pub fn write_octet_bytes(&mut self, value: &[u8]) -> CdrResult<()> {
        self.write_u32(value.len() as u32);
        self.write_bytes_raw(value);
        Ok(())
    }

    pub fn write_byte_array<const N: usize>(&mut self, value: &[u8; N]) -> CdrResult<()> {
        self.write_bytes_raw(value);
        Ok(())
    }

    pub fn write_seq<T: EncodeCdr>(&mut self, value: &[T]) -> CdrResult<()> {
        self.write_u32(value.len() as u32);
        for item in value {
            item.encode_cdr(self)?;
        }
        Ok(())
    }

    pub fn write_array<T: EncodeCdr, const N: usize>(&mut self, value: &[T; N]) -> CdrResult<()> {
        for item in value {
            item.encode_cdr(self)?;
        }
        Ok(())
    }

    pub fn write_u16_seq(&mut self, value: &[u16]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_u32_seq(&mut self, value: &[u32]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_u64_seq(&mut self, value: &[u64]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_i16_seq(&mut self, value: &[i16]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_i32_seq(&mut self, value: &[i32]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_i64_seq(&mut self, value: &[i64]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_f32_seq(&mut self, value: &[f32]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_f64_seq(&mut self, value: &[f64]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, true);
        }
        self.write_swapped_primitive_slice(value, true)
    }

    pub fn write_u16_array<const N: usize>(&mut self, value: &[u16; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_u32_array<const N: usize>(&mut self, value: &[u32; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_u64_array<const N: usize>(&mut self, value: &[u64; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_i16_array<const N: usize>(&mut self, value: &[i16; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_i32_array<const N: usize>(&mut self, value: &[i32; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_i64_array<const N: usize>(&mut self, value: &[i64; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_f32_array<const N: usize>(&mut self, value: &[f32; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }

    pub fn write_f64_array<const N: usize>(&mut self, value: &[f64; N]) -> CdrResult<()> {
        if self.native_endianness_matches() {
            return self.write_native_primitive_slice(value, false);
        }
        self.write_swapped_primitive_slice(value, false)
    }
}

fn swap_endian_in_place(bytes: &mut [u8], elem_size: usize) {
    if bytes.len() < elem_size || elem_size <= 1 {
        return;
    }

    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("ssse3") {
            unsafe { swap_endian_in_place_ssse3(bytes, elem_size) };
            return;
        }
    }

    #[cfg(target_arch = "aarch64")]
    unsafe {
        swap_endian_in_place_neon(bytes, elem_size);
        return;
    }

    for chunk in bytes.chunks_exact_mut(elem_size) {
        chunk.reverse();
    }
}

#[cfg(target_arch = "x86_64")]
unsafe fn copy_bytes_into_simd(src: *const u8, dst: *mut u8, len: usize) {
    use std::arch::x86_64::{__m128i, _mm_loadu_si128, _mm_storeu_si128};

    let mut offset = 0usize;
    while offset + 16 <= len {
        let chunk = unsafe { _mm_loadu_si128(src.add(offset) as *const __m128i) };
        unsafe { _mm_storeu_si128(dst.add(offset) as *mut __m128i, chunk) };
        offset += 16;
    }
    unsafe { ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset) };
}

#[cfg(target_arch = "aarch64")]
unsafe fn copy_bytes_into_simd(src: *const u8, dst: *mut u8, len: usize) {
    use std::arch::aarch64::{uint8x16_t, vld1q_u8, vst1q_u8};

    let mut offset = 0usize;
    while offset + 16 <= len {
        let chunk: uint8x16_t = unsafe { vld1q_u8(src.add(offset)) };
        unsafe { vst1q_u8(dst.add(offset), chunk) };
        offset += 16;
    }
    unsafe { ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset) };
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
unsafe fn copy_bytes_into_simd(src: *const u8, dst: *mut u8, len: usize) {
    unsafe { ptr::copy_nonoverlapping(src, dst, len) };
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "ssse3")]
unsafe fn swap_endian_in_place_ssse3(bytes: &mut [u8], elem_size: usize) {
    use std::arch::x86_64::{
        __m128i, _mm_loadu_si128, _mm_setr_epi8, _mm_shuffle_epi8, _mm_storeu_si128,
    };

    let mask = match elem_size {
        2 => _mm_setr_epi8(1, 0, 3, 2, 5, 4, 7, 6, 9, 8, 11, 10, 13, 12, 15, 14),
        4 => _mm_setr_epi8(3, 2, 1, 0, 7, 6, 5, 4, 11, 10, 9, 8, 15, 14, 13, 12),
        8 => _mm_setr_epi8(7, 6, 5, 4, 3, 2, 1, 0, 15, 14, 13, 12, 11, 10, 9, 8),
        _ => {
            for chunk in bytes.chunks_exact_mut(elem_size) {
                chunk.reverse();
            }
            return;
        }
    };

    let simd_len = bytes.len() - (bytes.len() % 16);
    let mut offset = 0usize;
    while offset < simd_len {
        let chunk = unsafe { _mm_loadu_si128(bytes.as_ptr().add(offset) as *const __m128i) };
        let swapped = _mm_shuffle_epi8(chunk, mask);
        unsafe { _mm_storeu_si128(bytes.as_mut_ptr().add(offset) as *mut __m128i, swapped) };
        offset += 16;
    }

    for chunk in bytes[simd_len..].chunks_exact_mut(elem_size) {
        chunk.reverse();
    }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
unsafe fn swap_endian_in_place_neon(bytes: &mut [u8], elem_size: usize) {
    use std::arch::aarch64::{uint8x16_t, vld1q_u8, vrev16q_u8, vrev32q_u8, vrev64q_u8, vst1q_u8};

    let simd_len = bytes.len() - (bytes.len() % 16);
    let mut offset = 0usize;
    while offset < simd_len {
        let chunk = unsafe { vld1q_u8(bytes.as_ptr().add(offset)) };
        let swapped: uint8x16_t = match elem_size {
            2 => vrev16q_u8(chunk),
            4 => vrev32q_u8(chunk),
            8 => vrev64q_u8(chunk),
            _ => {
                for chunk in bytes[offset..].chunks_exact_mut(elem_size) {
                    chunk.reverse();
                }
                return;
            }
        };
        unsafe { vst1q_u8(bytes.as_mut_ptr().add(offset), swapped) };
        offset += 16;
    }

    for chunk in bytes[simd_len..].chunks_exact_mut(elem_size) {
        chunk.reverse();
    }
}

pub fn decode_from_bytes<T: DecodeCdr>(bytes: &[u8]) -> CdrResult<T> {
    let mut cdr_decoder = CdrDecoder::new(bytes)?;
    T::decode_cdr(&mut cdr_decoder)
}

pub fn borrow_decode_from_bytes<'a, T: BorrowDecodeCdr<'a>>(bytes: &'a [u8]) -> CdrResult<T> {
    let mut cdr_decoder = CdrDecoder::new(bytes)?;
    T::borrow_decode_cdr(&mut cdr_decoder)
}

pub fn encode_to_vec<T: EncodeCdr>(value: &T) -> CdrResult<Vec<u8>> {
    let mut encoder = CdrEncoder::new(CdrEncoding {
        cdr_representation: CdrRepresentation::Xcdr1,
        endianness: Endianness::Little,
    });
    value.encode_cdr(&mut encoder)?;
    Ok(encoder.data_raw)
}

impl DecodeCdr for bool {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_bool()
    }
}

impl EncodeCdr for bool {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_bool(*self);
        Ok(())
    }
}

impl DecodeCdr for u8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_u8()
    }
}

impl EncodeCdr for u8 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_u8(*self);
        Ok(())
    }
}

impl DecodeCdr for u16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_u16()
    }
}

impl EncodeCdr for u16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_u16(*self);
        Ok(())
    }
}

impl PrimitiveValue for u16 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => u16::from_be_bytes(bytes.try_into().expect("u16 chunk")),
            Endianness::Little => u16::from_le_bytes(bytes.try_into().expect("u16 chunk")),
        }
    }
}

impl DecodeCdr for u32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_u32()
    }
}

impl EncodeCdr for u32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_u32(*self);
        Ok(())
    }
}

impl PrimitiveValue for u32 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => u32::from_be_bytes(bytes.try_into().expect("u32 chunk")),
            Endianness::Little => u32::from_le_bytes(bytes.try_into().expect("u32 chunk")),
        }
    }
}

impl DecodeCdr for u64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_u64()
    }
}

impl EncodeCdr for u64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_u64(*self);
        Ok(())
    }
}

impl PrimitiveValue for u64 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => u64::from_be_bytes(bytes.try_into().expect("u64 chunk")),
            Endianness::Little => u64::from_le_bytes(bytes.try_into().expect("u64 chunk")),
        }
    }
}

impl DecodeCdr for i8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_i8()
    }
}

impl EncodeCdr for i8 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_i8(*self);
        Ok(())
    }
}

impl DecodeCdr for i16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_i16()
    }
}

impl EncodeCdr for i16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_i16(*self);
        Ok(())
    }
}

impl PrimitiveValue for i16 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => i16::from_be_bytes(bytes.try_into().expect("i16 chunk")),
            Endianness::Little => i16::from_le_bytes(bytes.try_into().expect("i16 chunk")),
        }
    }
}

impl DecodeCdr for i32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_i32()
    }
}

impl EncodeCdr for i32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_i32(*self);
        Ok(())
    }
}

impl PrimitiveValue for i32 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => i32::from_be_bytes(bytes.try_into().expect("i32 chunk")),
            Endianness::Little => i32::from_le_bytes(bytes.try_into().expect("i32 chunk")),
        }
    }
}

impl DecodeCdr for i64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_i64()
    }
}

impl EncodeCdr for i64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_i64(*self);
        Ok(())
    }
}

impl PrimitiveValue for i64 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => i64::from_be_bytes(bytes.try_into().expect("i64 chunk")),
            Endianness::Little => i64::from_le_bytes(bytes.try_into().expect("i64 chunk")),
        }
    }
}

impl DecodeCdr for f32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_f32()
    }
}

impl EncodeCdr for f32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_f32(*self);
        Ok(())
    }
}

impl PrimitiveValue for f32 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => f32::from_be_bytes(bytes.try_into().expect("f32 chunk")),
            Endianness::Little => f32::from_le_bytes(bytes.try_into().expect("f32 chunk")),
        }
    }
}

impl DecodeCdr for f64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_f64()
    }
}

impl EncodeCdr for f64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_f64(*self);
        Ok(())
    }
}

impl PrimitiveValue for f64 {
    fn decode_chunk(bytes: &[u8], endianness: Endianness) -> Self {
        match endianness {
            Endianness::Big => f64::from_be_bytes(bytes.try_into().expect("f64 chunk")),
            Endianness::Little => f64::from_le_bytes(bytes.try_into().expect("f64 chunk")),
        }
    }
}

impl DecodeCdr for char {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_char()
    }
}

impl EncodeCdr for char {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_char(*self)
    }
}

impl DecodeCdr for String {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_string()
    }
}

impl<'a> BorrowDecodeCdr<'a> for &'a str {
    fn borrow_decode_cdr(decoder: &mut CdrDecoder<'a>) -> CdrResult<Self> {
        decoder.read_str()
    }
}

impl EncodeCdr for String {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_string(self);
        Ok(())
    }
}

impl DecodeCdr for WChar16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_wchar_u16().map(Self)
    }
}

impl EncodeCdr for WChar16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_wchar16(self.0);
        Ok(())
    }
}

impl DecodeCdr for WChar32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> CdrResult<Self> {
        decoder.read_wchar_u32().map(Self)
    }
}

impl EncodeCdr for WChar32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> CdrResult<()> {
        encoder.write_wchar32(self.0);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::{
        decode_from_bytes, encode_to_vec, CdrDecoder, CdrEncoder, CdrEncoding,
        CdrRepresentation, DecodeCdr, EncodeCdr, Endianness, WChar16, WChar32,
    };

    fn little_endian_message(payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0x00, 0x01, 0x00, 0x00];
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn new_rejects_short_input() {
        assert!(CdrDecoder::new(&[0x00, 0x01, 0x00]).is_err());
    }

    #[test]
    fn read_u32_uses_payload_after_encapsulation() {
        let bytes = little_endian_message(&[0x2a, 0x00, 0x00, 0x00]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(decoder.read_u32().expect("u32 should decode"), 42);
    }

    #[test]
    fn read_u64_aligns_after_u8() {
        let bytes = little_endian_message(&[
            0x07, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(decoder.read_u8().expect("u8 should decode"), 7);
        assert_eq!(decoder.read_u64().expect("u64 should decode"), 42);
    }

    #[test]
    fn read_string_aligns_next_u64() {
        let bytes = little_endian_message(&[
            0x02, 0x00, 0x00, 0x00, b'a', 0x00, 0x00, 0x00, 0x2a, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(decoder.read_string().expect("string should decode"), "a");
        assert_eq!(decoder.read_u64().expect("u64 should decode"), 42);
    }

    #[test]
    fn read_wstring_trims_utf16_terminator() {
        let bytes = little_endian_message(&[
            0x02, 0x00, 0x00, 0x00, 0x41, 0x00, 0x00, 0x00,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(decoder.read_wstring().expect("wstring should decode"), "A");
    }

    #[test]
    fn read_wstring_rejects_invalid_utf16() {
        let bytes = little_endian_message(&[
            0x02, 0x00, 0x00, 0x00, 0x00, 0xd8, 0x00, 0x00,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert!(decoder.read_wstring().is_err());
    }

    #[test]
    fn read_octet_seq_reads_owned_bytes() {
        let bytes = little_endian_message(&[
            0x03, 0x00, 0x00, 0x00, 0xaa, 0xbb, 0xcc,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(
            decoder.read_octet_seq().expect("octet sequence should decode"),
            vec![0xaa, 0xbb, 0xcc]
        );
    }

    #[test]
    fn read_seq_decodes_primitive_items() {
        let bytes = little_endian_message(&[
            0x03, 0x00, 0x00, 0x00, 0x11, 0x22, 0x33,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(
            decoder.read_seq::<u8>().expect("sequence should decode"),
            vec![0x11, 0x22, 0x33]
        );
    }

    #[test]
    fn read_array_decodes_primitive_items() {
        let bytes = little_endian_message(&[0x34, 0x12, 0x78, 0x56]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(
            decoder
                .read_array::<u16, 2>()
                .expect("array should decode"),
            [0x1234, 0x5678]
        );
    }

    #[test]
    fn read_byte_array_reads_fixed_bytes() {
        let bytes = little_endian_message(&[0x2a, 0x2b, 0x2c, 0x2d]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(
            decoder
                .read_byte_array::<4>()
                .expect("byte array should decode"),
            [0x2a, 0x2b, 0x2c, 0x2d]
        );
    }

    #[test]
    fn read_f32_seq_decodes_specialized_sequence() {
        let bytes = little_endian_message(&[
            0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3f, 0x00, 0x00, 0x00, 0x40,
        ]);
        let mut decoder = CdrDecoder::new(&bytes).expect("decoder should be created");
        assert_eq!(
            decoder.read_f32_seq().expect("f32 seq should decode"),
            vec![1.0, 2.0]
        );
    }

    #[test]
    fn wchar_wrapper_types_decode() {
        let wchar16_bytes = little_endian_message(&[0x41, 0x00]);
        assert_eq!(
            decode_from_bytes::<WChar16>(&wchar16_bytes).expect("wchar16 should decode"),
            WChar16('A')
        );

        let wchar32_bytes = little_endian_message(&[0x42, 0x00, 0x00, 0x00]);
        assert_eq!(
            decode_from_bytes::<WChar32>(&wchar32_bytes).expect("wchar32 should decode"),
            WChar32('B')
        );
    }

    #[test]
    fn encode_round_trips_primitives() {
        let bytes = encode_to_vec(&42u32).expect("u32 should encode");
        assert_eq!(decode_from_bytes::<u32>(&bytes).expect("u32 should decode"), 42);

        let bytes = encode_to_vec(&-7i16).expect("i16 should encode");
        assert_eq!(decode_from_bytes::<i16>(&bytes).expect("i16 should decode"), -7);

        let bytes = encode_to_vec(&3.5f32).expect("f32 should encode");
        assert_eq!(decode_from_bytes::<f32>(&bytes).expect("f32 should decode"), 3.5);
    }

    #[test]
    fn encode_round_trips_string() {
        let value = String::from("abc");
        let bytes = encode_to_vec(&value).expect("string should encode");
        assert_eq!(
            decode_from_bytes::<String>(&bytes).expect("string should decode"),
            value
        );
    }

    #[test]
    fn encoder_aligns_u64_after_u8() {
        let mut encoder = CdrEncoder::new(CdrEncoding {
            cdr_representation: CdrRepresentation::Xcdr1,
            endianness: Endianness::Little,
        });
        7u8.encode_cdr(&mut encoder).expect("u8 should encode");
        42u64.encode_cdr(&mut encoder).expect("u64 should encode");

        let mut decoder = CdrDecoder::new(&encoder.data_raw).expect("decoder should be created");
        assert_eq!(decoder.read_u8().expect("u8 should decode"), 7);
        assert_eq!(decoder.read_u64().expect("u64 should decode"), 42);
    }

    #[test]
    fn write_byte_array_encodes_fixed_bytes() {
        let mut encoder = CdrEncoder::new(CdrEncoding {
            cdr_representation: CdrRepresentation::Xcdr1,
            endianness: Endianness::Little,
        });
        encoder
            .write_byte_array(&[0x2a, 0x2b, 0x2c, 0x2d])
            .expect("byte array should encode");
        assert_eq!(encoder.data_raw, little_endian_message(&[0x2a, 0x2b, 0x2c, 0x2d]));
    }
}
