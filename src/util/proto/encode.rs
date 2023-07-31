use std::{error::Error, ops::AddAssign};

use crate::util::error::NeverError;

use super::core::Codec;

// === Codec Traits === //

pub trait EncodeCodec: Codec {
    type WriteElement<'a>: ?Sized;
    type SizeMetric: SizeMetricFor<Self>;
}

// WriteStream
pub trait WriteStream<E: ?Sized> {
    type PushError: 'static + Error + Send + Sync;

    fn push(&mut self, elem: &E) -> Result<(), Self::PushError>;
}

pub trait WriteStreamFor<C: EncodeCodec>: for<'a> WriteStream<C::WriteElement<'a>> {}

impl<C, T> WriteStreamFor<C> for T
where
    C: EncodeCodec,
    T: for<'a> WriteStream<C::WriteElement<'a>>,
{
}

// SizeMetric
pub trait SizeMetric: 'static + Copy + AddAssign + Default {}

pub trait SizeMetricForElement<E: ?Sized>: SizeMetric {
    fn size_of(elem: &E) -> Self;
}

pub trait SizeMetricFor<C: EncodeCodec>: for<'a> SizeMetricForElement<C::WriteElement<'a>> {}

impl<C, T> SizeMetricFor<C> for T
where
    C: EncodeCodec,
    T: for<'a> SizeMetricForElement<C::WriteElement<'a>>,
{
}

#[derive(Debug, Clone, Default)]
pub struct SizeCountingWriteStream<M: SizeMetric>(pub M);

impl<M, E> WriteStream<E> for SizeCountingWriteStream<M>
where
    M: SizeMetricForElement<E>,
    E: ?Sized,
{
    type PushError = NeverError;

    fn push(&mut self, elem: &E) -> Result<(), Self::PushError> {
        self.0 += M::size_of(elem);
        Ok(())
    }
}

// TODO: Optimize size metric

// === Serialization traits === //

pub trait SerializeInto<C: EncodeCodec, T, A>: Sized {
    fn serialize(&self, stream: &mut impl WriteStreamFor<C>, args: &mut A) -> anyhow::Result<()>;

    fn size(&self, args: &mut A) -> anyhow::Result<C::SizeMetric> {
        let mut counter = SizeCountingWriteStream::default();
        self.serialize(&mut counter, args)?;
        Ok(counter.0)
    }
}

pub trait SerializeFrom<C: EncodeCodec, D, A> {
    fn serialize_from(
        value: &D,
        stream: &mut impl WriteStreamFor<C>,
        args: &mut A,
    ) -> anyhow::Result<()>;
}

impl<C: EncodeCodec, T, D, A> SerializeFrom<C, D, A> for T
where
    C: EncodeCodec,
    D: SerializeInto<C, T, A>,
{
    fn serialize_from(
        value: &D,
        stream: &mut impl WriteStreamFor<C>,
        args: &mut A,
    ) -> anyhow::Result<()> {
        value.serialize(stream, args)
    }
}

// === Derivation Macro === //

#[doc(hidden)]
pub mod derive_encode_internals {
    pub use {
        super::{EncodeCodec, SerializeInto, WriteStream},
        crate::util::proto::core::Codec,
        anyhow,
        std::result::Result::Ok,
    };
}

macro_rules! derive_encode {
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
		#[derive(Debug, Copy, Clone)]
		#[allow(non_camel_case_types)]
		pub struct Builder<$($field_name,)*> {
			$(pub $field_name: $field_name,)*
		}

		#[allow(non_camel_case_types, unused_parens)]
		impl<$($field_name,)*> $crate::util::proto::encode::derive_encode_internals::SerializeInto<$codec, $struct_name, ()> for Builder<$($field_name,)*>
		where
			$($field_name: $crate::util::proto::encode::derive_encode_internals::SerializeInto<$codec, $field_ty, ($($config_ty)?)>,)*
		{
			fn serialize(
				&self,
				stream: &mut impl for<'a>
					$crate::util::proto::encode::derive_encode_internals::WriteStream<
						<$codec as $crate::util::proto::encode::derive_encode_internals::Codec>::WriteElement<'a>>,
				_args: &mut (),
			) -> $crate::util::proto::encode::derive_encode_internals::anyhow::Result<()> {
				let _ = &stream;

				$(
					$crate::util::proto::encode::derive_encode_internals::SerializeInto::<$codec, $field_ty, ($($config_ty)?)>::serialize(
						&self.$field_name,
						stream,
						&mut {$($config)?},
					)?;
				)*

				$crate::util::proto::encode::derive_encode_internals::Ok(())
			}
		}

		// TODO: Ensure that reified form can also serialize.
	};
}

pub(super) mod derive_encode_macro {
    pub(crate) use derive_encode;
}
