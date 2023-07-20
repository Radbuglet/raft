use std::cell::Cell;

use bytes::{Buf, Bytes, BytesMut};

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

    pub fn frozen_bytes(&self) -> Bytes {
        self.bytes.clone().freeze()
    }

    pub fn freeze_range(&self, subset: &[u8]) -> Bytes {
        self.frozen_bytes().slice_ref(subset)
    }

    pub fn bytes(&self) -> &BytesMut {
        &self.bytes
    }

    pub fn cursor(&self) -> ByteCursor<'_> {
        ByteCursor::new(&self.bytes)
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

    pub fn consume_cursor(&self, cursor: &ByteCursor) {
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
        let res = self.remaining.try_into().ok()?;
        self.remaining = &self.remaining[N..];

        Some(res)
    }
}
