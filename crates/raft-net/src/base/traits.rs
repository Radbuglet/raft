use std::{
    borrow::Cow,
    error::Error,
    fmt::{self, Write},
};

use bytes::{Buf, BufMut, Bytes};
use raft_utils::ctfe::format::CtfeFormatter;

// === Serde === //

pub trait Serde<A = ()>: Sized {
    fn decode_cx(cursor: &mut impl Buf, args: A) -> Result<Self, DecodeError>;

    fn encode_cx(&self, cursor: &mut impl BufMut, args: A);

    fn decode(cursor: &mut impl Buf) -> Result<Self, DecodeError>
    where
        A: Default,
    {
        Self::decode_cx(cursor, A::default())
    }

    fn encode(&self, cursor: &mut impl BufMut)
    where
        A: Default,
    {
        self.encode_cx(cursor, A::default());
    }
}

impl<const N: usize> Serde for [u8; N] {
    fn decode_cx(cursor: &mut impl Buf, _args: ()) -> Result<Self, DecodeError> {
        DecodeError::kinded("fixed-size byte array", || {
            let mut arr = [0; N];

            if cursor.try_copy_to_slice(&mut arr).is_err() {
                return Err(DecodeError::new_static(
                    const {
                        const {
                            let mut f = CtfeFormatter::<64>::new();

                            f.write_str("not enough data to consume ");
                            f.write_u128(N as u128);
                            f.write_str(" byte");
                            if N != 1 {
                                f.write_str("s");
                            }
                            f.write_str(" from the section.");

                            f
                        }
                        .finish()
                    },
                ));
            }

            Ok(arr)
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, _args: ()) {
        cursor.put_slice(self);
    }
}

impl Serde<usize> for Bytes {
    fn decode_cx(cursor: &mut impl Buf, len: usize) -> Result<Self, DecodeError> {
        DecodeError::kinded("dynamically-size byte array", || {
            if len > cursor.remaining() {
                return Err(DecodeError::new_string(format!(
                    "expected buffer of {} byte{} but only has {} byte{} remaining",
                    len,
                    if len != 1 { "s" } else { "" },
                    cursor.remaining(),
                    if cursor.remaining() != 1 { "s" } else { "" },
                )));
            }

            Ok(cursor.copy_to_bytes(len))
        })
    }

    fn encode_cx(&self, cursor: &mut impl BufMut, len: usize) {
        assert_eq!(self.len(), len);

        cursor.put_slice(self);
    }
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
