//! One gRPC call over HTTP/2 with prior knowledge: a connection per call,
//! the call on stream 1. Response header blocks are skipped rather than
//! HPACK-decoded, so a call succeeds when it carries a message.

use std::{fmt, io};

use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::pb::DecodeError;

const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const DATA: u8 = 0x0;
const HEADERS: u8 = 0x1;
const RST_STREAM: u8 = 0x3;
const SETTINGS: u8 = 0x4;
const PING: u8 = 0x6;
const GOAWAY: u8 = 0x7;
const WINDOW_UPDATE: u8 = 0x8;

const END_STREAM: u8 = 0x1;
const ACK: u8 = 0x1;
const END_HEADERS: u8 = 0x4;
const PADDED: u8 = 0x8;

const SETTINGS_ENABLE_PUSH: u16 = 0x2;

/// The peer's default SETTINGS_MAX_FRAME_SIZE, which we never ask it to raise.
const MAX_FRAME: usize = 16_384;

const STREAM: u32 = 1;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The daemon hung up before the call ended.
    Closed,
    GoAway(u32),
    Reset(u32),
    /// The call ended without a message: a gRPC error status.
    NoMessage,
    Compressed,
    Protocol(&'static str),
    Decode(DecodeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "kopuzd: {e}"),
            Error::Closed => f.write_str("kopuzd: connection closed"),
            Error::GoAway(code) => write!(f, "kopuzd: GOAWAY {code:#x}"),
            Error::Reset(code) => write!(f, "kopuzd: stream reset {code:#x}"),
            Error::NoMessage => f.write_str("kopuzd: call ended without a message"),
            Error::Compressed => f.write_str("kopuzd: compressed message"),
            Error::Protocol(what) => write!(f, "kopuzd: {what}"),
            Error::Decode(e) => write!(f, "kopuzd: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<DecodeError> for Error {
    fn from(e: DecodeError) -> Self {
        Error::Decode(e)
    }
}

struct Frame {
    kind: u8,
    flags: u8,
    stream: u32,
    payload: Vec<u8>,
}

fn write_frame(out: &mut Vec<u8>, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
    let len = payload.len() as u32;
    out.extend_from_slice(&len.to_be_bytes()[1..]);
    out.push(kind);
    out.push(flags);
    out.extend_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
    out.extend_from_slice(payload);
}

fn hpack_int(out: &mut Vec<u8>, first: u8, prefix: u32, mut v: usize) {
    let max = (1usize << prefix) - 1;
    if v < max {
        out.push(first | v as u8);
        return;
    }
    out.push(first | max as u8);
    v -= max;
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Literal header fields without indexing and with a literal name, no
/// Huffman: nothing enters either side's dynamic table.
fn header_block(method: &str) -> Vec<u8> {
    let path = format!("/kopuz.v1.Kopuz/{method}");
    let mut out = Vec::new();
    for (name, value) in [
        (":method", "POST"),
        (":scheme", "http"),
        (":path", path.as_str()),
        (":authority", "localhost"),
        ("content-type", "application/grpc"),
        ("te", "trailers"),
    ] {
        out.push(0x00);
        for s in [name, value] {
            hpack_int(&mut out, 0x00, 7, s.len());
            out.extend_from_slice(s.as_bytes());
        }
    }
    out
}

/// One call on its own connection. Every await in `message` is cancel-safe:
/// reads land in `rbuf` and writes drain from `wbuf`, so a dropped future
/// loses nothing.
pub struct Call<S> {
    io: S,
    rbuf: Vec<u8>,
    wbuf: Vec<u8>,
    body: Vec<u8>,
    ended: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Call<S> {
    /// Sends the preface, the request headers and `message` as the whole
    /// request body.
    pub async fn start(io: S, method: &str, message: &[u8]) -> Result<Self, Error> {
        let mut wbuf = PREFACE.to_vec();
        let mut settings = SETTINGS_ENABLE_PUSH.to_be_bytes().to_vec();
        settings.extend_from_slice(&0u32.to_be_bytes());
        write_frame(&mut wbuf, SETTINGS, 0, 0, &settings);
        write_frame(&mut wbuf, HEADERS, END_HEADERS, STREAM, &header_block(method));
        let mut body = vec![0];
        body.extend_from_slice(&(message.len() as u32).to_be_bytes());
        body.extend_from_slice(message);
        let mut chunks = body.chunks(MAX_FRAME).peekable();
        while let Some(chunk) = chunks.next() {
            let flags = if chunks.peek().is_none() { END_STREAM } else { 0 };
            write_frame(&mut wbuf, DATA, flags, STREAM, chunk);
        }
        let mut call = Self { io, rbuf: Vec::new(), wbuf, body: Vec::new(), ended: false };
        call.flush().await?;
        Ok(call)
    }

    /// The call's single reply, past anything after it up to the stream's end.
    pub async fn unary(io: S, method: &str, message: &[u8]) -> Result<Vec<u8>, Error> {
        let mut call = Self::start(io, method, message).await?;
        let reply = call.message().await?.ok_or(Error::NoMessage)?;
        while call.message().await?.is_some() {}
        Ok(reply)
    }

    /// The next whole gRPC message, `None` once the stream has ended.
    pub async fn message(&mut self) -> Result<Option<Vec<u8>>, Error> {
        loop {
            self.flush().await?;
            if let Some(m) = self.take_message()? {
                return Ok(Some(m));
            }
            if self.ended {
                if !self.body.is_empty() {
                    return Err(Error::Protocol("stream ended inside a message"));
                }
                return Ok(None);
            }
            let frame = self.read_frame().await?;
            self.handle(frame)?;
        }
    }

    fn take_message(&mut self) -> Result<Option<Vec<u8>>, Error> {
        if self.body.len() < 5 {
            return Ok(None);
        }
        if self.body[0] != 0 {
            return Err(Error::Compressed);
        }
        let len = u32::from_be_bytes(self.body[1..5].try_into().unwrap()) as usize;
        if self.body.len() < 5 + len {
            return Ok(None);
        }
        let m = self.body[5..5 + len].to_vec();
        self.body.drain(..5 + len);
        Ok(Some(m))
    }

    async fn flush(&mut self) -> Result<(), Error> {
        while !self.wbuf.is_empty() {
            let n = self.io.write(&self.wbuf).await?;
            if n == 0 {
                return Err(Error::Closed);
            }
            self.wbuf.drain(..n);
        }
        self.io.flush().await?;
        Ok(())
    }

    async fn read_frame(&mut self) -> Result<Frame, Error> {
        let mut tmp = [0u8; 8192];
        loop {
            if self.rbuf.len() >= 9 {
                let len = u32::from_be_bytes([0, self.rbuf[0], self.rbuf[1], self.rbuf[2]]) as usize;
                if self.rbuf.len() >= 9 + len {
                    let frame = Frame {
                        kind: self.rbuf[3],
                        flags: self.rbuf[4],
                        stream: u32::from_be_bytes(self.rbuf[5..9].try_into().unwrap()) & 0x7fff_ffff,
                        payload: self.rbuf[9..9 + len].to_vec(),
                    };
                    self.rbuf.drain(..9 + len);
                    return Ok(frame);
                }
            }
            let n = self.io.read(&mut tmp).await?;
            if n == 0 {
                return Err(Error::Closed);
            }
            self.rbuf.extend_from_slice(&tmp[..n]);
        }
    }

    fn handle(&mut self, frame: Frame) -> Result<(), Error> {
        let ours = frame.stream == STREAM;
        match frame.kind {
            DATA if ours => {
                let mut data = &frame.payload[..];
                if frame.flags & PADDED != 0 {
                    let (&pad, rest) = data.split_first().ok_or(Error::Protocol("empty padded frame"))?;
                    let keep = rest.len().checked_sub(pad as usize).ok_or(Error::Protocol("padding past the frame"))?;
                    data = &rest[..keep];
                }
                self.body.extend_from_slice(data);
                // Padding counts against flow control too, so the whole
                // payload goes back.
                let n = frame.payload.len() as u32;
                if n > 0 {
                    write_frame(&mut self.wbuf, WINDOW_UPDATE, 0, 0, &n.to_be_bytes());
                    if frame.flags & END_STREAM == 0 {
                        write_frame(&mut self.wbuf, WINDOW_UPDATE, 0, STREAM, &n.to_be_bytes());
                    }
                }
                if frame.flags & END_STREAM != 0 {
                    self.ended = true;
                }
            }
            HEADERS if ours && frame.flags & END_STREAM != 0 => self.ended = true,
            RST_STREAM if ours => return Err(Error::Reset(code(&frame.payload, 0))),
            SETTINGS if frame.flags & ACK == 0 => write_frame(&mut self.wbuf, SETTINGS, ACK, 0, &[]),
            PING if frame.flags & ACK == 0 => write_frame(&mut self.wbuf, PING, ACK, 0, &frame.payload),
            GOAWAY => return Err(Error::GoAway(code(&frame.payload, 4))),
            _ => {}
        }
        Ok(())
    }
}

fn code(payload: &[u8], at: usize) -> u32 {
    payload.get(at..at + 4).map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hpack_integers() {
        let mut out = Vec::new();
        hpack_int(&mut out, 0, 7, 10);
        assert_eq!(out, [0x0a]);
        out.clear();
        hpack_int(&mut out, 0, 7, 127);
        assert_eq!(out, [0x7f, 0x00]);
        out.clear();
        hpack_int(&mut out, 0, 5, 1337);
        assert_eq!(out, [0x1f, 0x9a, 0x0a]);
        out.clear();
        hpack_int(&mut out, 0, 7, 300);
        assert_eq!(out, [0x7f, 0xad, 0x01]);
    }

    #[test]
    fn header_block_is_literal_fields() {
        let block = header_block("GetLyrics");
        let mut expect = Vec::new();
        for (n, v) in [
            (":method", "POST"),
            (":scheme", "http"),
            (":path", "/kopuz.v1.Kopuz/GetLyrics"),
            (":authority", "localhost"),
            ("content-type", "application/grpc"),
            ("te", "trailers"),
        ] {
            expect.push(0x00);
            expect.push(n.len() as u8);
            expect.extend_from_slice(n.as_bytes());
            expect.push(v.len() as u8);
            expect.extend_from_slice(v.as_bytes());
        }
        assert_eq!(block, expect);
        assert_eq!(&block[..7], &[0x00, 0x07, b':', b'm', b'e', b't', b'h']);
    }

    #[test]
    fn frame_header_layout() {
        let mut out = Vec::new();
        write_frame(&mut out, WINDOW_UPDATE, 0, 1, &70_000u32.to_be_bytes());
        assert_eq!(out, [0, 0, 4, 0x08, 0, 0, 0, 0, 1, 0x00, 0x01, 0x11, 0x70]);
    }
}
