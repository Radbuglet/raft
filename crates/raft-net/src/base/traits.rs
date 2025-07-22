use std::{
    borrow::Cow,
    collections::{VecDeque, vec_deque},
    error::Error,
    fmt::{self, Write},
};

use raft_utils::ctfe::format::CtfeFormatter;

// === Encode === //

pub trait Encode<C: EncodeCursor, A = ()> {
    fn encode(&self, cursor: &mut C, args: A);
}

pub trait EncodeCursor: Sized {
    fn write_slice(&mut self, data: &[u8]);
}

// === DecodeError === //

#[derive(Debug, Clone)]
#[must_use]
pub struct DecodeError {
    item_path: Vec<ItemPathPart>,
    item_kind: Option<&'static str>,
    message: Cow<'static, str>,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub enum ItemPathPart {
    Named(&'static str),
    Indexed(usize),
}

impl fmt::Display for ItemPathPart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ItemPathPart::Named(v) => v.fmt(f),
            ItemPathPart::Indexed(v) => v.fmt(f),
        }
    }
}

impl From<&'static str> for ItemPathPart {
    fn from(value: &'static str) -> Self {
        Self::Named(value)
    }
}

impl From<usize> for ItemPathPart {
    fn from(value: usize) -> Self {
        Self::Indexed(value)
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("error while reading ")?;

        if let Some(primitive) = self.item_kind {
            f.write_str(primitive)?;
            f.write_str(" at ")?;
        }

        f.write_char('`')?;

        for (i, part) in self.item_path.iter().rev().enumerate() {
            if i > 0 {
                f.write_char('.')?;
            }

            part.fmt(f)?;
        }

        f.write_char('`')?;

        f.write_str(": ")?;
        f.write_str(&self.message)?;

        Ok(())
    }
}

impl Error for DecodeError {}

impl DecodeError {
    pub const fn new_cow(message: Cow<'static, str>) -> Self {
        Self {
            item_path: Vec::new(),
            item_kind: None,
            message,
        }
    }

    pub const fn new_static(message: &'static str) -> Self {
        Self::new_cow(Cow::Borrowed(message))
    }

    pub fn new_string(message: impl Into<String>) -> Self {
        Self::new_cow(Cow::Owned(message.into()))
    }

    pub fn new_fmt(message: impl fmt::Display) -> Self {
        Self::new_cow(Cow::Owned(message.to_string()))
    }

    pub fn set_kind(&mut self, kind: &'static str) {
        self.item_kind = Some(kind);
    }

    pub fn with_kind(mut self, kind: &'static str) -> Self {
        self.set_kind(kind);
        self
    }

    pub fn push_path(&mut self, scope: impl Into<ItemPathPart>) {
        self.item_path.push(scope.into());
    }

    pub fn with_path(mut self, scope: impl Into<ItemPathPart>) -> Self {
        self.push_path(scope);
        self
    }

    pub fn kinded<R>(
        kind: &'static str,
        f: impl FnOnce() -> Result<R, DecodeError>,
    ) -> Result<R, DecodeError> {
        f().map_err(|v| v.with_kind(kind))
    }

    pub fn pathed<R>(
        scope: impl Into<ItemPathPart>,
        f: impl FnOnce() -> Result<R, DecodeError>,
    ) -> Result<R, DecodeError> {
        f().map_err(|v| v.with_path(scope))
    }
}

// === Decode === //

pub trait Decode<C: DecodeCursor, A = ()>: Sized {
    fn decode(cursor: &mut C, _args: A) -> Result<Self, DecodeError>;
}

pub trait DecodeCursor: Sized {
    fn read<const N: usize>(&mut self) -> Result<[u8; N], DecodeError>;

    fn remaining(&self) -> usize;

    fn is_done(&self) -> bool {
        self.remaining() == 0
    }

    fn finish(&self) -> Result<(), DecodeError> {
        if !self.is_done() {
            let len = self.remaining();

            return Err(DecodeError::new_fmt(format_args!(
                "expected end of buffer but {len} byte{} remaining",
                if len == 1 { " is" } else { "s are" },
            )));
        }

        Ok(())
    }
}

pub trait ContiguousDecodeCursor: DecodeCursor {
    type Slice: AsRef<[u8]>;

    fn read_slice(&mut self, len: usize) -> Result<Self::Slice, DecodeError>;

    fn advance(&mut self, len: usize) -> Result<(), DecodeError>;
}

// === FrameCursor === //

#[derive(Debug)]
pub struct FrameCursor<'a> {
    cursor: vec_deque::Iter<'a, u8>,
}

impl<'a> FrameCursor<'a> {
    pub fn new(queue: &'a VecDeque<u8>) -> Self {
        Self {
            cursor: queue.iter(),
        }
    }
}

impl DecodeCursor for FrameCursor<'_> {
    fn read<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let err = const {
            const {
                let mut fmt = CtfeFormatter::<48>::new();
                fmt.write_str("buffer not large enough to decode ");
                fmt.write_u128(N as u128);
                fmt.write_str(" byte");

                if N != 1 {
                    fmt.write_char('s');
                }

                fmt
            }
            .finish()
        };

        if self.remaining() < N {
            return Err(DecodeError::new_static(err));
        }

        todo!()
    }

    fn remaining(&self) -> usize {
        self.cursor.len()
    }
}
