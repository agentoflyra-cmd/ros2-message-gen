use std::char;

pub enum CdrRepresentation {
    Xcdr1,
    Xcdr2,
}

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
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String>;
}

pub struct CdrEncoder {
    pub data_raw: Vec<u8>,
    pub endianness: Endianness,
}

pub trait EncodeCdr: Sized {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar16(pub char);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar32(pub char);

pub struct CdrEncoding {
    pub cdr_representation: CdrRepresentation,
    pub endianness: Endianness,
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

    pub fn parse(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.len() < 4 {
            return Err("CdrEncoding: parse_from: len should be longer than 4".to_string());
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
            _ => Err("CdrEncoding: parse: invalid representaion!".to_string()),
        }
    }
}

impl<'a> CdrDecoder<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 4 {
            return Err("CdrDecoder: new: length of data not enough.".to_string());
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

    fn read_bytes_raw(&mut self, length: usize) -> Result<&'a [u8], String> {
        if length == 0 {
            return Ok(&[]);
        }

        let end = self
            .pos
            .checked_add(length)
            .ok_or_else(|| "CdrDecoder: read_string: length overflow.".to_string())?;

        if end > self.payload.len() {
            return Err("CdrDecoder: read_string: not enough bytes in payload.".to_string());
        }

        let bytes = &self.payload[self.pos..end];
        self.pos = end;
        Ok(bytes)
    }

    pub fn read_bytes(&mut self, length: usize) -> Result<Vec<u8>, String> {
        let bytes = self.read_bytes_raw(length)?;
        Ok(bytes.to_vec())
    }

    pub fn read_octet_seq(&mut self) -> Result<Vec<u8>, String> {
        let length = self.read_u32()?;
        self.read_bytes(length as usize)
    }

    pub fn read_string(&mut self) -> Result<String, String> {
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

        String::from_utf8(bytes.to_vec())
            .map_err(|_| "CdrDecoder: read_string: invalid utf8 bytes.".to_string())
    }

    pub fn read_char(&mut self) -> Result<char, String> {
        let u8_unit = self.read_u8()?;
        Ok(u8_unit as char)
    }

    pub fn read_wchar_u16(&mut self) -> Result<char, String> {
        let u16_unit = self.read_u16()? as u32;
        char::from_u32(u16_unit)
            .ok_or_else(|| "CdrDecoder: read_wchar_u16: invalid unicode scalar value.".to_string())
    }

    pub fn read_wchar_u32(&mut self) -> Result<char, String> {
        let u32_unit = self.read_u32()?;
        char::from_u32(u32_unit)
            .ok_or_else(|| "CdrDecoder: read_wchar_u32: invalid unicode scalar value.".to_string())
    }

    pub fn read_wstring(&mut self) -> Result<String, String> {
        let code_unit_length = self.read_u32()? as usize;
        let length = code_unit_length
            .checked_mul(2)
            .ok_or_else(|| "CdrDecoder: read_wstring: length overflow.".to_string())?;

        let bytes = self.read_bytes_raw(length)?;

        let bytes = if bytes.last_chunk::<2>() == Some(&[0, 0]) {
            &bytes[..bytes.len() - 2]
        } else {
            bytes
        };

        if bytes.len() % 2 != 0 {
            return Err("CdrDecoder: read_wstring: odd byte length.".to_string());
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
            .map(|r| {
                r.map_err(|_| "CdrDecoder: read_wstring: cannot decode utf16 units.".to_string())
            })
            .collect()
    }

    pub fn read_bool(&mut self) -> Result<bool, String> {
        let raw = self.read_u8()?;
        if raw == 0 {
            Ok(false)
        } else if raw == 1 {
            Ok(true)
        } else {
            Err("CdrDecoder: read_bool: invalid number, should be 0 or 1.".to_string())
        }
    }

    pub fn read_u64(&mut self) -> Result<u64, String> {
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

    pub fn read_u32(&mut self) -> Result<u32, String> {
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

    pub fn read_u16(&mut self) -> Result<u16, String> {
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

    pub fn read_u8(&mut self) -> Result<u8, String> {
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

    pub fn read_i64(&mut self) -> Result<i64, String> {
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

    pub fn read_i32(&mut self) -> Result<i32, String> {
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

    pub fn read_i16(&mut self) -> Result<i16, String> {
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

    pub fn read_i8(&mut self) -> Result<i8, String> {
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

    pub fn read_f64(&mut self) -> Result<f64, String> {
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

    pub fn read_f32(&mut self) -> Result<f32, String> {
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

    pub fn read_seq<T>(&mut self) -> Result<Vec<T>, String>
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

    pub fn read_array<T, const N: usize>(&mut self) -> Result<[T; N], String>
    where
        T: DecodeCdr,
    {
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::decode_cdr(self)?);
        }
        items
            .try_into()
            .map_err(|_| "CdrDecoder: read_array: failed to build fixed-size array".to_string())
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
        self.write_bytes_raw(bytes.to_vec());
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

    pub fn write_char(&mut self, value: char) -> Result<(), String> {
        if !value.is_ascii() {
            return Err("CdrEncoder: write_char: char is not ascii code.".to_string());
        }
        self.write_u8(value as u8);
        Ok(())
    }

    pub fn write_u16(&mut self, value: u16) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<u16>());
        self.write_bytes_raw(data);
    }

    pub fn write_u8(&mut self, value: u8) {
        self.data_raw.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    pub fn write_i8(&mut self, value: i8) {
        self.write_bytes_raw(value.to_ne_bytes().to_vec());
    }

    pub fn write_u32(&mut self, value: u32) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<u32>());
        self.write_bytes_raw(data);
    }

    pub fn write_u64(&mut self, value: u64) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<u64>());
        self.write_bytes_raw(data);
    }

    pub fn write_i16(&mut self, value: i16) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<i16>());
        self.write_bytes_raw(data);
    }

    pub fn write_i32(&mut self, value: i32) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<i32>());
        self.write_bytes_raw(data);
    }

    pub fn write_i64(&mut self, value: i64) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<i64>());
        self.write_bytes_raw(data);
    }

    pub fn write_f32(&mut self, value: f32) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<f32>());
        self.write_bytes_raw(data);
    }

    pub fn write_f64(&mut self, value: f64) {
        let data = match self.endianness {
            Endianness::Big => value.to_be_bytes(),
            Endianness::Little => value.to_le_bytes(),
        }
        .to_vec();
        self.align_to(size_of::<f64>());
        self.write_bytes_raw(data);
    }

    pub fn write_bytes_raw(&mut self, value: Vec<u8>) {
        self.data_raw.extend_from_slice(&value);
    }

    pub fn write_octet_bytes(&mut self, value: Vec<u8>) {
        self.write_u32(value.len() as u32);
        self.write_bytes_raw(value);
    }

    pub fn write_seq<T: EncodeCdr>(&mut self, value: &[T]) -> Result<(), String> {
        self.write_u32(value.len() as u32);
        for item in value {
            item.encode_cdr(self)?;
        }
        Ok(())
    }

    pub fn write_array<T: EncodeCdr, const N: usize>(&mut self, value: &[T; N]) -> Result<(), String> {
        for item in value {
            item.encode_cdr(self)?;
        }
        Ok(())
    }
}

pub fn decode_from_bytes<T: DecodeCdr>(bytes: &[u8]) -> Result<T, String> {
    let mut cdr_decoder = CdrDecoder::new(bytes)?;
    T::decode_cdr(&mut cdr_decoder)
}

pub fn encode_to_vec<T: EncodeCdr>(value: &T) -> Result<Vec<u8>, String> {
    let mut encoder = CdrEncoder::new(CdrEncoding {
        cdr_representation: CdrRepresentation::Xcdr1,
        endianness: Endianness::Little,
    });
    value.encode_cdr(&mut encoder)?;
    Ok(encoder.data_raw)
}

impl DecodeCdr for bool {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_bool()
    }
}

impl EncodeCdr for bool {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_bool(*self);
        Ok(())
    }
}

impl DecodeCdr for u8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u8()
    }
}

impl EncodeCdr for u8 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_u8(*self);
        Ok(())
    }
}

impl DecodeCdr for u16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u16()
    }
}

impl EncodeCdr for u16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_u16(*self);
        Ok(())
    }
}

impl DecodeCdr for u32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u32()
    }
}

impl EncodeCdr for u32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_u32(*self);
        Ok(())
    }
}

impl DecodeCdr for u64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u64()
    }
}

impl EncodeCdr for u64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_u64(*self);
        Ok(())
    }
}

impl DecodeCdr for i8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i8()
    }
}

impl EncodeCdr for i8 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_i8(*self);
        Ok(())
    }
}

impl DecodeCdr for i16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i16()
    }
}

impl EncodeCdr for i16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_i16(*self);
        Ok(())
    }
}

impl DecodeCdr for i32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i32()
    }
}

impl EncodeCdr for i32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_i32(*self);
        Ok(())
    }
}

impl DecodeCdr for i64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i64()
    }
}

impl EncodeCdr for i64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_i64(*self);
        Ok(())
    }
}

impl DecodeCdr for f32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_f32()
    }
}

impl EncodeCdr for f32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_f32(*self);
        Ok(())
    }
}

impl DecodeCdr for f64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_f64()
    }
}

impl EncodeCdr for f64 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_f64(*self);
        Ok(())
    }
}

impl DecodeCdr for char {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_char()
    }
}

impl EncodeCdr for char {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_char(*self)
    }
}

impl DecodeCdr for String {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_string()
    }
}

impl EncodeCdr for String {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_string(self);
        Ok(())
    }
}

impl DecodeCdr for WChar16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_wchar_u16().map(Self)
    }
}

impl EncodeCdr for WChar16 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
        encoder.write_wchar16(self.0);
        Ok(())
    }
}

impl DecodeCdr for WChar32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_wchar_u32().map(Self)
    }
}

impl EncodeCdr for WChar32 {
    fn encode_cdr(&self, encoder: &mut CdrEncoder) -> Result<(), String> {
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
}
