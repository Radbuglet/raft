use bytes::{BufMut, Bytes};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{any::type_name, mem, ops::Deref};

use smallvec::SmallVec;

use crate::util::{
    bits::{i32_from_u32_2c, i32_to_u32_2c, StaticBitSet},
    byte_cursor::{ByteReadCursor, Snip},
    write::WriteByteCounter,
};

const TOO_BIG_ERR: &str = "byte array is too big to send over the network";

// === Traits === //

pub type StreamingDecodeResult<T> = anyhow::Result<Option<T>>;

pub trait StreamingCodec: SizedCodec<()> {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self>;

    fn encode_streaming(&self, cursor: &mut impl BufMut);
}

pub trait SizedCodec<A>: Codec<A> {
    fn size(&self, args: A) -> usize;
}

pub trait Codec<A>: Sized {
    fn decode(args: A, src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self>;

    fn encode(&self, args: A, cursor: &mut impl BufMut);

    fn decode_bytes(args: A, bytes: &Bytes) -> anyhow::Result<Self> {
        Self::decode(args, bytes, &mut ByteReadCursor::new(bytes))
    }
}

pub fn size_of_tiny<const MAX_SIZE: usize>(body: &impl StreamingCodec) -> usize {
    let mut container = &mut [0u8; MAX_SIZE][..];

    // `BufMut` is `&mut &mut [u8]`.
    body.encode((), &mut container);

    // This works because `&mut [u8]`'s impl of `BufMut` advances the buffer by replacing the
    // slice reference with a shortened one.
    MAX_SIZE - container.len()
}

impl<T: StreamingCodec> Codec<()> for T {
    fn decode(_args: (), _snip: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        match Self::decode_streaming(cursor)? {
            Some(value) => Ok(value),
            None => anyhow::bail!(
                "incomplete primitive of type {} (location: {})",
                type_name::<Self>(),
                cursor.format_location(),
            ),
        }
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        self.encode_streaming(cursor);
    }
}

// === Macros === //

#[doc(hidden)]
pub mod codec_struct_internals {
    pub use {
        super::{Codec, SizedCodec},
        crate::util::byte_cursor::{ByteReadCursor, Snip},
        anyhow::Result,
        bytes::BufMut,
        log::trace,
        std::{any::type_name, result::Result::Ok, stringify},
    };
}

macro_rules! codec_struct {
    ($(
		$(#[$attr:meta])*
		$struct_vis:vis struct $struct_name:ident {
			$($field_vis:vis $field_name:ident: $field_ty:ty $(=> $config:expr)?),*
			$(,)?
		}
	)*) => {$(
		$(#[$attr])*
		$struct_vis struct $struct_name {
			$($field_vis $field_name: $field_ty,)*
		}

        impl $crate::net::primitives::codec_struct_internals::Codec<()> for $struct_name {
			#[allow(unused_variables)]
            fn decode(
				_args: (),
                src: &impl $crate::net::primitives::codec_struct_internals::Snip,
                cursor: &mut $crate::net::primitives::codec_struct_internals::ByteReadCursor,
            ) -> $crate::net::primitives::codec_struct_internals::Result<Self> {
				log::trace!(
					"Decoding {}...",
					$crate::net::primitives::codec_struct_internals::type_name::<Self>(),
				);
				$(
					let start_offset = cursor.read_count();
					let $field_name = $crate::net::primitives::codec_struct_internals::Codec::decode({ $($config)? }, src, cursor)?;
					$crate::net::primitives::codec_struct_internals::trace!(
						"\tDecoded {}: {:?} (ending offset: {}..{})",
						$crate::net::primitives::codec_struct_internals::stringify!($field_name),
						$field_name,
						start_offset,
						cursor.read_count(),
					);
				)*
				$crate::net::primitives::codec_struct_internals::Ok(Self { $($field_name,)* })
            }

			#[allow(unused_variables)]
			fn encode(
                &self,
				_args: (),
                cursor: &mut impl $crate::net::primitives::codec_struct_internals::BufMut,
            ) {
				$($crate::net::primitives::codec_struct_internals::Codec::encode(
					&self.$field_name,
					{ $($config)? },
					cursor,
				);)*
            }
        }

		impl $crate::net::primitives::codec_struct_internals::SizedCodec<()> for $struct_name {
            fn size(&self, _args: ()) -> usize {
				$($crate::net::primitives::codec_struct_internals::SizedCodec::size(&self.$field_name, { $($config)? }) + )* 0
			}
        }
    )*};
}

pub(crate) use codec_struct;

// === Streaming Primitives === //

impl StreamingCodec for bool {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
        let bytes = u8::decode_streaming(cursor)?;

        match bytes {
            Some(0) => Ok(Some(false)),
            Some(1) => Ok(Some(true)),
            Some(got) => anyhow::bail!(
                "invalid variant for boolean; got: {got} (location: {})",
                cursor.format_location(),
            ),
            None => Ok(None),
        }
    }

    fn encode_streaming(&self, cursor: &mut impl BufMut) {
        cursor.put_u8(match *self {
            true => 1,
            false => 0,
        });
    }
}

impl SizedCodec<()> for bool {
    fn size(&self, _args: ()) -> usize {
        1
    }
}

macro_rules! impl_prim {
    ($($ty:ty),*$(,)?) => {$(
		impl StreamingCodec for $ty {
			fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
				if let Some(bytes) = cursor.read_arr() {
					Ok(Some(<$ty>::from_be_bytes(bytes)))
				} else {
					Ok(None)
				}
			}

			fn encode_streaming(&self, cursor: &mut impl BufMut) {
				cursor.put_slice(&self.to_be_bytes());
			}
		}

		impl SizedCodec<()> for $ty {
			fn size(&self, _args: ()) -> usize {
				mem::size_of::<Self>()
			}
		}
	)*};
}

impl_prim!(i8, u8, i16, u16, i32, u32, i64, f32, f64, u128);

#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub i32);

// Adapted from: https://wiki.vg/index.php?title=Protocol&oldid=18305#VarInt_and_VarLong
impl StreamingCodec for VarInt {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
        let mut accum = 0u32;
        let mut shift = 0;

        loop {
            let Some(byte) = cursor.read() else { return Ok(None) };
            accum |= ((byte & !u8::MSB) as u32) << shift;

            if byte & u8::MSB == 0 {
                break;
            }

            shift += 7;

            if shift >= 32 {
                anyhow::bail!(
                    "VarInt is too long to fit an i32 (location: {}).",
                    cursor.format_location(),
                );
            }
        }

        let accum = i32_from_u32_2c(accum);
        Ok(Some(Self(accum)))
    }

    fn encode_streaming(&self, cursor: &mut impl BufMut) {
        let mut accum = i32_to_u32_2c(self.0);

        loop {
            let byte = accum & !u8::MSB as u32;
            accum >>= 7;

            if accum > 0 {
                cursor.put_u8(byte as u8 | u8::MSB);
            } else {
                cursor.put_u8(byte as u8);
                break;
            }
        }
    }
}

impl SizedCodec<()> for VarInt {
    fn size(&self, _args: ()) -> usize {
        size_of_tiny::<5>(self)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct VarUint(pub u32);

impl StreamingCodec for VarUint {
    fn decode_streaming(cursor: &mut ByteReadCursor) -> StreamingDecodeResult<Self> {
        let Some(value) = VarInt::decode_streaming(cursor)? else { return Ok(None) };
        let Ok(value) = u32::try_from(value.0) else {
			anyhow::bail!(
				"Encountered a negative value of {} for what should have been a positive VarInt (location: {}).",
				value.0,
				cursor.format_location(),
			);
		};

        Ok(Some(Self(value)))
    }

    fn encode_streaming(&self, cursor: &mut impl BufMut) {
        VarInt(i32::try_from(self.0).expect("Attempted to encode a VarUint which was too big!"))
            .encode_streaming(cursor)
    }
}

impl SizedCodec<()> for VarUint {
    fn size(&self, _args: ()) -> usize {
        VarInt(i32::try_from(self.0).expect("Attempted to encode a VarUint which was too big!"))
            .size(())
    }
}

// === Codec === //

// Bytes
impl Codec<()> for Bytes {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let bytes = src.freeze_range(cursor.remaining());
        cursor.advance_remaining();
        Ok(bytes)
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        cursor.put_slice(self);
    }
}

impl SizedCodec<()> for Bytes {
    fn size(&self, _args: ()) -> usize {
        self.len()
    }
}

// NetString
#[derive(Debug, Clone, Default)]
pub struct NetString(Bytes);

impl NetString {
    pub fn from_string(str: String) -> Self {
        Self(Bytes::from(str.into_bytes()))
    }

    pub fn from_static_str(str: &'static str) -> Self {
        Self(Bytes::from_static(str.as_bytes()))
    }

    pub fn from_bytes(bytes: Bytes) -> Result<Self, std::str::Utf8Error> {
        let _ = std::str::from_utf8(&bytes)?;
        Ok(Self(bytes))
    }

    pub unsafe fn from_bytes_unchecked(bytes: Bytes) -> Self {
        Self(bytes)
    }

    pub fn bytes(&self) -> &Bytes {
        &self.0
    }

    pub fn into_bytes(self) -> Bytes {
        self.0
    }
}

impl Deref for NetString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        unsafe {
            // Safety: by structure invariants, we know that this byte buffers's contents are always
            // valid UTF-8.
            std::str::from_utf8_unchecked(&self.0)
        }
    }
}

impl Codec<Option<u32>> for NetString {
    fn decode(
        max_len: Option<u32>,
        snip: &impl Snip,
        cursor: &mut ByteReadCursor,
    ) -> anyhow::Result<Self> {
        let size = VarUint::decode((), snip, cursor)?.0;

        if let Some(max_len) = max_len {
            let max_bytes = max_len.checked_mul(4).unwrap_or_else(|| {
                panic!(
                    "NetStrings with a maximum codepoint length of {max_len} are untenable due to \
					 encoding constraints."
                )
            });

            if size > max_bytes {
                anyhow::bail!(
					"String byte stream is too long. The string is limited to {max_len} codepoint(s), \
					 which can be encoded in up to {max_bytes} bytes, but the size of the string in \
					 bytes is specified as {size} (location: {}).",
					cursor.format_location(),
				);
            }
        }

        let Some(data) = cursor.read_slice(size as usize) else {
			anyhow::bail!(
				"Packet did not contain the necessary bytes to form the string. Available: {}, \
				 Expected: {} (location: {}).",
				 cursor.remaining().len(),
				 size,
				 cursor.format_location(),
			);
		};

        match Self::from_bytes(snip.freeze_range(data)) {
            Ok(str) => {
                // TODO: Do this in one pass.
                if let Some(max_len) = max_len {
                    let actual_len = str.chars().count();
                    if actual_len > max_len as usize {
                        anyhow::bail!(
                            "String is too long: can contain at most {max_len} codepoint(s) but \
							 contains {actual_len} (location: {}).",
                            cursor.format_location(),
                        );
                    }
                }

                Ok(str)
            }
            Err(err) => Err(anyhow::anyhow!(err).context(format!(
                "String byte data was not valid UTF8 (location: {}).",
                cursor.format_location(),
            ))),
        }
    }

    fn encode(&self, max_len: Option<u32>, cursor: &mut impl BufMut) {
        // Validate string length in debug builds.
        #[cfg(debug_assertions)]
        {
            let str_len = self.chars().count();
            let max_len = max_len.unwrap_or(u32::MAX) as usize;
            debug_assert!(
                str_len <= max_len,
                "String can be at most {max_len} codepoint(s) but ended up being {str_len}."
            );
        }

        // Send string length in bytes.
        VarInt(self.0.len().try_into().expect(TOO_BIG_ERR)).encode((), cursor);

        // Send string's UTF-8 encoded contents.
        self.bytes().encode((), cursor);
    }
}

impl SizedCodec<Option<u32>> for NetString {
    fn size(&self, _max_len: Option<u32>) -> usize {
        VarUint(self.0.len() as u32).size(()) + self.bytes().size(())
    }
}

impl Codec<u32> for NetString {
    fn decode(max_len: u32, src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Self::decode(Some(max_len), src, cursor)
    }

    fn encode(&self, max_len: u32, cursor: &mut impl BufMut) {
        self.encode(Some(max_len), cursor)
    }
}

impl SizedCodec<u32> for NetString {
    fn size(&self, max_len: u32) -> usize {
        self.size(Some(max_len))
    }
}

impl Codec<()> for NetString {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Self::decode(None, src, cursor)
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        self.encode(None, cursor)
    }
}

impl SizedCodec<()> for NetString {
    fn size(&self, _args: ()) -> usize {
        self.size(None)
    }
}

// Identifier
#[derive(Debug, Clone)]
pub struct Identifier(pub NetString);

impl Codec<()> for Identifier {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Ok(Self(NetString::decode(32767, src, cursor)?))
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        self.0.encode((), cursor);
    }
}

impl SizedCodec<()> for Identifier {
    fn size(&self, _args: ()) -> usize {
        self.0.size(())
    }
}

// JSON
#[derive(Debug, Clone)]
pub struct JsonValue<E>(pub E);

pub trait SerializableJsonValue: serde::de::DeserializeOwned + serde::Serialize {
    const MAX_STR_LEN: u32;
}

impl<E: SerializableJsonValue> Codec<()> for JsonValue<E> {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let input = NetString::decode(E::MAX_STR_LEN, src, cursor)?;
        let parsed = serde_json::from_str(&input)?;

        Ok(Self(parsed))
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        let encoded = serde_json::to_string(&self.0).unwrap();
        NetString::from_string(encoded).encode(E::MAX_STR_LEN, cursor);
    }
}

impl<E: SerializableJsonValue> SizedCodec<()> for JsonValue<E> {
    fn size(&self, _args: ()) -> usize {
        let mut counter = WriteByteCounter::default();
        serde_json::to_writer(&mut counter, &self.0).unwrap();

        VarUint(u32::try_from(counter.0).expect(TOO_BIG_ERR)).size(()) + counter.0
    }
}

// Chat
pub type Chat = JsonValue<RootChatComponent>;

#[derive(Debug, Clone)]
pub struct RootChatComponent(pub SmallVec<[ChatComponent; 1]>);

impl Serialize for RootChatComponent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.len() == 1 {
            self.0[0].serialize(serializer)
        } else {
            self.0.serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for RootChatComponent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'a> serde::de::Visitor<'a> for Visitor {
            type Value = RootChatComponent;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("root chat component")
            }

            fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'a>,
            {
                Ok(RootChatComponent(SmallVec::from([
                    ChatComponent::deserialize(deserializer)?,
                ])))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'a>,
            {
                let mut buffer = SmallVec::new();
                while let Some(elem) = seq.next_element::<ChatComponent>()? {
                    buffer.push(elem);
                }
                Ok(RootChatComponent(buffer))
            }
        }

        deserializer.deserialize_any(Visitor)
    }
}
impl SerializableJsonValue for RootChatComponent {
    const MAX_STR_LEN: u32 = 262144;
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatComponent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub translate: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub keybind: Option<String>,

    // TODO: Include score attributes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub underlined: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub obfuscated: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub insertion: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "clickEvent")]
    pub click_event: Option<ChatClickEvent>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "hoverEvent")]
    pub hover_event: Option<ChatHoverEvent>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra: Vec<ChatComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatClickEvent {
    pub action: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatHoverEvent {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_text: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_item: Option<ChatShownItem>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_entity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChatShownItem {
    pub id: String,
    pub count: i32,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

// Option
impl<A, T: Codec<A>> Codec<A> for Option<T> {
    fn decode(args: A, src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Ok(if bool::decode((), src, cursor)? {
            Some(T::decode(args, src, cursor)?)
        } else {
            None
        })
    }

    fn encode(&self, args: A, cursor: &mut impl BufMut) {
        if let Some(inner) = self {
            true.encode((), cursor);
            inner.encode(args, cursor);
        } else {
            false.encode((), cursor);
        }
    }
}

impl<A, T: SizedCodec<A>> SizedCodec<A> for Option<T> {
    fn size(&self, args: A) -> usize {
        if let Some(inner) = self {
            true.size(()) + inner.size(args)
        } else {
            false.size(())
        }
    }
}

// UUID
#[derive(Debug, Copy, Clone)]
pub struct Uuid(pub u128);

impl Codec<()> for Uuid {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        Ok(Self(u128::decode((), src, cursor)?))
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        self.0.encode((), cursor)
    }
}

impl SizedCodec<()> for Uuid {
    fn size(&self, _args: ()) -> usize {
        self.0.size(())
    }
}

// Byte Array
#[derive(Debug, Clone)]
pub struct ByteArray(Bytes);

impl Codec<()> for ByteArray {
    fn decode(_args: (), src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let len = VarInt::decode((), src, cursor)?.0;

        let Some(data) = cursor.read_slice(len as usize) else {
			anyhow::bail!(
				"Expected {len} byte(s) of data for the byte array; found {} (location: {}).",
				cursor.remaining().len(),
				cursor.format_location(),
			);
		};

        Ok(ByteArray(src.freeze_range(data)))
    }

    fn encode(&self, _args: (), cursor: &mut impl BufMut) {
        VarUint(u32::try_from(self.0.len()).expect(TOO_BIG_ERR)).encode((), cursor);
        self.0.encode((), cursor);
    }
}

impl SizedCodec<()> for ByteArray {
    fn size(&self, _args: ()) -> usize {
        VarUint(u32::try_from(self.0.len()).expect(TOO_BIG_ERR)).size(()) + self.0.len()
    }
}

// Vec
impl<A, F, T> Codec<F> for Vec<T>
where
    T: Codec<A>,
    F: FnMut() -> A,
{
    fn decode(mut args: F, src: &impl Snip, cursor: &mut ByteReadCursor) -> anyhow::Result<Self> {
        let len = VarUint::decode((), src, cursor)?.0;
        let mut builder = Vec::with_capacity(len as usize);

        for _ in 0..len {
            builder.push(T::decode(args(), src, cursor)?);
        }

        Ok(builder)
    }

    fn encode(&self, mut args: F, cursor: &mut impl BufMut) {
        VarUint(u32::try_from(self.len()).expect("vector is too large to send over the network"))
            .encode((), cursor);

        for elem in self {
            elem.encode(args(), cursor);
        }
    }
}

impl<A, F, T> SizedCodec<F> for Vec<T>
where
    T: SizedCodec<A>,
    F: FnMut() -> A,
{
    fn size(&self, mut args: F) -> usize {
        let mut accum = VarUint(
            u32::try_from(self.len()).expect("vector is too large to send over the network"),
        )
        .size(());

        for elem in self {
            accum += elem.size(args());
        }

        accum
    }
}
