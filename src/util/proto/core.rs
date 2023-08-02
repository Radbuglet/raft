// === Codec === //

pub trait Codec: Sized + 'static {}

// === Macros === //

#[doc(hidden)]
pub mod codec_struct_internals {
    pub(crate) use super::super::{
        decode_schema::derive_schema_decode_macro::derive_schema_decode,
        decode_seq::derive_seq_decode_macro::derive_seq_decode,
        encode::derive_encode_macro::derive_encode,
    };
}

macro_rules! seq_codec_struct {
    ($(
		$(#[$attr:meta])*
        $struct_vis:vis struct $mod_name:ident::$struct_name:ident($codec:ty) {
            $(
				$(#[$field_attr:meta])*
				$field_name:ident: $field_ty:ty $(=> $config_ty:ty : $config:expr)?
			),*
            $(,)?
        }
	)*) => {$(
		$struct_vis mod $mod_name {
			use super::*;

			$(#[$attr])*
			pub struct $struct_name {
				$(pub $field_name: $field_ty,)*
			}

			$crate::util::proto::core::codec_struct_internals::derive_encode! {
				$(#[$attr])*
				$struct_vis struct $struct_name($codec) {
					$(
						$(#[$field_attr])*
						$field_name: $field_ty $(=> $config_ty : $config)?
					),*
				}
			}

			$crate::util::proto::core::codec_struct_internals::derive_seq_decode! {
				$(#[$attr])*
				$struct_vis struct $struct_name($codec) {
					$(
						$(#[$field_attr])*
						$field_name: $field_ty $(=> $config_ty : $config)?
					),*
				}
			}
		}
	)*};
}

pub(crate) use seq_codec_struct;

macro_rules! schema_codec_struct {
    ($(
		$(#[$attr:meta])*
        $struct_vis:vis struct $mod_name:ident::$struct_name:ident($codec:ty) {
            $(
				$(#[$field_attr:meta])*
				$field_name:ident: $field_ty:ty $(=> $config_ty:ty : $config:expr)?
			),*
            $(,)?
        }
	)*) => {$(
		$struct_vis mod $mod_name {
			use super::*;

			$(#[$attr])*
			pub struct $struct_name {
				$(pub $field_name: $field_ty,)*
			}

			// TODO: Re-enable
			// $crate::util::proto::core::codec_struct_internals::derive_encode! {
			// 	$(#[$attr])*
			// 	$struct_vis struct $struct_name($codec) {
			// 		$(
			// 			$(#[$field_attr])*
			// 			$field_name: $field_ty $(=> $config_ty : $config)?
			// 		),*
			// 	}
			// }

			$crate::util::proto::core::codec_struct_internals::derive_schema_decode! {
				$(#[$attr])*
				$struct_vis struct $struct_name($codec) {
					$(
						$(#[$field_attr])*
						$field_name: $field_ty $(=> $config_ty : $config)?
					),*
				}
			}
		}
	)*};
}

pub(crate) use schema_codec_struct;
