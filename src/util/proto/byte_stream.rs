use std::{fmt, io, str};

use crate::util::format::lazy_format;

use super::{decode_seq::ReadCursor, encode::WriteStream};

// === Reader === //

#[derive(Debug, Clone)]
pub struct ByteCursor<'a> {
    original: &'a [u8],
    remaining: &'a [u8],
}

impl<'a> ByteCursor<'a> {
    pub const fn new(buf: &'a [u8]) -> Self {
        Self {
            original: buf,
            remaining: buf,
        }
    }

    pub fn original(&self) -> &'a [u8] {
        self.original
    }

    pub fn remaining(&self) -> &'a [u8] {
        self.remaining
    }

    pub fn pos(&self) -> usize {
        self.original.len() - self.remaining.len()
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.remaining = &self.original[pos..];
    }

    pub fn with_pos(self, pos: usize) -> Self {
        let mut fork = self.clone();
        fork.set_pos(pos);
        fork
    }

    pub fn len(&self) -> usize {
        self.remaining.len()
    }

    pub fn is_empty(&self) -> bool {
        self.remaining.is_empty()
    }

    pub fn advance_remaining(&mut self) {
        self.remaining = &[];
    }

    pub fn read(&mut self) -> Option<u8> {
        self.read_arr::<1>().map(|[v]| v)
    }

    pub fn read_slice(&mut self, count: usize) -> Option<&'a [u8]> {
        let res = self.remaining.get(0..count)?;
        self.remaining = &self.remaining[count..];

        Some(res)
    }

    pub fn read_arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.read_slice(N).map(|slice| slice.try_into().unwrap())
    }

    pub fn format_location(&self) -> impl fmt::Display {
        let read_count = self.pos();
        lazy_format!("{read_count} byte(s) from the packet frame start")
    }
}

impl ReadCursor for ByteCursor<'_> {
    type Pos = usize;

    fn pos(&self) -> Self::Pos {
        self.pos()
    }

    fn set_pos(&mut self, pos: Self::Pos) {
        self.set_pos(pos);
    }
}

// === Write as Stream === //

impl<T: io::Write> WriteStream<[u8]> for T {
    type Error = io::Error;

    fn push(&mut self, elem: &[u8]) -> io::Result<()> {
        self.write_all(elem)
    }
}

// === Stream as Write === //

pub trait ByteWriteStream: WriteStream<[u8]> {
    fn as_write(&mut self) -> AdaptWriteStream<'_, Self> {
        AdaptWriteStream(self)
    }
}

impl<T: ?Sized + WriteStream<[u8]>> ByteWriteStream for T {}

pub struct AdaptWriteStream<'a, S: ?Sized>(&'a mut S);

impl<S: ByteWriteStream> io::Write for AdaptWriteStream<'_, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.push(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// === Write helpers === //

#[derive(Debug, Clone, Default)]
pub struct WriteByteCounter(pub usize);

impl io::Write for WriteByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0 += buf.len();

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteCodepointCounter {
    buffer: [u8; 4],
    offset: u8,
    codepoints: usize,
    bytes: usize,
}

impl io::Write for WriteCodepointCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.bytes += buf.len();

        if self.codepoints == usize::MAX {
            return Ok(buf.len());
        }

        for &byte in buf {
            self.buffer[self.offset as usize] = byte;

            if str::from_utf8(&self.buffer[0..self.offset as usize]).is_ok() {
                self.offset = 0;
                self.codepoints += 1;
            } else {
                self.offset += 1;

                if self.offset >= 4 {
                    self.codepoints = usize::MAX;
                    return Ok(buf.len());
                }
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WriteCodepointCounter {
    pub fn codepoints(&self) -> Option<usize> {
        (self.codepoints != usize::MAX).then_some(self.codepoints)
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
