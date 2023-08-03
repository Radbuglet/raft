use std::fmt;

use super::core::Codec;

// === Codec === //

pub trait SeqDecodeCodec: Codec {
    type Reader<'a>: ReadCursor<Pos = Self::ReaderPos>;
    type ReaderPos: ReadPos;

    fn covariant_cast<'a: 'b, 'b>(reader: Self::Reader<'a>) -> Self::Reader<'b>;
}

pub trait ReadCursor: Sized + Clone {
    type Pos: ReadPos;

    fn pos(&self) -> Self::Pos;

    fn set_pos(&mut self, pos: Self::Pos);
}

pub trait ReadPos: Sized + 'static + Copy + Eq {}

impl<T: 'static + Copy + Eq> ReadPos for T {}

// === Deserialize === //

pub trait DeserializeSeq<C: SeqDecodeCodec>: Sized + 'static {
    /// A summary of the deserialized contents. This includes enough information to:
    ///
    /// 1. Determine the position of sub-fields quickly given the starting position of the object.
    /// 2. Decode its contents quickly given the backing stream.
    /// 3. Determine the starting position of the next object quickly given the starting position of
    ///    this object.
    ///
    type Summary: 'static + fmt::Debug + Clone;

    /// A user-friendly view into the contents of this object given a bound backing buffer.
    ///
    /// This view should be lazily evaluated and must provide a mechanism for reifying the view into
    /// its regular type.
    type View<'a>: fmt::Debug + Clone;

    /// Reifies a view object into an owned version of that object.
    fn reify_view(view: &Self::View<'_>) -> Self;
}

pub trait DeserializeSeqFor<C: SeqDecodeCodec, A>: DeserializeSeq<C> {
    /// Validates and summarizes an input stream, leaving the `cursor` head at the start of
    /// the next item.
    fn summarize(cursor: &mut C::Reader<'_>, args: &mut A) -> anyhow::Result<Self::Summary>;

    /// Produces a user-friendly view of a summary. It should always be valid to construct this view
    /// since summarization should have already performed all the necessary validation.
    ///
    /// ## Safety
    ///
    /// Callers assert that:
    ///
    /// - The `summary` was generated by this `impl` block's [`summarize`](DeserializeSeqFor::summarize)
    ///   method and not some other `impl` with the same `summary` type.
    ///
    /// - The `summary` was generated using the same backing buffer as is being provided in the
    ///   `cursor`.
    ///
    /// Note that there is *no guarantee* that:
    ///
    /// - The `args` provided are the same as the arguments provided to `summarize`.
    /// - The exact type of the arguments is identical since one may have blanket `impl`'d this trait
    ///   for several argument types.
    /// - The cursor is in the same place as it was when the summary was generated.
    ///
    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: C::Reader<'a>,
        args: &mut A,
    ) -> Self::View<'a>;

    /// Skips a cursor starting at the beginning of the stream to the start of the next element.
    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut C::Reader<'_>),
        cursor: &mut C::Reader<'_>,
        args: &mut A,
    );

    /// Summarizes and views the deserialized contents of the reader in a single step.
    fn summarize_and_view<F, R>(cursor: &mut C::Reader<'_>, args: &mut A, f: F) -> anyhow::Result<R>
    where
        F: FnOnce(&mut C::Reader<'_>, Self::View<'_>) -> anyhow::Result<R>,
    {
        let fork = cursor.clone();
        let summary = Self::summarize(cursor, args)?;

        let view = unsafe {
            // Safety: we just generated this summary with the appropriate cursor.
            Self::view(&summary, C::covariant_cast(fork), args)
        };

        f(cursor, view)
    }

    /// Decodes the reified version of this value in a single step.
    fn decode<'a>(cursor: &'a mut C::Reader<'_>, args: &mut A) -> anyhow::Result<Self> {
        Self::summarize_and_view(cursor, args, |_, view| Ok(Self::reify_view(&view)))
    }
}

// === DeserializeSeqForSimple === //

pub trait DeserializeSeqForSimple<C: SeqDecodeCodec, A>: DeserializeSeq<C> {
    // N.B. The Rust trait checker differentiates between methods which are made early-bound and
    // methods which are made late-bound when converted into a closure and currently refuses to unify
    // the former into the latter during trait member compatibility checks.
    //
    // An early-bound closure is a closure whose lifetime parameter has a higher-ranked trait bound
    // (HRTB) (e.g. `impl for<'a> Fn(&'a [u32]) -> &'a u32`) versus a closure whose lifetime parameters
    // have all been bound to specific values (e.g. `impl Fn(&'1 [u32]) -> &'1 u32`). The former is
    // more general than the latter (early-bound types can be coerced into a specific late-bound
    // instantiation of their lifetime parameters) but it is not always possible to construct an
    // early-bound closure for every signature. For example, writing `impl for<'a> Fn(u32) -> &'a u32`
    // is illegal since it would require a corresponding `impl` of the form:
    //
    // ```
    // impl<'a> FnMut<u32> for MyClosure {
    //     type Output = &'a u32;
    // }
    // ```
    //
    // ...which is disallowed by virtue of `'a` not being bound by any generic parameter.
    //
    // Anyways, for GATs, we don't know whether `C::Reader<'a>` will actually depend on `'a`. So, a
    // function closure `impl for<'a> Fn(C::Reader<'a>) -> ViewType<'a>` may not know whether it can be
    // early or late bound until it knows the specific implementation of `C`. Therefore, this method
    // is declared as late-bound.
    //
    // Unfortunately, if a user types out this same method declaration in their `impl` block but
    // substitutes `C::Reader<'a>` with an actual value which depends on `'a`, the Rust trait checker
    // will see a declaration like `impl for<'a> Fn(SomeReader<'a>) -> ViewType<'a>` and reason that,
    // because `SomeReader<'a>` depends on `'a`, that trait's implementation of `decode_simple` is
    // actually early-bound instead of late-bound.
    //
    // We have two ways of remedying this:
    //
    // 1. We can define the method such that it is always possible for the method to be early-bound.
    //    We could do this, for example, by defining a dummy parameter like `_early_bind: &'a ()`,
    //    which would ensure that, regardless of the choice of `C::Reader<'a>`, the closure could
    //    always be early-bound.
    //
    // 2. We could define the method such that it becomes more clear to the user that they have to
    //    make the method late-bound. For example, we could use the constraint `'a: 'a`, which forces
    //    any method with that constraint to become late-bound, to give the user a clear way to make
    //    their method late-bound.
    //
    // There are benefits and drawbacks to each approach. With approach 1, we let the method be in
    // its most general form possible but clutter the signature. Which approach 2, the syntax is a
    // bit nicer but the method is now never allowed to be early-bound and the `'a: 'a` trick may
    // stop working in the future as the constraint solver becomes more intelligent.
    //
    // We chose approach 1 since it offers more flexibility and is the most forwards-compatible with
    // future Rust releases which may either break the `'a: 'a` trick. Additionally, approach 1
    // provides continued benefit once the Rust trait compatibility checker considers early-bound
    // methods to be more general than late-bound since making the method signature early-bound will
    // always be necessary anyways in case we want to write generic code w.r.t `C`.
    //
    // As one final note unrelated to this explanation, one may be wondering why the Rust trait
    // checker would even need to care about whether a method is either early-bound or late-bound.
    // This differentiation is necessary to allow generic code such as:
    //
    // ```rust
    // fn foo<C, A, T>()
    // where
    //     C: SeqDecodeCodec,
    //     T: DeserializeSeqForSimple<C, A>,
    // {
    //     let closure = T::decode_simple;
    //     needs_higher_kinded_closure(closure);
    // }
    // ```
    //
    // to be sound for all implementations of the trait. If the generic code assumed the closure
    // could be early-bound but an implementation was defined in such a way that it could only be
    // late-bound, we'd have a problem. Huzzah for unexpected cross-system interactions!
    //
    // Thanks so much to Yandros (https://github.com/danielhenrymantilla) for their explanation of
    // this issue!
    fn decode_simple<'a>(
        _binding: [&'a (); 0],
        cursor: &mut C::Reader<'a>,
        args: &mut A,
    ) -> anyhow::Result<Self::View<'a>>;
}

pub trait SimpleSummary<C: SeqDecodeCodec>: ReadPos {
    fn from_pos(pos: C::ReaderPos) -> Self;

    fn skip_to_end<A, T>(
        self,
        skip_to_start: impl Fn(&mut C::Reader<'_>),
        cursor: &mut C::Reader<'_>,
        args: &mut A,
    ) where
        T: DeserializeSeqForSimple<C, A>;
}

impl<C: SeqDecodeCodec> SimpleSummary<C> for () {
    fn from_pos(_pos: C::ReaderPos) -> Self {
        ()
    }

    fn skip_to_end<A, T>(
        self,
        skip_to_start: impl Fn(&mut C::Reader<'_>),
        cursor: &mut C::Reader<'_>,
        args: &mut A,
    ) where
        T: DeserializeSeqForSimple<C, A>,
    {
        skip_to_start(cursor);
        let _ = T::decode_simple([], cursor, args);
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub struct EndPosSummary<P>(pub P);

impl<C: SeqDecodeCodec> SimpleSummary<C> for EndPosSummary<C::ReaderPos> {
    fn from_pos(pos: C::ReaderPos) -> Self {
        Self(pos)
    }

    fn skip_to_end<A, T>(
        self,
        _skip_to_start: impl Fn(&mut C::Reader<'_>),
        cursor: &mut C::Reader<'_>,
        _args: &mut A,
    ) where
        T: DeserializeSeqForSimple<C, A>,
    {
        cursor.set_pos(self.0);
    }
}

impl<C, A, T> DeserializeSeqFor<C, A> for T
where
    C: SeqDecodeCodec,
    T: DeserializeSeqForSimple<C, A>,
    T::Summary: SimpleSummary<C>,
{
    fn summarize(cursor: &mut C::Reader<'_>, args: &mut A) -> anyhow::Result<Self::Summary> {
        Self::decode_simple([], cursor, args)?;
        Ok(SimpleSummary::from_pos(cursor.pos()))
    }

    unsafe fn view<'a>(
        _summary: &'a Self::Summary,
        cursor: C::Reader<'a>,
        args: &mut A,
    ) -> Self::View<'a> {
        Self::decode_simple([], &mut cursor.clone(), args).unwrap()
    }

    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut C::Reader<'_>),
        cursor: &mut C::Reader<'_>,
        args: &mut A,
    ) {
        summary.skip_to_end::<A, T>(skip_to_start, cursor, args)
    }
}

// === Derivation Macro === //

#[doc(hidden)]
pub mod derive_seq_decode_internals {
    pub use {
        super::{DeserializeSeq, DeserializeSeqFor, ReadCursor, SeqDecodeCodec},
        anyhow,
        std::{clone::Clone, convert::identity, fmt, result::Result::Ok, stringify},
    };
}

macro_rules! derive_seq_decode {
    (
        $(#[$attr:meta])*
        $struct_vis:vis struct $struct_name:ident($codec:ty) {
            $(
				$(#[$field_attr:meta])*
				$field_name:ident: $field_ty:ty $(=> $config_ty:ty : $config:expr)?
			),*
            $(,)?
        }
    ) => {
		// Structure definitions
		#[derive(Debug, Clone)]
		pub struct Summary {
			$($field_name: <$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeq<$codec>>::Summary,)*
		}

		#[derive(Clone)]
		pub struct View<'a> {
			// Safety invariant: the cursor and all the summary's elements have the same backing buffer.
			summary: &'a Summary,
			cursor: <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'a>,
		}

		// Deserialization
		impl $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeq<$codec> for $struct_name {
			type Summary = Summary;
			type View<'a> = View<'a>;

			fn reify_view(view: &Self::View<'_>) -> Self {
				let _ = view;

				Self {
					$($field_name: $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeq::<$codec>::reify_view(&view.$field_name()),)*
				}
			}
		}

		impl $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeqFor<$codec, ()> for $struct_name {
			fn summarize(
				cursor: &mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>,
				_args: &mut (),
			) -> $crate::util::proto::decode_seq::derive_seq_decode_internals::anyhow::Result<Self::Summary> {
				let _ = &cursor;

				$crate::util::proto::decode_seq::derive_seq_decode_internals::Ok(Summary {$(
					#[allow(unused_parens)]
					$field_name: <$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeqFor::<$codec, ($($config_ty)?)>>::summarize(
						cursor,
						&mut {$($config)?},
					)?,
				)*})
			}

			unsafe fn view<'a>(
				summary: &'a Self::Summary,
				cursor: <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'a>,
				_args: &mut (),
			) -> Self::View<'a> {
				// Safety: the caller guarantees that the summary was generated using this cursor's
				// backing buffer. Because every sub-summary created by `summarize` was also
				// generated by a cursor derived from this backing buffer, they too have the
				// appropriate backing buffer, satisfying this structure's safety invariants.
				Self::View { summary, cursor }
			}

			fn skip(
				summary: &Self::Summary,
				skip_to_start: impl Fn(&mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>),
				cursor: &mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>,
				_args: &mut (),
			) {
				let _ = (summary, &cursor);

				$(
					#[allow(unused_parens)]
					let skip_to_start = |cursor: &mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>| {
						<$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeqFor<$codec, ($($config_ty)?)>>::skip(
							&summary.$field_name,
							&skip_to_start,
							cursor,
							&mut {$($config)?},
						);
					};
				)*

				skip_to_start(cursor);
			}
		}

		// View accessors
		mod skippers {
			use super::*;

			macro_rules! prev_func_call {
				($cursor:expr, $summary:expr) => {
					let _ = $cursor;
				};
			}

			$(
				pub fn $field_name(
					cursor: &mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>,
					summary: &Summary,
				) {
					<$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeqFor<$codec, ($($config_ty)?)>>::skip(
						&summary.$field_name,
						|cursor: &mut <$codec as $crate::util::proto::decode_seq::derive_seq_decode_internals::SeqDecodeCodec>::Reader<'_>| {
							prev_func_call!(cursor, summary);
						},
						cursor,
						&mut {$($config)?},
					);
				}

				#[allow(unused_macros)]
				macro_rules! prev_func_call {
					($cursor:expr, $summary:expr) => {
						$field_name($cursor, $summary);
					};
				}
			)*
		}

		impl<'a> View<'a> {
			$(
				pub fn $field_name(&self) -> <$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeq<$codec>>::View<'a> {
					// Align the cursor to the appropriate location.
					let mut cursor = $crate::util::proto::decode_seq::derive_seq_decode_internals::Clone::clone(&self.cursor);
					skippers::$field_name(&mut cursor, &self.summary);

					// Compute the config outside of the `unsafe` block.
					let mut config = {$($config)?};

					unsafe {
						// Safety: by invariant, we know the summary, its sub-element summaries, and
						// its cursor were all derived from the same backing buffer, making this call
						// valid. We know the config type is constant because `$config_ty` fixes it
						// to a value.
						#[allow(unused_parens)]
						<$field_ty as $crate::util::proto::decode_seq::derive_seq_decode_internals::DeserializeSeqFor<$codec, ($($config_ty)?)>>::view(
							&self.summary.$field_name,
							cursor,
							&mut config,
						)
					}
				}
			)*
		}

		// View formatting
		impl $crate::util::proto::decode_seq::derive_seq_decode_internals::fmt::Debug for View<'_> {
			fn fmt(&self, f: &mut $crate::util::proto::decode_seq::derive_seq_decode_internals::fmt::Formatter<'_>) -> $crate::util::proto::decode_seq::derive_seq_decode_internals::fmt::Result {
				f.debug_struct($crate::util::proto::decode_seq::derive_seq_decode_internals::stringify!($struct_name))
					$(.field(
						$crate::util::proto::decode_seq::derive_seq_decode_internals::stringify!($field_name),
						&self.$field_name(),
					))*
					.finish()
			}
		}
    };
}

pub(super) mod derive_seq_decode_macro {
    pub(crate) use derive_seq_decode;
}
