use std::{
    hash::{BuildHasher, Hasher},
    marker::PhantomData,
};

use derive_where::derive_where;
use hashbrown::HashMap;
use justjson::{
    parser::{JsonKind, ParseDelegate, Parser},
    JsonString,
};

use crate::util::slice::detect_sub_slice;

// === JsonDocumentSummary === //

// Summary
#[derive_where(Debug)]
pub struct JsonDocumentSummary<S> {
    _ty: PhantomData<fn() -> S>,
    map: HashMap<JsonKey, JsonValue>,
    keys: HashMap<InternKey, u32>,
    root: JsonValue,
}

#[derive(Debug)]
struct InternKey {
    hash: u64,
    str: JsonStringSlice,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct JsonKey {
    parent: u32,
    key: u32,
}

impl<S: StaticInterner> JsonDocumentSummary<S> {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        // N.B. this check is necessary to allow us to use u32s everywhere.
        assert!(text.len() <= u32::MAX as usize);

        let mut delegate = JsonDocumentParser::<S> {
            _ty: PhantomData,
            backing: text,
            map: HashMap::default(),
            keys: HashMap::default(),
            gen: S::COUNT,
        };

        let root = Parser::parse_json(text, &mut delegate)?;

        Ok(Self {
            _ty: PhantomData,
            map: delegate.map,
            keys: delegate.keys,
            root,
        })
    }

    pub fn root(&self) -> &JsonValue {
        &self.root
    }

    pub fn root_view<'a>(&'a self, backing: &'a str) -> JsonValueView<'a, S> {
        self.view(backing).root()
    }

    pub fn view<'a>(&'a self, backing: &'a str) -> JsonDocumentView<'a, S> {
        JsonDocumentView {
            summary: self,
            backing,
        }
    }

    pub fn object_field(&self, backing: &str, obj: JsonObject, key: &str) -> Option<&JsonValue> {
        let key = JsonString::from_json(key).unwrap();
        let key = if let Some(intern) = S::try_intern(&key) {
            intern
        } else {
            let hash = hash_char_stream(self.keys.hasher(), key.len(), key.decoded());
            let Some((_, intern)) = self.keys.raw_entry().from_hash(hash, |entry| {
                entry.hash == hash && entry.str.decode(backing) == key
            }) else {
				return None;
			};

            *intern
        };

        self.map.get(&JsonKey { parent: obj.0, key })
    }

    pub fn array_element(&self, obj: JsonArray, index: u32) -> Option<&JsonValue> {
        self.map.get(&JsonKey {
            parent: obj.id,
            key: index,
        })
    }
}

// Data model
#[derive(Debug, Clone)]
pub enum JsonValue {
    Object(JsonObject),
    Array(JsonArray),
    String(JsonStringSlice),
    Number(JsonNumber),
    Boolean(bool),
    Null,
}

#[derive(Debug, Copy, Clone)]
pub struct JsonObject(u32);

#[derive(Debug, Copy, Clone)]
pub struct JsonArray {
    id: u32,
    len: u32,
}

impl JsonArray {
    pub fn len(self) -> u32 {
        self.len
    }
}

#[derive(Debug, Copy, Clone)]
pub enum JsonNumber {
    F64(f64),
    U64(u64),
    I64(i64),
}

// TODO: Make string handling more reasonable
#[derive(Debug, Clone)]
pub enum JsonStringSlice {
    Owned(Box<str>),
    Borrowed { start: u32, end: u32 },
}

impl JsonStringSlice {
    pub fn encode(backing: &str, str: &JsonString<'_>) -> Self {
        let target = 'a: {
            if let Some(justjson::AnyStr::Borrowed(borrowed)) = str.as_json_str() {
                break 'a Some(*borrowed);
            }

            if let Some(borrowed) = str.as_str() {
                break 'a Some(borrowed);
            }

            None
        };

        if let Some(range) =
            target.and_then(|target| detect_sub_slice(backing.as_bytes(), target.as_bytes()))
        {
            Self::Borrowed {
                start: range.start as u32,
                end: range.end as u32,
            }
        } else {
            Self::Owned(match str.decode_if_needed() {
                justjson::AnyStr::Owned(str) => Box::from(str),
                justjson::AnyStr::Borrowed(str) => Box::from(str),
            })
        }
    }

    pub fn decode<'a>(&'a self, backing: &'a str) -> JsonString<'a> {
        let backing = match self {
            JsonStringSlice::Owned(owned) => owned,
            JsonStringSlice::Borrowed { start, end } => {
                &backing[(*start as usize)..(*end as usize)]
            }
        };

        JsonString::from_json(&backing).unwrap()
    }
}

fn hash_char_stream(
    hasher: &impl BuildHasher,
    len: usize,
    chars: impl IntoIterator<Item = char>,
) -> u64 {
    let mut hasher = hasher.build_hasher();
    hasher.write_usize(len);
    for char in chars {
        hasher.write_u32(char as u32);
    }
    hasher.finish()
}

// === JsonDocumentView === //

// JsonDocumentView
#[derive_where(Debug, Copy, Clone)]
pub struct JsonDocumentView<'a, S: StaticInterner> {
    pub summary: &'a JsonDocumentSummary<S>,
    pub backing: &'a str,
}

impl<'a, S: StaticInterner> JsonDocumentView<'a, S> {
    pub fn root(self) -> JsonValueView<'a, S> {
        JsonValueView::wrap(self, self.summary.root())
    }

    pub fn object_field(self, obj: JsonObject, key: &str) -> Option<&'a JsonValue> {
        self.summary.object_field(self.backing, obj, key)
    }

    pub fn array_element(self, obj: JsonArray, index: u32) -> Option<&'a JsonValue> {
        self.summary.array_element(obj, index)
    }

    pub fn string_value<'b>(self, str: &'b JsonStringSlice) -> JsonString<'b>
    where
        'a: 'b,
    {
        str.decode(self.backing)
    }
}

// JsonValueView
#[derive_where(Debug, Copy, Clone)]
pub enum JsonValueView<'a, S: StaticInterner> {
    Object(JsonObjectView<'a, S>),
    Array(JsonArrayView<'a, S>),
    String(JsonStringSliceView<'a, S>),
    Number(JsonNumber),
    Boolean(bool),
    Null,
}

impl<'a, S: StaticInterner> JsonValueView<'a, S> {
    pub fn wrap(document: JsonDocumentView<'a, S>, value: &'a JsonValue) -> Self {
        match value {
            JsonValue::Object(handle) => Self::Object(JsonObjectView {
                document,
                handle: *handle,
            }),
            JsonValue::Array(handle) => Self::Array(JsonArrayView {
                document,
                handle: *handle,
            }),
            JsonValue::String(handle) => Self::String(JsonStringSliceView { document, handle }),
            JsonValue::Number(number) => Self::Number(*number),
            JsonValue::Boolean(value) => Self::Boolean(*value),
            JsonValue::Null => Self::Null,
        }
    }
}

// JsonObjectView
#[derive_where(Debug, Copy, Clone)]
pub struct JsonObjectView<'a, S: StaticInterner> {
    pub document: JsonDocumentView<'a, S>,
    pub handle: JsonObject,
}

impl<'a, S: StaticInterner> JsonObjectView<'a, S> {
    pub fn get(&self, key: &str) -> Option<JsonValueView<'a, S>> {
        self.document
            .object_field(self.handle, key)
            .map(|value| JsonValueView::wrap(self.document, value))
    }
}

// JsonArrayView
#[derive_where(Debug, Copy, Clone)]
pub struct JsonArrayView<'a, S: StaticInterner> {
    pub document: JsonDocumentView<'a, S>,
    pub handle: JsonArray,
}

impl<'a, S: StaticInterner> JsonArrayView<'a, S> {
    pub fn get(&self, index: u32) -> Option<JsonValueView<'a, S>> {
        self.document
            .array_element(self.handle, index)
            .map(|value| JsonValueView::wrap(self.document, value))
    }
}

// JsonStringSliceView
#[derive_where(Debug, Copy, Clone)]
pub struct JsonStringSliceView<'a, S: StaticInterner> {
    pub document: JsonDocumentView<'a, S>,
    pub handle: &'a JsonStringSlice,
}

// TODO: Finish implementing

// === JsonDocumentParser === //

#[derive(Debug)]
struct JsonDocumentParser<'a, S> {
    _ty: PhantomData<fn() -> S>,
    backing: &'a str,
    map: HashMap<JsonKey, JsonValue>,
    keys: HashMap<InternKey, u32>,
    gen: u32,
}

#[derive(Debug)]
struct ObjectOrArrayBuilder {
    id: u32,
    len: u32,
}

impl<'a, S: StaticInterner> ParseDelegate<'a> for &'_ mut JsonDocumentParser<'a, S> {
    type Value = JsonValue;
    type Object = ObjectOrArrayBuilder;
    type Array = ObjectOrArrayBuilder;
    type Key = u32;
    type Error = anyhow::Error;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Null)
    }

    fn boolean(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Boolean(value))
    }

    fn number(&mut self, value: justjson::JsonNumber<'a>) -> Result<Self::Value, Self::Error> {
        if let Some(value) = value.as_u64() {
            return Ok(JsonValue::Number(JsonNumber::U64(value)));
        }

        if let Some(value) = value.as_i64() {
            return Ok(JsonValue::Number(JsonNumber::I64(value)));
        }

        if let Some(value) = value.as_f64() {
            return Ok(JsonValue::Number(JsonNumber::F64(value)));
        }

        anyhow::bail!("Failed to parse JSON number {:?}", value.source());
    }

    fn string(&mut self, value: justjson::JsonString<'a>) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::String(JsonStringSlice::encode(
            self.backing,
            &value,
        )))
    }

    fn begin_object(&mut self) -> Result<Self::Object, Self::Error> {
        self.gen += 1;

        Ok(ObjectOrArrayBuilder {
            id: self.gen,
            len: 0,
        })
    }

    fn object_key(
        &mut self,
        _object: &mut Self::Object,
        key: justjson::JsonString<'a>,
    ) -> Result<Self::Key, Self::Error> {
        if let Some(intern) = S::try_intern(&key) {
            Ok(intern)
        } else {
            let hash = hash_char_stream(self.keys.hasher(), key.decoded_len(), key.decoded());
            let entry = self.keys.raw_entry_mut().from_hash(hash, |entry| {
                entry.hash == hash && entry.str.decode(self.backing) == key
            });

            match entry {
                hashbrown::hash_map::RawEntryMut::Occupied(entry) => Ok(*entry.get()),
                hashbrown::hash_map::RawEntryMut::Vacant(entry) => {
                    self.gen += 1;
                    entry.insert_with_hasher(
                        hash,
                        InternKey {
                            hash: hash,
                            str: JsonStringSlice::encode(self.backing, &key),
                        },
                        self.gen,
                        |entry| entry.hash,
                    );

                    Ok(self.gen)
                }
            }
        }
    }

    fn object_value(
        &mut self,
        object: &mut Self::Object,
        key: Self::Key,
        value: Self::Value,
    ) -> Result<(), Self::Error> {
        self.map.insert(
            JsonKey {
                parent: object.id,
                key,
            },
            value,
        );
        object.len += 1;

        Ok(())
    }

    fn object_is_empty(&self, object: &Self::Object) -> bool {
        object.len == 0
    }

    fn end_object(&mut self, object: Self::Object) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Object(JsonObject(object.id)))
    }

    fn begin_array(&mut self) -> Result<Self::Array, Self::Error> {
        self.gen += 1;

        Ok(ObjectOrArrayBuilder {
            id: self.gen,
            len: 0,
        })
    }

    fn array_value(
        &mut self,
        array: &mut Self::Array,
        value: Self::Value,
    ) -> Result<(), Self::Error> {
        self.map.insert(
            JsonKey {
                parent: array.id,
                key: array.len,
            },
            value,
        );
        array.len += 1;

        Ok(())
    }

    fn array_is_empty(&self, array: &Self::Array) -> bool {
        array.len == 0
    }

    fn end_array(&mut self, array: Self::Array) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Array(JsonArray {
            id: array.id,
            len: array.len,
        }))
    }

    fn kind_of(&self, value: &Self::Value) -> JsonKind {
        match value {
            JsonValue::Object(_) => JsonKind::Object,
            JsonValue::Array(_) => JsonKind::Array,
            JsonValue::Number(_) => JsonKind::Number,
            JsonValue::Boolean(_) => JsonKind::Boolean,
            JsonValue::String(_) => JsonKind::String,
            JsonValue::Null => JsonKind::Null,
        }
    }
}

// === StaticInterner === //

pub trait StaticInterner {
    const COUNT: u32;

    fn try_intern(text: &JsonString) -> Option<u32>;
}
