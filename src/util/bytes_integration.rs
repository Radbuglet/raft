use std::cell::Cell;

use bytes::{Buf, Bytes, BytesMut};

use super::{proto::byte_stream::ByteCursor, slice::detect_sub_slice};

// === Snip === //

pub trait Snip {
    fn freeze_range(&self, subset: &[u8]) -> Bytes;
}

impl Snip for Bytes {
    fn freeze_range(&self, subset: &[u8]) -> Bytes {
        self.slice_ref(subset)
    }
}

impl Snip for BytesMut {
    fn freeze_range(&self, subset: &[u8]) -> Bytes {
        if subset.is_empty() {
            Bytes::new()
        } else {
            self.clone()
                .freeze()
                .slice(detect_sub_slice(&*self, subset).unwrap())
        }
    }
}

// === ByteMutReadSession === //

#[derive(Debug)]
pub struct ByteMutReadSession<'a> {
    bytes: &'a mut BytesMut,
    post_op: Cell<PostOp>,
}

#[derive(Debug, Copy, Clone)]
enum PostOp {
    Reserve(usize),
    Consume(usize),
}

impl<'a> ByteMutReadSession<'a> {
    pub fn new(bytes: &'a mut BytesMut) -> Self {
        Self {
            bytes,
            post_op: Cell::new(PostOp::Reserve(0)),
        }
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
        self.consume(cursor.pos());
    }
}

impl Snip for ByteMutReadSession<'_> {
    fn freeze_range(&self, subset: &[u8]) -> Bytes {
        self.bytes.freeze_range(subset)
    }
}

impl Drop for ByteMutReadSession<'_> {
    fn drop(&mut self) {
        match self.post_op.get() {
            PostOp::Reserve(count) => self.bytes.reserve(count),
            PostOp::Consume(count) => self.bytes.advance(count),
        }
    }
}
