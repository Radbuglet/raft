use std::{cell::Cell, fmt};

use bytes::{Buf, Bytes, BytesMut};

use super::format::lazy_format;

#[derive(Debug)]
pub struct ByteReadSession<'a> {
    bytes: &'a mut BytesMut,
    post_op: Cell<PostOp>,
}

#[derive(Debug, Copy, Clone)]
enum PostOp {
    Reserve(usize),
    Consume(usize),
}

impl<'a> ByteReadSession<'a> {
    pub fn new(bytes: &'a mut BytesMut) -> Self {
        Self {
            bytes,
            post_op: Cell::new(PostOp::Reserve(0)),
        }
    }

    pub fn freeze_range(&self, subset: &[u8]) -> Bytes {
        // Adapted from `Bytes::slice_ref`.

        // Empty slice and empty Bytes may have their pointers reset
        // so explicitly allow empty slice to be a sub-slice of any slice.
        if subset.is_empty() {
            return Bytes::new();
        }

        let bytes_p = self.bytes.as_ptr() as usize;
        let bytes_len = self.bytes.len();

        let sub_p = subset.as_ptr() as usize;
        let sub_len = subset.len();

        assert!(
            sub_p >= bytes_p,
            "subset pointer ({:p}) is smaller than self pointer ({:p})",
            subset.as_ptr(),
            self.bytes.as_ptr(),
        );
        assert!(
            sub_p + sub_len <= bytes_p + bytes_len,
            "subset is out of bounds: self = ({:p}, {}), subset = ({:p}, {})",
            self.bytes.as_ptr(),
            bytes_len,
            subset.as_ptr(),
            sub_len,
        );

        let sub_offset = sub_p - bytes_p;

        self.bytes
            .clone()
            .freeze()
            .slice(sub_offset..(sub_offset + sub_len))
    }

    pub fn bytes(&self) -> &BytesMut {
        &self.bytes
    }

    pub fn cursor(&self) -> ByteReadCursor<'_> {
        ByteReadCursor::new(&self.bytes)
    }

    pub fn reserve(&self, additional: usize) {
        self.post_op.set(match self.post_op.get() {
            PostOp::Reserve(old) => PostOp::Reserve(old.max(additional)),
            PostOp::Consume(_) => {
                log::warn!("Cannot reserve additional memory after committing to a consumption.");
                return;
            }
        });
    }

    pub fn consume(&self, count: usize) {
        self.post_op.set(PostOp::Consume(
            match self.post_op.get() {
                PostOp::Reserve(_) => 0,
                PostOp::Consume(old) => old,
            } + count,
        ));
    }

    pub fn consume_cursor(&self, cursor: &ByteReadCursor) {
        self.consume(cursor.read_count());
    }
}

impl Drop for ByteReadSession<'_> {
    fn drop(&mut self) {
        match self.post_op.get() {
            PostOp::Reserve(count) => self.bytes.reserve(count),
            PostOp::Consume(count) => self.bytes.advance(count),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ByteReadCursor<'a> {
    original: &'a [u8],
    remaining: &'a [u8],
}

impl<'a> ByteReadCursor<'a> {
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

    pub fn read_count(&self) -> usize {
        self.original.len() - self.remaining.len()
    }

    pub fn len(&self) -> usize {
        self.remaining.len()
    }

    pub fn is_empty(&self) -> bool {
        self.remaining.is_empty()
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
        let read_count = self.read_count();
        lazy_format!("{read_count} byte(s) from the packet frame start")
    }
}
