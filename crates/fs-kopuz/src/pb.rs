//! The protobuf wire format, as much of it as kopuzd's messages use.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(pub &'static str);

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "protobuf: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

pub trait Message: Sized {
    fn encode(&self, w: &mut Writer);
    fn decode(buf: &[u8]) -> Result<Self, DecodeError>;

    fn to_bytes(&self) -> Vec<u8> {
        let mut w = Writer::default();
        self.encode(&mut w);
        w.into_bytes()
    }
}

const VARINT: u8 = 0;
const FIXED64: u8 = 1;
const LEN: u8 = 2;
const FIXED32: u8 = 5;

/// Fields with implicit presence are left out at their default, as proto3
/// encoders do; `optional` ones are written whenever they are set.
#[derive(Debug, Default)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn varint(&mut self, mut v: u64) {
        while v >= 0x80 {
            self.buf.push(v as u8 | 0x80);
            v >>= 7;
        }
        self.buf.push(v as u8);
    }

    fn key(&mut self, field: u32, wire: u8) {
        self.varint(u64::from(field) << 3 | u64::from(wire));
    }

    pub fn uint64(&mut self, field: u32, v: u64) {
        if v != 0 {
            self.optional_uint64(field, Some(v));
        }
    }

    pub fn optional_uint64(&mut self, field: u32, v: Option<u64>) {
        if let Some(v) = v {
            self.key(field, VARINT);
            self.varint(v);
        }
    }

    pub fn bool(&mut self, field: u32, v: bool) {
        if v {
            self.key(field, VARINT);
            self.varint(1);
        }
    }

    /// Negative values take ten bytes, as int32 does on the wire.
    pub fn enumeration(&mut self, field: u32, v: i32) {
        if v != 0 {
            self.key(field, VARINT);
            self.varint(i64::from(v) as u64);
        }
    }

    pub fn string(&mut self, field: u32, v: &str) {
        if !v.is_empty() {
            self.bytes(field, v.as_bytes());
        }
    }

    pub fn optional_string(&mut self, field: u32, v: Option<&str>) {
        if let Some(v) = v {
            self.bytes(field, v.as_bytes());
        }
    }

    pub fn bytes(&mut self, field: u32, v: &[u8]) {
        self.key(field, LEN);
        self.varint(v.len() as u64);
        self.buf.extend_from_slice(v);
    }

    pub fn message(&mut self, field: u32, m: &impl Message) {
        self.bytes(field, &m.to_bytes());
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Value<'a> {
    pub fn uint64(self) -> Result<u64, DecodeError> {
        match self {
            Value::Varint(v) => Ok(v),
            _ => Err(DecodeError("expected a varint")),
        }
    }

    pub fn uint32(self) -> Result<u32, DecodeError> {
        self.uint64().map(|v| v as u32)
    }

    pub fn int32(self) -> Result<i32, DecodeError> {
        self.uint64().map(|v| v as i32)
    }

    pub fn bool(self) -> Result<bool, DecodeError> {
        self.uint64().map(|v| v != 0)
    }

    pub fn bytes(self) -> Result<&'a [u8], DecodeError> {
        match self {
            Value::Bytes(b) => Ok(b),
            _ => Err(DecodeError("expected a length-delimited field")),
        }
    }

    pub fn string(self) -> Result<String, DecodeError> {
        let b = self.bytes()?;
        String::from_utf8(b.to_vec()).map_err(|_| DecodeError("string is not UTF-8"))
    }

    pub fn message<M: Message>(self) -> Result<M, DecodeError> {
        M::decode(self.bytes()?)
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if self.buf.len() < n {
            return Err(DecodeError("field runs past the message"));
        }
        let (head, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(head)
    }

    fn varint(&mut self) -> Result<u64, DecodeError> {
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.take(1).map_err(|_| DecodeError("truncated varint"))?[0];
            v |= u64::from(b & 0x7f) << (7 * i);
            if b < 0x80 {
                return Ok(v);
            }
        }
        Err(DecodeError("varint longer than ten bytes"))
    }

    /// The next field number and its value, `None` at the end. Callers skip
    /// the numbers they do not know, so a newer daemon still reads.
    pub fn field(&mut self) -> Result<Option<(u32, Value<'a>)>, DecodeError> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let key = self.varint()?;
        let field = u32::try_from(key >> 3).map_err(|_| DecodeError("field number out of range"))?;
        if field == 0 {
            return Err(DecodeError("field number zero"));
        }
        let value = match (key & 7) as u8 {
            VARINT => Value::Varint(self.varint()?),
            FIXED64 => Value::Fixed64(u64::from_le_bytes(self.take(8)?.try_into().unwrap())),
            LEN => {
                let len = usize::try_from(self.varint()?).map_err(|_| DecodeError("length out of range"))?;
                Value::Bytes(self.take(len)?)
            }
            FIXED32 => Value::Fixed32(u32::from_le_bytes(self.take(4)?.try_into().unwrap())),
            _ => return Err(DecodeError("unsupported wire type")),
        };
        Ok(Some((field, value)))
    }
}
