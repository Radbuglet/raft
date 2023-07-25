use std::{fmt, io::Write};

use super::{byte_cursor::ByteReadCursor, write::WriteByteCounter};

// === Common === //

pub trait Codec: Sized + 'static {}

// === Deserialize === //

pub struct NoExternalCall {
    _private: (),
}

pub trait Deserialize<C: Codec>: Sized {
    /// A summary of the deserialized contents. This includes enough information to:
    ///
    /// 1. Determine the position of sub-fields quickly given the starting position of the object.
    /// 2. Decode its contents quickly given the backing byte array.
    /// 3. Determine the starting position of the next object quickly given the starting position of
    ///    this object.
    ///
    type Summary: fmt::Debug + Clone;

    /// A user-friendly view into the contents of this object given a bound backing buffer.
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
    fn summarize(cursor: &mut ByteReadCursor, args: &mut A) -> anyhow::Result<Self::Summary>;

    /// Produces a user-friendly view of a summary. It should always be valid to construct this view
    /// since summarization should have already performed all the necessary validation.
    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteReadCursor<'a>,
        args: &mut A,
    ) -> Self::View<'a> {
        Self::view_(NoExternalCall { _private: () }, summary, cursor, args)
    }

    /// Produces a user-friendly view of a summary. It should always be valid to construct this view
    /// since summarization should have already performed all the necessary validation.
    ///
    /// This method cannot be called externally and is purely intended to be used to derive the
    /// `unsafe` [`view`](DeserializeFor::view) method without "leaking" an `unsafe` context so
    /// implement this method instead of overwriting `view`. This method is allowed to make all the
    /// same assumptions `view` makes.
    fn view_<'a>(
        _no_external_call: NoExternalCall,
        summary: &'a Self::Summary,
        cursor: ByteReadCursor<'a>,
        args: &mut A,
    ) -> Self::View<'a>;

    /// Returns the absolute byte-position of the object right after this one given its own absolute
    /// starting byte-position.
    fn end(summary: &Self::Summary, cursor: ByteReadCursor, args: &mut A) -> usize;

    /// Decodes the reified version of this value in a single pass.
    fn decode<'a>(cursor: &'a mut ByteReadCursor, args: &mut A) -> anyhow::Result<Self> {
        let fork = cursor.clone();
        let summary = Self::summarize(cursor, args)?;
        let view = unsafe {
            // Safety: we just generated this summary with the appropriate cursor.
            Self::view(&summary, fork, args)
        };

        Ok(view.into())
    }
}

// === DeserializeForSimples === //

pub trait DeserializeForSimple<C: Codec, A>: 'static + Deserialize<C> {
    fn decode_simple<'a>(
        cursor: &mut ByteReadCursor<'a>,
        args: &mut A,
    ) -> anyhow::Result<Self::View<'a>>;
}

pub trait SimpleSummary: 'static + fmt::Debug + Copy {
    fn from_pos(pos: usize) -> Self;

    fn detect_end<C, A, T>(self, cursor: ByteReadCursor, args: &mut A) -> usize
    where
        C: Codec,
        T: DeserializeForSimple<C, A>;
}

impl SimpleSummary for () {
    fn from_pos(_pos: usize) -> Self {
        ()
    }

    fn detect_end<C, A, T>(self, mut cursor: ByteReadCursor, args: &mut A) -> usize
    where
        C: Codec,
        T: DeserializeForSimple<C, A>,
    {
        let _ = T::decode_simple(&mut cursor, args);
        cursor.pos()
    }
}

impl SimpleSummary for usize {
    fn from_pos(pos: usize) -> Self {
        pos
    }

    fn detect_end<C, A, T>(self, _cursor: ByteReadCursor, _args: &mut A) -> usize
    where
        C: Codec,
        T: DeserializeForSimple<C, A>,
    {
        self
    }
}

impl<C, A, T> DeserializeFor<C, A> for T
where
    C: Codec,
    T: DeserializeForSimple<C, A>,
    T::Summary: SimpleSummary,
{
    fn summarize(cursor: &mut ByteReadCursor, args: &mut A) -> anyhow::Result<Self::Summary> {
        Self::decode_simple(cursor, args)?;
        Ok(SimpleSummary::from_pos(cursor.pos()))
    }

    fn view_<'a>(
        _no_external_call: NoExternalCall,
        _summary: &'a Self::Summary,
        cursor: ByteReadCursor<'a>,
        args: &mut A,
    ) -> Self::View<'a> {
        Self::decode_simple(&mut cursor.clone(), args).unwrap()
    }

    fn end(summary: &Self::Summary, cursor: ByteReadCursor, args: &mut A) -> usize {
        summary.detect_end::<C, A, T>(cursor, args)
    }
}

// === Serialize === //

pub trait SerializeInto<C: Codec, T, A> {
    fn serialize(&self, stream: &mut impl Write, args: &mut A) -> anyhow::Result<()>;

    fn length(&self, args: &mut A) -> anyhow::Result<usize> {
        let mut counter = WriteByteCounter::default();
        self.serialize(&mut counter, args)?;
        Ok(counter.0)
    }
}

// === Struct === //

pub mod codec_struct_internals {
    pub use {
        super::{Deserialize, DeserializeFor, SerializeInto},
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
				// Safety invariant: the cursor and all the summary's elements have the same backing buffer.
                summary: &'a Summary,
                cursor: $crate::util::byte_codec::codec_struct_internals::ByteReadCursor<'a>,
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
                    cursor: &mut $crate::util::byte_codec::codec_struct_internals::ByteReadCursor,
                    _args: &mut (),
                ) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<Self::Summary> {
					let _ = &cursor;

                    $crate::util::byte_codec::codec_struct_internals::Ok(Summary {$(
						#[allow(unused_parens)]
						$field_name: <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor::<$codec, ($($config_ty)?)>>::summarize(
                            cursor,
                            &mut {$($config)?},
                        )?,
					)*})
                }

                fn view_<'a>(
					_no_external_call: NoExternalCall,
                    summary: &'a Self::Summary,
                    cursor: $crate::util::byte_codec::codec_struct_internals::ByteReadCursor<'a>,
                    _args: &mut (),
                ) -> Self::View<'a> {
					// Safety: the caller guarantees that the summary was generated using this cursor's
					// backing buffer. Because every sub-summary created by `summarize` was also
					// generated by a cursor derived from this backing buffer, they too have the
					// appropriate backing buffer, satisfying this structure's safety invariants.
                    Self::View { summary, cursor }
                }

                fn end(
                    summary: &Self::Summary,
                    cursor: $crate::util::byte_codec::codec_struct_internals::ByteReadCursor,
                    _args: &mut (),
                ) -> $crate::util::byte_codec::codec_struct_internals::usize {
					let _ = summary;

					let offset = cursor.pos();
                    $(
						#[allow(unused_parens)]
                        let offset = <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::end(
                            &summary.$field_name,
                            cursor.with_offset(offset),
                            &mut {$($config)?},
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
					let offset = self.cursor.pos();
					let _ = &offset;

                    $(
						#[allow(unused_parens)]
                        let offset = <$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::end(
                            &self.summary.$field_name,
                            self.cursor.with_offset(offset),
                            &mut {$($config)?},
                        );
						let $field_name = offset;
                    )*

					OffsetsTmp { $($field_name,)* }
				}
			}

            impl<'a> View<'a> {$(
                pub fn $field_name(&self) -> <$field_ty as $crate::util::byte_codec::codec_struct_internals::Deserialize<$codec>>::View<'a> {
					let offset = self.offsets().$field_name;
					let config = {$($config)?};

					unsafe {
						// Safety: by invariant, we know the summary, its sub-element summaries, and
						// its cursor were all derived from the same backing buffer, making this call
						// valid.
						<$field_ty as $crate::util::byte_codec::codec_struct_internals::DeserializeFor<$codec, ($($config_ty)?)>>::view(
							&self.summary.$field_name,
							self.cursor.with_offset(offset),
							&mut config,
						)
					}
                }
            )*}

            // View reification
            impl $crate::util::byte_codec::codec_struct_internals::From<View<'_>> for $struct_name {
                fn from(view: View<'_>) -> Self {
					let _ = &view;

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
            #[allow(non_camel_case_types, unused_parens)]
            impl<$($field_name,)*> $crate::util::byte_codec::codec_struct_internals::SerializeInto<$codec, $struct_name, ()> for Builder<$($field_name,)*>
            where
                $($field_name: $crate::util::byte_codec::codec_struct_internals::SerializeInto<$codec, $field_ty, ($($config_ty)?)>,)*
            {
                fn serialize(
					&self,
					stream: &mut impl $crate::util::byte_codec::codec_struct_internals::Write,
					_args: &mut (),
				) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<()> {
					let _ = &stream;

					$(
						$crate::util::byte_codec::codec_struct_internals::SerializeInto::<$codec, $field_ty, ($($config_ty)?)>::serialize(
							&self.$field_name,
							stream,
							&mut {$($config)?},
						)?;
					)*

					$crate::util::byte_codec::codec_struct_internals::Ok(())
				}

                fn length(&self, _args: &mut ()) -> $crate::util::byte_codec::codec_struct_internals::anyhow::Result<$crate::util::byte_codec::codec_struct_internals::usize> {
                    $crate::util::byte_codec::codec_struct_internals::Ok(
						0 $(+ $crate::util::byte_codec::codec_struct_internals::SerializeInto::<$codec, $field_ty, ($($config_ty)?)>::length(
							&self.$field_name,
							&mut {$($config)?},
						)?)*
					)
                }
            }

			// TODO: Ensure that reified form can also serialize.
        }

        #[allow(unused_imports)]
        $struct_vis use $mod_name::$struct_name;
    )*};
}

pub(crate) use codec_struct;
