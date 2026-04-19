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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar16(pub char);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WChar32(pub char);

pub struct CdrEncoding {
    pub cdr_representation: CdrRepresentation,
    pub endianness: Endianness,
}

impl CdrEncoding {
    pub fn parse(data: &[u8]) -> Result<(Self, &[u8]), String> {
        if data.len() < 4 {
            return Err("CdrEncoding: parse_from: len should be longer than 4".to_string());
        }
        // let mask = [0x0, 0x1, 0x2, 0x3];

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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<u64>())
        //     .ok_or_else(|| "CdrDecoder: read_u64: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_u64: not enough bytes in payload.".to_string());
        // }

        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<u32>())
        //     .ok_or_else(|| "CdrDecoder: read_u32: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_u32: not enough bytes in payload.".to_string());
        // }

        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<u16>())
        //     .ok_or_else(|| "CdrDecoder: read_u16: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_u16: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<u8>())
        //     .ok_or_else(|| "CdrDecoder: read_u8: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_u8: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<i64>())
        //     .ok_or_else(|| "CdrDecoder: read_i64: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_i64: not enough bytes in payload.".to_string());
        // }

        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<i32>())
        //     .ok_or_else(|| "CdrDecoder: read_i32: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_i32: not enough bytes in payload.".to_string());
        // }

        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<i16>())
        //     .ok_or_else(|| "CdrDecoder: read_i16: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_i16: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<i8>())
        //     .ok_or_else(|| "CdrDecoder: read_i8: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_i8: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<f64>())
        //     .ok_or_else(|| "CdrDecoder: read_f64: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_f64: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // let end = self
        //     .pos
        //     .checked_add(size_of::<f32>())
        //     .ok_or_else(|| "CdrDecoder: read_f32: position overflow.".to_string())?;
        // if end > self.payload.len() {
        //     return Err("CdrDecoder: read_f32: not enough bytes in payload.".to_string());
        // }
        // let bytes = &self.payload[self.pos..end];
        // self.pos = end;
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
        // std::array::try_from_fn(|_| T::decode_cdr(self))
        let mut items = Vec::with_capacity(N);
        for _ in 0..N {
            items.push(T::decode_cdr(self)?);
        }
        items
            .try_into()
            .map_err(|_| "CdrDecoder: read_array: failed to build fixed-size array".to_string())
    }
}

pub fn decode_from_bytes<T: DecodeCdr>(bytes: &[u8]) -> Result<T, String> {
    let mut cdr_decoder = CdrDecoder::new(bytes)?;
    T::decode_cdr(&mut cdr_decoder)
}

impl DecodeCdr for bool {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_bool()
    }
}

impl DecodeCdr for u8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u8()
    }
}

impl DecodeCdr for u16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u16()
    }
}

impl DecodeCdr for u32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u32()
    }
}

impl DecodeCdr for u64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_u64()
    }
}

impl DecodeCdr for i8 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i8()
    }
}

impl DecodeCdr for i16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i16()
    }
}

impl DecodeCdr for i32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i32()
    }
}

impl DecodeCdr for i64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_i64()
    }
}

impl DecodeCdr for f32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_f32()
    }
}

impl DecodeCdr for f64 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_f64()
    }
}

impl DecodeCdr for char {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_char()
    }
}

impl DecodeCdr for String {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_string()
    }
}

impl DecodeCdr for WChar16 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_wchar_u16().map(Self)
    }
}

impl DecodeCdr for WChar32 {
    fn decode_cdr(decoder: &mut CdrDecoder<'_>) -> Result<Self, String> {
        decoder.read_wchar_u32().map(Self)
    }
}

#[cfg(test)]
mod test {
    use super::{decode_from_bytes, CdrDecoder, WChar16, WChar32};

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
}
