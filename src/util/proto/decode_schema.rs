use std::fmt;

use super::core::Codec;

// === Codec Traits === //

pub trait SchemaDecodeCodec: Codec {
    type Document: SchemaDocument<AnyRef = Self::AnyRef, ObjectRef = Self::ObjectRef>;
    type AnyRef: 'static + fmt::Debug + Clone;
    type ObjectRef: 'static + fmt::Debug + Clone;
}

pub trait SchemaDocument: Sized + 'static {
    type AnyRef: 'static + fmt::Debug + Clone;
    type ObjectRef: 'static + fmt::Debug + Clone;

    fn root(&self) -> Self::AnyRef;

    fn any_ref_as_object(&self, any_ref: Self::AnyRef) -> Result<Self::ObjectRef, Self::AnyRef>;

    fn object_entry(&self, obj: &Self::ObjectRef, key: &str) -> Option<Self::AnyRef>;
}

// === Deserialize Traits === //

pub trait DeserializeSchema<C: SchemaDecodeCodec, A>: Sized + 'static {
    type Shortcut: 'static + fmt::Debug + Clone;
    type View<'a>: SchemaView<
        C,
        A,
        Validated = Self::ValidatedView<'a>,
        Shortcut = Self::Shortcut,
        Reified = Self,
    >;

    type ValidatedView<'a>: ValidatedSchemaView<
        C,
        A,
        RawView = Self::View<'a>,
        Shortcut = Self::Shortcut,
        Reified = Self,
    >;

    fn make_shortcut(
        document: &C::Document,
        object: Option<C::AnyRef>,
    ) -> anyhow::Result<Self::Shortcut>;

    fn view_shortcut<'a>(
        document: &'a C::Document,
        shortcut: Self::Shortcut,
        args: A,
    ) -> Self::View<'a>;

    fn view_object<'a>(
        document: &'a C::Document,
        object: Option<C::AnyRef>,
        args: A,
    ) -> anyhow::Result<Self::View<'a>> {
        Self::make_shortcut(document, object)
            .map(|shortcut| Self::view_shortcut(document, shortcut, args))
    }
}

pub trait SchemaView<C: SchemaDecodeCodec, A>: fmt::Debug + Clone {
    type Reified: DeserializeSchema<C, A, Shortcut = Self::Shortcut>;
    type Shortcut: 'static + fmt::Debug + Clone;
    type Validated: ValidatedSchemaView<
        C,
        A,
        Reified = Self::Reified,
        Shortcut = Self::Shortcut,
        RawView = Self,
    >;

    fn assume_valid(self) -> Self::Validated;

    fn validate_deep(&self) -> anyhow::Result<()>;

    fn as_shortcut(&self) -> Self::Shortcut;

    fn reify(&self) -> anyhow::Result<Self::Reified>;
}

pub trait ValidatedSchemaView<C: SchemaDecodeCodec, A>: fmt::Debug + Clone {
    type Reified: DeserializeSchema<C, A, Shortcut = Self::Shortcut>;
    type Shortcut: 'static + fmt::Debug + Clone;
    type RawView: SchemaView<
        C,
        A,
        Reified = Self::Reified,
        Shortcut = Self::Shortcut,
        Validated = Self,
    >;

    fn unwrap_validation(self) -> Self::RawView;

    fn as_shortcut_validated(&self) -> Self::Shortcut;

    fn reify_validated(&self) -> Self::Reified;
}

// === Derivation Macro === //

#[doc(hidden)]
pub mod derive_schema_decode_internals {
    pub use {
        super::{
            DeserializeSchema, SchemaDecodeCodec, SchemaDocument, SchemaView, ValidatedSchemaView,
        },
        anyhow,
        std::{concat, fmt, option::Option, stringify},
    };
}

macro_rules! derive_schema_decode {
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
		#[derive(Clone)]
		pub struct View<'a> {
			document: &'a <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::Document,
			shortcut: <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::ObjectRef,
		}

        impl $crate::util::proto::decode_schema::derive_schema_decode_internals::DeserializeSchema<$codec, ()> for $struct_name {
			type Shortcut = <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::ObjectRef;
			type View<'a> = View<'a>;
			type ValidatedView<'a> = ValidatedView<'a>;

			fn make_shortcut(
				document: &<$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::Document,
				object: $crate::util::proto::decode_schema::derive_schema_decode_internals::Option<
					<$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::AnyRef
				>,
			) -> $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result<Self::Shortcut> {
				let $crate::util::proto::decode_schema::derive_schema_decode_internals::Option::Some(object) = object else {
					$crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::bail!("Expected an object, got an unassigned key.");
				};

				$crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDocument::any_ref_as_object(document, object)
					.map_err(|value| $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::anyhow!("Expected an object, got {value:?}"))
			}

			fn view_shortcut<'a>(
				document: &'a <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::Document,
				shortcut: Self::Shortcut,
				_args: (),
			) -> Self::View<'a> {
				Self::View {
					document,
					shortcut,
				}
			}
		}

		#[allow(unused_parens)]
		impl<'a> $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView<$codec, ()> for View<'a> {
			type Reified = $struct_name;
			type Shortcut = <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::ObjectRef;
			type Validated = ValidatedView<'a>;

			fn assume_valid(self) -> Self::Validated {
				ValidatedView(self)
			}

			fn validate_deep(&self) -> $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result<()> {
				$($crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView::<$codec, ($($config_ty)?)>::validate_deep(
					&self.$field_name()?,
				)?;)*
				$crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result::Ok(())
			}

			fn as_shortcut(&self) -> Self::Shortcut {
				self.shortcut
			}

			fn reify(&self) -> $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result<Self::Reified> {
				$crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result::Ok(Self::Reified {
					$(
						$field_name: $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView::<$codec, ($($config_ty)?)>::reify(
							&self.$field_name()?,
						)?,
					)*
				})
			}
		}

		#[allow(unused_parens)]
		impl<'a> View<'a> {
			$(
				pub fn $field_name(&self) -> $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Result<
					<$field_ty as $crate::util::proto::decode_schema::derive_schema_decode_internals::DeserializeSchema<$codec, ($($config_ty)?)>>::View<'a>
				> {
					let res = <$field_ty as $crate::util::proto::decode_schema::derive_schema_decode_internals::DeserializeSchema<$codec, ($($config_ty)?)>>::view_object(
						self.document,
						$crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDocument::object_entry(
							self.document,
							&self.shortcut,
							$crate::util::proto::decode_schema::derive_schema_decode_internals::stringify!($field_name)
						),
						{$($config)?},
					);

					let res = $crate::util::proto::decode_schema::derive_schema_decode_internals::anyhow::Context::context(
						res,
						$crate::util::proto::decode_schema::derive_schema_decode_internals::concat!(
							"Failed to access field `",
							$crate::util::proto::decode_schema::derive_schema_decode_internals::stringify!($field_name),
							"` of ",
							$crate::util::proto::decode_schema::derive_schema_decode_internals::stringify!($struct_name),
							".",
						),
					);

					res
				}
			)*
		}

		impl $crate::util::proto::decode_schema::derive_schema_decode_internals::fmt::Debug for View<'_> {
			fn fmt(&self, f: &mut $crate::util::proto::decode_schema::derive_schema_decode_internals::fmt::Formatter<'_>) -> $crate::util::proto::decode_schema::derive_schema_decode_internals::fmt::Result {
				f.debug_struct($crate::util::proto::decode_schema::derive_schema_decode_internals::stringify!($struct_name))
					$(.field($crate::util::proto::decode_schema::derive_schema_decode_internals::stringify!($field_name), &self.$field_name()))*
					.finish()
			}
		}

		#[derive(Debug, Clone)]
		pub struct ValidatedView<'a>(pub View<'a>);

		impl<'a> $crate::util::proto::decode_schema::derive_schema_decode_internals::ValidatedSchemaView<$codec, ()> for ValidatedView<'a> {
			type Reified = $struct_name;
			type Shortcut = <$codec as $crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaDecodeCodec>::ObjectRef;
			type RawView = View<'a>;

			fn unwrap_validation(self) -> Self::RawView {
				self.0
			}

			fn as_shortcut_validated(&self) -> Self::Shortcut {
				$crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView::<$codec, ()>::as_shortcut(&self.0)
			}

			fn reify_validated(&self) -> Self::Reified {
				$crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView::<$codec, ()>::reify(&self.0).unwrap()
			}
		}

		#[allow(unused_parens)]
		impl<'a> ValidatedView<'a> {
			$(
				pub fn $field_name(&self) -> <$field_ty as $crate::util::proto::decode_schema::derive_schema_decode_internals::DeserializeSchema<$codec, ($($config_ty)?)>>::ValidatedView<'a> {
					$crate::util::proto::decode_schema::derive_schema_decode_internals::SchemaView::<$codec, ($($config_ty)?)>::assume_valid(
						self.0.$field_name().unwrap()
					)
				}
			)*
		}
    };
}

pub(super) mod derive_schema_decode_macro {
    pub(crate) use derive_schema_decode;
}
