//! Length-prefixed frames: a u32 big-endian body length, then the body.
//!
//! The length is checked against a maximum before anything is allocated, so a
//! corrupt or hostile prefix cannot make either side reserve gigabytes.

use std::fmt;
use std::io::{self, Read, Write};

/// The largest body either side accepts.
pub const MAX_FRAME: usize = 32 * 1024 * 1024;

const HEADER: usize = 4;

#[derive(Debug)]
pub enum FrameError {
    /// A frame announced (or was asked to carry) more than the maximum.
    TooLarge { len: usize, max: usize },
    /// The stream ended inside a frame.
    Truncated { expected: usize, got: usize },
    Io(io::Error),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => {
                write!(f, "frame of {len} bytes exceeds the {max}-byte limit")
            }
            Self::Truncated { expected, got } => {
                write!(f, "stream ended {got} bytes into a {expected}-byte frame")
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FrameError {}

impl From<io::Error> for FrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// `body` as one frame.
pub fn encode(body: &[u8], max: usize) -> Result<Vec<u8>, FrameError> {
    let len = checked_len(body.len(), max)?;
    let mut frame = Vec::with_capacity(HEADER + body.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(body);
    Ok(frame)
}

/// Writes `body` as one frame and flushes, so the peer sees it at once.
pub fn write_frame(writer: &mut impl Write, body: &[u8], max: usize) -> Result<(), FrameError> {
    let len = checked_len(body.len(), max)?;
    writer.write_all(&len.to_be_bytes())?;
    writer.write_all(body)?;
    writer.flush()?;
    Ok(())
}

/// Reads one frame, blocking. `Ok(None)` when the stream ends cleanly between
/// frames.
pub fn read_frame(reader: &mut impl Read, max: usize) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0u8; HEADER];
    let got = read_full(reader, &mut header)?;
    if got == 0 {
        return Ok(None);
    }
    if got < HEADER {
        return Err(FrameError::Truncated {
            expected: HEADER,
            got,
        });
    }
    let len = u32::from_be_bytes(header) as usize;
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    // Grows with what actually arrives rather than trusting the prefix.
    let mut body = Vec::new();
    reader.take(len as u64).read_to_end(&mut body)?;
    if body.len() < len {
        return Err(FrameError::Truncated {
            expected: len,
            got: body.len(),
        });
    }
    Ok(Some(body))
}

/// Decodes frames from bytes that arrive in any split.
pub struct FrameReader {
    buf: Vec<u8>,
    /// Bytes of `buf` already handed out.
    start: usize,
    max: usize,
}

impl FrameReader {
    pub fn new(max: usize) -> Self {
        Self {
            buf: Vec::new(),
            start: 0,
            max,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 && self.start * 2 >= self.buf.len() {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// The next whole frame, or `None` until more bytes arrive. After an
    /// error the stream is unusable: framing has been lost.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError> {
        let pending = &self.buf[self.start..];
        if pending.len() < HEADER {
            return Ok(None);
        }
        let len = u32::from_be_bytes(pending[..HEADER].try_into().expect("4 bytes")) as usize;
        if len > self.max {
            return Err(FrameError::TooLarge { len, max: self.max });
        }
        if pending.len() < HEADER + len {
            return Ok(None);
        }
        let body = pending[HEADER..HEADER + len].to_vec();
        self.start += HEADER + len;
        Ok(Some(body))
    }

    /// Call at end of stream: an error when it stopped inside a frame.
    pub fn finish(&self) -> Result<(), FrameError> {
        let pending = &self.buf[self.start..];
        if pending.is_empty() {
            return Ok(());
        }
        let expected = if pending.len() < HEADER {
            HEADER
        } else {
            HEADER + u32::from_be_bytes(pending[..HEADER].try_into().expect("4 bytes")) as usize
        };
        Err(FrameError::Truncated {
            expected,
            got: pending.len(),
        })
    }
}

fn checked_len(len: usize, max: usize) -> Result<u32, FrameError> {
    if len > max {
        return Err(FrameError::TooLarge { len, max });
    }
    u32::try_from(len).map_err(|_| FrameError::TooLarge { len, max })
}

/// Reads until `buf` is full or the stream ends; how many bytes arrived.
fn read_full(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match reader.read(&mut buf[got..]) {
            Ok(0) => break,
            Ok(n) => got += n,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(got)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn frames(bodies: &[&[u8]]) -> Vec<u8> {
        bodies
            .iter()
            .flat_map(|body| encode(body, MAX_FRAME).unwrap())
            .collect()
    }

    fn drain(reader: &mut FrameReader) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while let Some(frame) = reader.next_frame().unwrap() {
            out.push(frame);
        }
        out
    }

    #[test]
    fn a_frame_is_a_big_endian_length_then_the_body() {
        assert_eq!(encode(b"abc", MAX_FRAME).unwrap(), [0, 0, 0, 3, b'a', b'b', b'c']);
        assert_eq!(encode(b"", MAX_FRAME).unwrap(), [0, 0, 0, 0]);
    }

    #[test]
    fn partial_and_combined_reads_yield_whole_frames() {
        let stream = frames(&[b"hello", b"", b"world!"]);
        let mut reader = FrameReader::new(MAX_FRAME);
        reader.push(&stream[..3]);
        assert_eq!(reader.next_frame().unwrap(), None, "half a header");
        reader.push(&stream[3..7]);
        assert_eq!(reader.next_frame().unwrap(), None, "header without the body");
        reader.push(&stream[7..]);
        assert_eq!(drain(&mut reader), [b"hello".to_vec(), Vec::new(), b"world!".to_vec()]);
        reader.finish().unwrap();
    }

    #[test]
    fn an_oversize_length_is_refused_before_the_body_arrives() {
        let mut reader = FrameReader::new(16);
        reader.push(&17u32.to_be_bytes());
        assert!(matches!(
            reader.next_frame(),
            Err(FrameError::TooLarge { len: 17, max: 16 })
        ));
        // A length of 4 GiB costs nothing: only the header is ever read.
        let mut blocking = &u32::MAX.to_be_bytes()[..];
        assert!(matches!(
            read_frame(&mut blocking, MAX_FRAME),
            Err(FrameError::TooLarge { .. })
        ));
        assert!(matches!(
            encode(&[0; 17], 16),
            Err(FrameError::TooLarge { len: 17, max: 16 })
        ));
    }

    #[test]
    fn a_stream_that_stops_inside_a_frame_is_truncated() {
        let stream = frames(&[b"hello"]);
        let mut reader = FrameReader::new(MAX_FRAME);
        reader.push(&stream[..6]);
        assert_eq!(reader.next_frame().unwrap(), None);
        assert!(matches!(
            reader.finish(),
            Err(FrameError::Truncated { expected: 9, got: 6 })
        ));
        let mut header_only = &stream[..2];
        assert!(matches!(
            read_frame(&mut header_only, MAX_FRAME),
            Err(FrameError::Truncated { expected: 4, got: 2 })
        ));
        let mut body_short = &stream[..6];
        assert!(matches!(
            read_frame(&mut body_short, MAX_FRAME),
            Err(FrameError::Truncated { expected: 5, got: 2 })
        ));
    }

    #[test]
    fn blocking_reads_and_writes_round_trip_and_end_cleanly() {
        let mut stream = Vec::new();
        write_frame(&mut stream, b"one", MAX_FRAME).unwrap();
        write_frame(&mut stream, b"two", MAX_FRAME).unwrap();
        let mut reader = &stream[..];
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap().unwrap(), b"one");
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap().unwrap(), b"two");
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap(), None);
    }

    /// A reader that hands out one byte per call, as a slow pipe may.
    struct Trickle<'a>(&'a [u8]);

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            match self.0.split_first() {
                Some((byte, rest)) if !buf.is_empty() => {
                    buf[0] = *byte;
                    self.0 = rest;
                    Ok(1)
                }
                _ => Ok(0),
            }
        }
    }

    #[test]
    fn a_blocking_read_assembles_a_frame_from_short_reads() {
        let stream = frames(&[b"slowly"]);
        let mut reader = Trickle(&stream);
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap().unwrap(), b"slowly");
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap(), None);
    }

    proptest! {
        #[test]
        fn any_split_of_a_stream_decodes_to_the_same_frames(
            bodies in prop::collection::vec(prop::collection::vec(any::<u8>(), 0..64), 0..12),
            cuts in prop::collection::vec(any::<prop::sample::Index>(), 0..16),
        ) {
            let refs: Vec<&[u8]> = bodies.iter().map(Vec::as_slice).collect();
            let stream = frames(&refs);
            let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(stream.len() + 1)).collect();
            points.sort_unstable();
            let mut reader = FrameReader::new(MAX_FRAME);
            let mut decoded = Vec::new();
            let mut last = 0;
            for point in points.into_iter().chain([stream.len()]) {
                reader.push(&stream[last..point]);
                last = point;
                decoded.extend(drain(&mut reader));
            }
            prop_assert_eq!(decoded, bodies);
            prop_assert!(reader.finish().is_ok());
        }
    }
}
