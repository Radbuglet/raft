use std::{fmt, io::Write, mem::MaybeUninit};

use super::{byte_cursor::ByteReadCursor, write::WriteByteCounter};

// === Common === //

pub trait Codec: Sized + 'static {}

// === Deserialize === //

#[derive(Debug, Default)]
pub struct Meta {
    buffer: Vec<u8>,
}

impl Meta {
    pub fn reserve(&mut self) -> MetaBuilder<'_> {
        MetaBuilder::new(&mut self.buffer)
    }

    pub fn fetch(&self, offset: usize) -> &[u8] {
        &self.buffer[offset..]
    }
}

#[derive(Debug)]
pub struct MetaBuilder<'a> {
    start: usize,
    buffer: &'a mut Vec<u8>,
}

impl<'a> MetaBuilder<'a> {
    fn new(buffer: &'a mut Vec<u8>) -> Self {
        let start = buffer.len();
        assert!(start < isize::MAX as usize);

        Self { start, buffer }
    }

    pub fn handle(&self) -> usize {
        self.start
    }

    // === Capacity Management === //

    pub fn capacity(&self) -> usize {
        self.buffer.capacity()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.buffer.reserve(additional);
    }

    pub fn reserve_exact(&mut self, additional: usize) {
        self.buffer.reserve_exact(additional);
    }

    pub fn shrink_to_fit(&mut self) {
        self.buffer.shrink_to_fit();
    }

    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.buffer.shrink_to(min_capacity);
    }

    // === Slice Manipulation === //

    pub fn as_slice(&self) -> &[u8] {
        &self.buffer[self.start..]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buffer[self.start..]
    }

    pub fn as_ptr(&self) -> *const u8 {
        // N.B. we do things this way instead of using the slice's method to allow users to write to
        // the extra capacity without provenance issues.
        unsafe {
            // Safety: the handle is at most one byte out of the range of the allocation, which is
            // allowed. Additionally, we always ensure that the handle is less than `isize::MAX`
            // before creating this object.
            self.buffer.as_ptr().add(self.start)
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        // N.B. we do things this way instead of using the slice's method to allow users to write to
        // the extra capacity without provenance issues.
        unsafe {
            // Safety: the handle is at most one byte out of the range of the allocation, which is
            // allowed. Additionally, we always ensure that the handle is less than `isize::MAX`
            // before creating this object.
            self.buffer.as_mut_ptr().add(self.start)
        }
    }

    pub unsafe fn set_len(&mut self, new_len: usize) {
        // Safety: provided by caller; the validity of this length immediately implies that
        // `start + new_len` won't overflow since we'd need those bytes to be initialized.
        self.buffer.set_len(self.start + new_len);
    }

    pub fn spare_capacity_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        self.buffer.spare_capacity_mut()
    }

    // === Primitives === //

    pub fn push(&mut self, byte: u8) {
        self.buffer.push(byte);
    }

    pub fn push_slice(&mut self, other: &[u8]) {
        self.buffer.extend_from_slice(other);
    }

    pub fn push_iter<I: IntoIterator<Item = u8>>(&mut self, iter: I) {
        self.buffer.extend(iter);
    }

    pub fn clear(&mut self) {
        self.buffer.truncate(self.start);
    }

    pub fn discard(mut self) {
        self.clear();
    }

    pub fn len(&self) -> usize {
        self.buffer.len() - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait Deserialize<C: Codec>: Sized {
    /// A summary of the deserialized contents. This includes enough information to:
    ///
    /// 1. Determine the position of sub-fields quickly given the starting position of the object.
    /// 2. Decode its contents quickly given the backing byte array and an additional metadata buffer
    ///    generated during summarization.
    /// 3. Determine the starting position of the next object quickly given the starting position of
    ///    this object.
    ///
    type Summary: fmt::Debug + Clone;

    /// A user-friendly view into the contents of this object given a bound backing and metadata buffer.
    ///
    /// This view should be lazily evaluated and must provide a mechanism for reifying the view into
    /// its regular type.
    type View<'a>: fmt::Debug + Copy + Into<Self>
    where
        Self: 'a;
}

pub trait DeserializeFor<C: Codec, A>: Deserialize<C> {
    /// Validates and summarizes an encoded byte stream, leaving the `cursor` head at the position of
    /// the next item if it exists.
    fn summarize(
        meta: &mut Meta,
        cursor: &mut ByteReadCursor,
        args: A,
    ) -> anyhow::Result<Self::Summary>;

    /// Produces a user-friendly view of a summary. It should always be valid to construct this view
    /// since summarization should have already performed all the necessary validation.
    fn view<'a>(summary: &'a Self::Summary, info: DeserializeInfo<'a>, args: A) -> Self::View<'a>;

    /// Returns the absolute byte-position of the object right after this one given its own absolute
    /// starting byte-position.
    fn end(summary: &Self::Summary, info: DeserializeInfo<'_>, args: A) -> usize;
}

#[derive(Debug, Copy, Clone)]
pub struct DeserializeInfo<'a> {
    pub start: usize,
    pub meta: &'a Meta,
    pub stream: &'a [u8],
}

impl DeserializeInfo<'_> {
    pub fn with_start(self, start: usize) -> Self {
        Self {
            start,
            meta: self.meta,
            stream: self.stream,
        }
    }

    pub fn stream_sub(&self) -> &[u8] {
        &self.stream[self.start..]
    }
}

// === Serialize === //

pub trait SerializeInto<C: Codec, T>: Sized {}

pub trait SerializeIntoFor<C: Codec, T, A>: SerializeInto<C, T> {
    fn serialize(&self, stream: &mut impl Write, args: A) -> anyhow::Result<()>;

    fn length(&self, args: A) -> anyhow::Result<usize> {
        let mut counter = WriteByteCounter::default();
        self.serialize(&mut counter, args)?;
        Ok(counter.0)
    }
}

// === Struct === //

pub mod codec_struct_internals {
    pub use {
        super::{
            Deserialize, DeserializeFor, DeserializeInfo, Meta, SerializeInto, SerializeIntoFor,
        },
        crate::util::byte_cursor::ByteReadCursor,
        anyhow,
        std::{
            convert::{identity, From},
            fmt,
            io::Write,
            primitive::usize,
            result::Result::Ok,
            stringify,
        },
    };
}

macro_rules! codec_struct {
    ($(
        $(#[$attr:meta])*
        $struct_vis:vis struct $mod_name:ident::$struct_name:ident($codec:ty) {
            $($field_name:ident: $field_ty:ty $(=> $config_ty:ty : $config:expr)?),*
            $(,)?
        }
    )*) => {$(
        $struct_vis mod $mod_name {
            #[allow(unused_imports)]
            use super::*;

            // Structure definitions
            $(#[$attr])*
            pub struct $struct_name {
                $(pub $field_name: $field_ty,)*
            }

            #[derive(Debug, Copy, Clone)]
            pub struct Summary {
                $($field_name: <$field_ty as $crate::util::byte_codec::codec_struct_internals::Deserialize<$codec>>::Summary,)*
            }

            #[derive(Copy, Clone)]
            pub struct View<'a> {
                summary: &'a Summary,
                info: $crate::util::byte_codec::codec_struct_internals::DeserializeInfo<'a>,
            }

            #[derive(Debug, Copy, Clone)]
            #[allow(non_camel_case_types)]
            pub struct Builder<$($field_name,)*> {
                $(pub $field_name: $field_name,)*
            }

            // Deserialization
            impl $crate::util::byte_codec::codec_struct_internals::Deserialize<$codec> for $struct_name {
                type Summary = Summary;
                type View<'a> = View<'a>
                where
                    Self: 'a;
            }

            impl $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ()> for $struct_name {
                fn summarize(
                    meta: &mut $crate::util::byte_codec::codec_struct_internals::Meta,
                    cursor: &mut $crate::util::byte_codec::codec_struct_internals::ByteReadCursor,
                    _args: (),
                ) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<Self::Summary> {
                    $crate::util::byte_codec::codec_struct_internals::Ok(Self::Summary {
						#[allow(unused_parens)]
                        $($field_name: <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor::<$codec, ($($config_ty)?)>>::summarize(
                            meta,
                            cursor,
                            {$($config)?},
                        )?,)*
                    })
                }

                fn view<'a>(
                    summary: &'a Self::Summary,
                    info: $crate::util::byte_codec::codec_struct_internals::DeserializeInfo<'a>,
                    _args: (),
                ) -> Self::View<'a> {
                    Self::View { summary, info }
                }

                fn end(
                    summary: &Self::Summary,
                    info: $crate::util::byte_codec::codec_struct_internals::DeserializeInfo<'_>,
                    _args: (),
                ) -> $crate::util::byte_codec::codec_struct_internals::usize {
					let offset = info.start;
                    $(
						#[allow(unused_parens)]
                        let offset = <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::end(
                            &summary.$field_name,
                            info.with_start(offset),
                            {$($config)?},
                        );
                    )*

                    offset
                }
            }

            // View accessors
			struct OffsetsTmp {
				$($field_name: $crate::util::byte_codec::codec_struct_internals::usize,)*
			}

			impl View<'_> {
				fn offsets(&self) -> OffsetsTmp {
					let offset = self.info.start;
                    $(
						#[allow(unused_parens)]
                        let offset = <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::end(
                            &self.summary.$field_name,
                            self.info.with_start(offset),
                            {$($config)?},
                        );
						let $field_name = offset;
                    )*

					OffsetsTmp { $($field_name,)* }
				}
			}

            impl<'a> View<'a> {$(
                pub fn $field_name(&self) -> <$field_ty as $crate::util::byte_codec::codec_struct_internals::Deserialize<$codec>>::View<'a> {
					let offset = self.offsets().$field_name;

					<$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::view(
						&self.summary.$field_name,
						self.info.with_start(offset),
						{$($config)?},
					)
                }
            )*}

            // View reification
            impl $crate::util::byte_codec::codec_struct_internals::From<View<'_>> for $struct_name {
                fn from(view: View<'_>) -> Self {
                    Self {
                        $($field_name: $crate::util::byte_codec::codec_struct_internals::From::from(view.$field_name()),)*
                    }
                }
            }

            // View formatting
            impl $crate::util::byte_codec::codec_struct_internals::fmt::Debug for View<'_> {
                fn fmt(&self, f: &mut $crate::util::byte_codec::codec_struct_internals::fmt::Formatter<'_>) -> $crate::util::byte_codec::codec_struct_internals::fmt::Result {
                    f.debug_struct($crate::util::byte_codec::codec_struct_internals::stringify!($struct_name))
                        $(.field(
                            $crate::util::byte_codec::codec_struct_internals::stringify!($field_name),
                            &self.$field_name(),
                        ))*
                        .finish()
                }
            }

            // Serialization
            #[allow(non_camel_case_types)]
            impl<$($field_name,)*> $crate::util::byte_codec::codec_struct_internals::SerializeInto<$codec, $struct_name> for Builder<$($field_name,)*>
            where
                $($field_name: $crate::util::byte_codec::codec_struct_internals::SerializeInto<$codec, $field_ty>,)*
            {
            }

            #[allow(non_camel_case_types, unused_parens)]
            impl<$($field_name,)*> $crate::util::byte_codec::codec_struct_internals::SerializeIntoFor<$codec, $struct_name, ()> for Builder<$($field_name,)*>
            where
                $($field_name: $crate::util::byte_codec::codec_struct_internals::SerializeIntoFor<$codec, $field_ty, ($($config_ty)?)>,)*
            {
                fn serialize(
					&self,
					stream: &mut impl $crate::util::byte_codec::codec_struct_internals::Write,
					_args: (),
				) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<()> {
					$(
						$crate::util::byte_codec::codec_struct_internals::SerializeIntoFor::<$codec, $field_ty, ($($config_ty)?)>::serialize(
							&self.$field_name,
							stream,
							{$($config)?},
						)?;
					)*

					$crate::util::byte_codec::codec_struct_internals::Ok(())
				}

                fn length(&self, _args: ()) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<$crate::util::byte_codec::codec_struct_internals::usize> {
                    $crate::util::byte_codec::codec_struct_internals::Ok(
						0 $(+ $crate::util::byte_codec::codec_struct_internals::SerializeIntoFor::<$codec, $field_ty, ($($config_ty)?)>::length(
							&self.$field_name,
							{$($config)?},
						)?)*
					)
                }
            }
        }

        #[allow(unused_imports)]
        $struct_vis use $mod_name::$struct_name;
    )*};
}

pub(crate) use codec_struct;
