use std::{fmt, io::Write, mem, str};

use anyhow::Context;
use bytes::Bytes;
use either::Either;

use crate::util::{
    proto::{
        byte_stream::{ByteCursor, ByteSize, ByteWriteStream, WriteCodepointCounter},
        core::{schema_codec_struct, Codec},
        decode_schema::{DeserializeSchema, SchemaView, ValidatedSchemaView},
        decode_seq::{
            DeserializeSeq, DeserializeSeqFor, DeserializeSeqForSimple, EndPosSummary,
            SeqDecodeCodec,
        },
        encode::{EncodeCodec, SerializeFrom, SerializeInto, WriteStreamFor},
        json_document::{JsonDocument, JsonSchema},
    },
    var_int::{decode_var_i32_streaming, encode_var_u32},
};

// === Codec === //

pub struct MineCodec {
    _never: (),
}

impl Codec for MineCodec {}

impl SeqDecodeCodec for MineCodec {
    type Reader<'a> = ByteCursor<'a>;
    type ReaderPos = usize;

    fn covariant_cast<'a: 'b, 'b>(reader: ByteCursor<'a>) -> ByteCursor<'b> {
        reader
    }
}

impl EncodeCodec for MineCodec {
    type WriteElement<'a> = [u8];
    type SizeMetric = ByteSize;
}

// === Numerics === //

// Primitives
macro_rules! impl_numerics {
	($($ty:ty),*$(,)?) => {$(
		impl DeserializeSeq<MineCodec> for $ty {
			type Summary = ();
			type View<'a> = Self;

			fn reify_view(view: &Self::View<'_>) -> Self {
				*view
			}
		}

		impl DeserializeSeqForSimple<MineCodec, ()> for $ty {
			fn decode_simple<'a>(
				_bind: [&'a (); 0],
				cursor: &mut ByteCursor<'a>,
				_args: &mut (),
			) -> anyhow::Result<Self::View<'a>> {
				let arr = cursor.read_arr()
					.ok_or_else(|| anyhow::anyhow!(
						"Unexpected end-of-stream while reading {}: expected {} byte(s), got {} \
						 (location: {}).",
						stringify!($ty),
						mem::size_of::<Self>(),
						cursor.remaining().len(),
						cursor.format_location(),
					))?;

				Ok(Self::from_be_bytes(arr))
			}
		}

		impl SerializeInto<MineCodec, $ty, ()> for $ty {
			fn serialize(&mut self, stream: &mut impl WriteStreamFor<MineCodec>, _args: &mut ()) -> anyhow::Result<()> {
				stream.push(&self.to_be_bytes())?;
				Ok(())
			}
		}
	)*};
}

impl_numerics!(i8, u8, i16, u16, i32, u32, i64, f32, f64, u128);

impl DeserializeSeq<MineCodec> for bool {
    type Summary = ();
    type View<'a> = Self;

    fn reify_view(view: &Self::View<'_>) -> Self {
        *view
    }
}

impl DeserializeSeqForSimple<MineCodec, ()> for bool {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let byte = u8::decode_simple([], cursor, &mut ())?;
        match byte {
            0 => Ok(false),
            1 => Ok(true),
            _ => anyhow::bail!(
                "Invalid variant for boolean: expected 0 or 1, got {byte} (location: {})",
                cursor.format_location(),
            ),
        }
    }
}

impl SerializeInto<MineCodec, bool, ()> for bool {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, u8, ()>::serialize(&mut (*self as u8), stream, &mut ())
    }
}

// VarInt
#[derive(Debug, Copy, Clone)]
pub struct VarInt(pub i32);

impl DeserializeSeq<MineCodec> for VarInt {
    type Summary = EndPosSummary<usize>;
    type View<'a> = i32;

    fn reify_view(view: &Self::View<'_>) -> Self {
        Self(*view)
    }
}

impl DeserializeSeqForSimple<MineCodec, ()> for VarInt {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        decode_var_i32_streaming(cursor)?
            .ok_or_else(|| anyhow::anyhow!("Unterminated VarInt (location: {})", cursor.pos()))
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for VarInt {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        encode_var_u32(&mut stream.as_write(), self.0)?;
        Ok(())
    }
}

impl SerializeInto<MineCodec, VarInt, ()> for i32 {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
        VarInt(*self).serialize(stream, args)
    }
}

// VarUint
#[derive(Debug, Copy, Clone)]
pub struct VarUint(pub u32);

impl DeserializeSeq<MineCodec> for VarUint {
    type Summary = EndPosSummary<usize>;
    type View<'a> = u32;

    fn reify_view(view: &Self::View<'_>) -> Self {
        Self(*view)
    }
}

impl DeserializeSeqForSimple<MineCodec, ()> for VarUint {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let value = VarInt::decode_simple([], cursor, &mut ())?;
        u32::try_from(value).map_err(|_| {
            anyhow::anyhow!(
                "Unexpected negative VarUint with value {value} (location: {}).",
                cursor.format_location()
            )
        })
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for VarUint {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
        let value = i32::try_from(self.0).context("Attempted to send oversized VarUint")?;
        VarInt(value).serialize(stream, args)
    }
}

impl SerializeInto<MineCodec, VarUint, ()> for u32 {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut (),
    ) -> anyhow::Result<()> {
        VarUint(*self).serialize(stream, args)
    }
}

// === Strings === //

// String
impl DeserializeSeq<MineCodec> for String {
    type Summary = usize;
    type View<'a> = &'a str;

    fn reify_view(view: &Self::View<'_>) -> Self {
        String::from(*view)
    }
}

impl DeserializeSeqFor<MineCodec, Option<u32>> for String {
    fn summarize(
        cursor: &mut ByteCursor,
        max_len: &mut Option<u32>,
    ) -> anyhow::Result<Self::Summary> {
        let start_pos = cursor.pos();
        let size = VarUint::decode_simple([], cursor, &mut ())?;

        // Validate length
        if let Some(max_len) = *max_len {
            let max_size = max_len
                .checked_mul(4)
                .filter(|&v| i32::try_from(v).is_ok())
                .unwrap_or_else(|| {
                    panic!(
						"Strings with a maximum codepoint length of {max_len} are untenable due to \
						 encoding constraints."
					)
                });

            anyhow::ensure!(
                size <= max_size,
                "String byte stream is too long. The string is limited to {max_len} codepoint(s), \
				 which can be encoded in up to {max_size} bytes, but the size of the string in \
				 bytes is specified as {size} (location: {}).",
                cursor.format_location(),
            );
        }

        // Fetch bytes
        let data = cursor.read_slice(size as usize).ok_or_else(|| {
            anyhow::anyhow!(
                "Packet did not contain the necessary bytes to form the string. Available: {}, \
				 Expected: {} (location: {}).",
                cursor.remaining().len(),
                size,
                cursor.format_location(),
            )
        })?;

        // Validate bytes
        let mut codepoints = WriteCodepointCounter::default();
        codepoints.write_all(data)?;
        let codepoints = codepoints.codepoints().ok_or_else(|| {
            anyhow::anyhow!(
                "String byte data was not valid UTF8 (location: {})",
                cursor.format_location(),
            )
        })?;

        if let Some(max_len) = *max_len {
            anyhow::ensure!(
                codepoints <= max_len as usize,
                "String is too long: can contain at most {max_len} codepoint(s) but \
				 contains {codepoints} (location: {}).",
                cursor.format_location(),
            );
        }

        Ok(start_pos)
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        _args: &mut Option<u32>,
    ) -> Self::View<'a> {
        let mut cursor = cursor.with_pos(*summary);
        let size = VarUint::decode_simple([], &mut cursor, &mut ()).unwrap();
        let data = cursor.read_slice(size as usize).unwrap();

        // Safety: `summarize` already validated that parsing from the `*summary` buffer location
        // on its corresponding buffer (guaranteed by safety invariants) onwards will result in
        // a valid string being constructed, allowing us to skip the validation step.
        str::from_utf8_unchecked(data)
    }

    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        _args: &mut Option<u32>,
    ) {
        debug_assert_eq!(cursor.pos(), *summary);

        skip_to_start(cursor);
        let byte_len = VarInt::decode_simple([], cursor, &mut ()).unwrap();
        let _ = cursor.read_slice(byte_len as usize);
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, String, Option<u32>> for T {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut Option<u32>,
    ) -> anyhow::Result<()> {
        // Determine the size of the string.
        let mut counter = WriteCodepointCounter::default();
        write!(&mut counter, "{self}")?;

        // Validate length
        if let Some(max_len) = *args {
            let curr_len = counter.codepoints().unwrap();
            anyhow::ensure!(
				curr_len <= max_len as usize,
				"String {:?} has a max length of {max_len} codepoint(s) but was {curr_len} codepoint(s) long.",
				self.to_string(),
			);
        }

        // Write out the packet
        let len = i32::try_from(counter.bytes())
            .map_err(|_| anyhow::anyhow!("String max length overflew an i32."))?;

        VarInt(len).serialize(stream, &mut ())?;
        write!(stream.as_write(), "{self}")?;

        Ok(())
    }
}

impl DeserializeSeqFor<MineCodec, u32> for String {
    fn summarize(cursor: &mut ByteCursor, args: &mut u32) -> anyhow::Result<Self::Summary> {
        Self::summarize(cursor, &mut Some(*args))
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        args: &mut u32,
    ) -> Self::View<'a> {
        Self::view(summary, cursor, &mut Some(*args))
    }

    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        args: &mut u32,
    ) {
        Self::skip(summary, skip_to_start, cursor, &mut Some(*args))
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, String, u32> for T {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut u32,
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, String, Option<u32>>::serialize(self, stream, &mut Some(*args))
    }
}

// Identifier
#[derive(Debug, Clone)]
pub struct Identifier(pub String);

impl Identifier {
    pub const MAX_LEN: u32 = 32767;
}

impl fmt::Display for Identifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl DeserializeSeq<MineCodec> for Identifier {
    type Summary = <String as DeserializeSeq<MineCodec>>::Summary;
    type View<'a> = &'a str;

    fn reify_view(view: &Self::View<'_>) -> Self {
        Self(String::from(*view))
    }
}

impl DeserializeSeqFor<MineCodec, ()> for Identifier {
    fn summarize(cursor: &mut ByteCursor, _args: &mut ()) -> anyhow::Result<Self::Summary> {
        String::summarize(cursor, &mut Some(Self::MAX_LEN))
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        _args: &mut (),
    ) -> Self::View<'a> {
        String::view(summary, cursor, &mut Some(Self::MAX_LEN))
    }

    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        _args: &mut (),
    ) {
        String::skip(summary, skip_to_start, cursor, &mut Some(Self::MAX_LEN))
    }
}

impl<T: fmt::Display> SerializeInto<MineCodec, Identifier, ()> for T {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        SerializeInto::<MineCodec, String, Option<u32>>::serialize(
            self,
            stream,
            &mut Some(Identifier::MAX_LEN),
        )
    }
}

// JSON
#[derive(Debug, Clone)]
pub struct Json<T>(pub T);

pub trait MineProtoJsonValue: DeserializeSchema<JsonSchema, ()> {
    const MAX_LEN: u32;
}

impl<T: MineProtoJsonValue> DeserializeSeq<MineCodec> for Json<T> {
    type Summary = (JsonDocument, usize);
    type View<'a> = T::ValidatedView<'a>;

    fn reify_view(view: &Self::View<'_>) -> Self {
        Self(view.reify())
    }
}

impl<T: MineProtoJsonValue> DeserializeSeqFor<MineCodec, ()> for Json<T> {
    fn summarize(cursor: &mut ByteCursor, _args: &mut ()) -> anyhow::Result<Self::Summary> {
        String::summarize_and_view(cursor, &mut Some(T::MAX_LEN), |cursor, text| {
            let document = JsonDocument::parse(text)?;
            T::view_object(&document, Some(document.root()), ())?.validate_deep()?;

            Ok((document, cursor.pos()))
        })
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        _cursor: ByteCursor<'a>,
        _args: &mut (),
    ) -> Self::View<'a> {
        T::view_object(&summary.0, Some(summary.0.root()), ())
            .unwrap()
            .assume_valid()
    }

    fn skip(
        summary: &Self::Summary,
        _skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        _args: &mut (),
    ) {
        cursor.set_pos(summary.1);
    }
}

// Chat
pub type Chat = Json<ChatRoot>;

pub type ChatRoot = Either<Vec<ChatComponent>, ChatComponent>;

schema_codec_struct! {
    pub struct chat_component::ChatComponent(JsonSchema) {
        text: Option<String>,
        translate: Option<String>,
        keybind: Option<String>,
        bold: Option<bool>,
        italic: Option<bool>,
        underlined: Option<bool>,
        strikethrough: Option<bool>,
        obfuscated: Option<bool>,
        font: Option<String>,
        color: Option<String>,
        insertion: Option<String>,
        click_event: Option<ChatClickEvent>,
        hover_event: Option<ChatHoverEvent>,
    }

    pub struct chat_click_event::ChatClickEvent(JsonSchema) {
        action: String,
        value: String,
    }

    pub struct chat_hover_event::ChatHoverEvent(JsonSchema) {
        show_text: Option<String>,
        show_item: Option<ChatShownItem>,
        show_entity: Option<String>,
    }


    pub struct chat_shown_item::ChatShownItem(JsonSchema) {
        id: String,
        count: u8,
        tag: Option<String>,
}
}

impl MineProtoJsonValue for ChatRoot {
    const MAX_LEN: u32 = 262144;
}

// === Containers === //

// TrailingByteArray
#[derive(Debug, Clone)]
pub struct TrailingByteArray(pub Bytes);

impl DeserializeSeq<MineCodec> for TrailingByteArray {
    type Summary = ();
    type View<'a> = &'a [u8];

    fn reify_view(view: &Self::View<'_>) -> Self {
        Self(Bytes::from(Vec::from(*view)))
    }
}

impl DeserializeSeqForSimple<MineCodec, ()> for TrailingByteArray {
    fn decode_simple<'a>(
        _bind: [&'a (); 0],
        cursor: &mut ByteCursor<'a>,
        _args: &mut (),
    ) -> anyhow::Result<Self::View<'a>> {
        let remaining = cursor.remaining();
        cursor.advance_remaining();
        Ok(remaining)
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for TrailingByteArray {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        stream.push(&self.0)?;
        Ok(())
    }
}

impl SerializeInto<MineCodec, TrailingByteArray, ()> for &'_ [u8] {
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        _args: &mut (),
    ) -> anyhow::Result<()> {
        stream.push(self)?;
        Ok(())
    }
}

// Option
impl<T> DeserializeSeq<MineCodec> for Option<T>
where
    T: DeserializeSeq<MineCodec>,
{
    type Summary = (Option<T::Summary>, usize);
    type View<'a> = Option<T::View<'a>>;

    fn reify_view(view: &Self::View<'_>) -> Self {
        view.as_ref().map(|view| T::reify_view(view))
    }
}

impl<T, A> DeserializeSeqFor<MineCodec, (A,)> for Option<T>
where
    T: DeserializeSeqFor<MineCodec, A>,
{
    fn summarize(cursor: &mut ByteCursor, args: &mut (A,)) -> anyhow::Result<Self::Summary> {
        if bool::decode(cursor, &mut ())? {
            Ok((Some(T::summarize(cursor, &mut args.0)?), cursor.pos()))
        } else {
            Ok((None, cursor.pos()))
        }
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        mut cursor: ByteCursor<'a>,
        args: &mut (A,),
    ) -> Self::View<'a> {
        // Skip the boolean field
        cursor.advance(1);

        // Produce the view
        summary
            .0
            .as_ref()
            .map(|summary| T::view(summary, cursor, &mut args.0))
    }

    fn skip(
        summary: &Self::Summary,
        _skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        _args: &mut (A,),
    ) {
        cursor.set_pos(summary.1);
    }
}

impl<T> DeserializeSeqFor<MineCodec, ()> for Option<T>
where
    T: DeserializeSeqFor<MineCodec, ()>,
{
    fn summarize(cursor: &mut ByteCursor, _args: &mut ()) -> anyhow::Result<Self::Summary> {
        <Option<T> as DeserializeSeqFor<MineCodec, ((),)>>::summarize(cursor, &mut ((),))
    }

    unsafe fn view<'a>(
        summary: &'a Self::Summary,
        cursor: ByteCursor<'a>,
        _args: &mut (),
    ) -> Self::View<'a> {
        <Option<T> as DeserializeSeqFor<MineCodec, ((),)>>::view(summary, cursor, &mut ((),))
    }

    fn skip(
        summary: &Self::Summary,
        skip_to_start: impl Fn(&mut ByteCursor),
        cursor: &mut ByteCursor,
        _args: &mut (),
    ) {
        <Option<T> as DeserializeSeqFor<MineCodec, ((),)>>::skip(
            summary,
            skip_to_start,
            cursor,
            &mut ((),),
        )
    }
}

impl<T, V, A> SerializeInto<MineCodec, Option<T>, A> for Option<V>
where
    V: SerializeInto<MineCodec, T, A>,
{
    fn serialize(
        &mut self,
        stream: &mut impl WriteStreamFor<MineCodec>,
        args: &mut A,
    ) -> anyhow::Result<()> {
        if let Some(inner) = self {
            bool::serialize_from(&mut true, stream, &mut ())?;
            T::serialize_from(inner, stream, args)?;
            Ok(())
        } else {
            bool::serialize_from(&mut false, stream, &mut ())?;
            Ok(())
        }
    }
}

// Vec
// TODO
