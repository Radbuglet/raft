use std::{fmt, marker::PhantomData, ops::Deref};

use derive_where::derive_where;
use hashbrown::HashMap;
use justjson::parser::{JsonKind, ParseDelegate, Parser};

use crate::util::interner::{Intern, Interner};

use super::{
    core::Codec,
    decode_schema::{DeserializeSchema, SchemaDecodeCodec, SchemaDocument, SchemaView},
};

// === JsonDocument === //

// Container
#[derive(Debug, Clone)]
pub struct JsonDocument {
    interner: Interner,
    map: HashMap<JsonKey, JsonValue>,
    root: JsonValue,
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct JsonKey {
    parent: u32,
    key: u32,
}

impl JsonDocument {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        // N.B. this check is necessary to allow us to use u32s everywhere.
        assert!(text.len() <= u32::MAX as usize);

        let mut delegate = JsonDocumentParser {
            interner: Interner::default(),
            map: HashMap::default(),
            gen: 0,
        };

        let root = Parser::parse_json(text, &mut delegate)?;

        Ok(Self {
            interner: delegate.interner,
            map: delegate.map,
            root,
        })
    }

    pub fn root(&self) -> JsonValue {
        self.root
    }

    pub fn root_view(&self) -> JsonValueView<'_> {
        JsonValueView::wrap(self, self.root)
    }

    pub fn object_field(&self, obj: JsonObject, key: &str) -> Option<JsonValue> {
        let key = self.interner.find_intern(key)?;
        self.map
            .get(&JsonKey {
                parent: obj.0,
                key: key.id(),
            })
            .copied()
    }

    pub fn array_element(&self, obj: JsonArray, index: u32) -> Option<JsonValue> {
        self.map
            .get(&JsonKey {
                parent: obj.id,
                key: index,
            })
            .copied()
    }

    pub fn string_value(&self, intern: Intern) -> &str {
        self.interner.decode(intern)
    }
}

// Data model
#[derive(Debug, Copy, Clone)]
pub enum JsonValue {
    Object(JsonObject),
    Array(JsonArray),
    String(Intern),
    Number(JsonNumber),
    Boolean(bool),
    Null,
}

impl JsonValue {
    pub fn as_view(self, document: &JsonDocument) -> JsonValueView<'_> {
        JsonValueView::wrap(document, self)
    }
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

// === JsonDocument Views === //

#[derive(Debug, Copy, Clone)]
pub enum JsonValueView<'a> {
    Object(JsonObjectView<'a>),
    Array(JsonArrayView<'a>),
    String(JsonStringView<'a>),
    Number(JsonNumber),
    Boolean(bool),
    Null,
}

impl<'a> JsonValueView<'a> {
    pub fn wrap(document: &'a JsonDocument, value: JsonValue) -> Self {
        match value {
            JsonValue::Object(handle) => Self::Object(JsonObjectView { document, handle }),
            JsonValue::Array(handle) => Self::Array(JsonArrayView { document, handle }),
            JsonValue::String(intern) => Self::String(JsonStringView {
                document,
                intern,
                text: document.string_value(intern),
            }),
            JsonValue::Number(number) => Self::Number(number),
            JsonValue::Boolean(bool) => Self::Boolean(bool),
            JsonValue::Null => Self::Null,
        }
    }

    pub fn unwrap(self) -> JsonValue {
        match self {
            JsonValueView::Object(obj) => JsonValue::Object(obj.handle),
            JsonValueView::Array(arr) => JsonValue::Array(arr.handle),
            JsonValueView::String(str) => JsonValue::String(str.intern),
            JsonValueView::Number(num) => JsonValue::Number(num),
            JsonValueView::Boolean(b) => JsonValue::Boolean(b),
            JsonValueView::Null => JsonValue::Null,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct JsonObjectView<'a> {
    pub document: &'a JsonDocument,
    pub handle: JsonObject,
}

impl<'a> JsonObjectView<'a> {
    pub fn get(self, key: &str) -> Option<JsonValueView<'a>> {
        self.document
            .object_field(self.handle, key)
            .map(|handle| JsonValueView::wrap(self.document, handle))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct JsonArrayView<'a> {
    pub document: &'a JsonDocument,
    pub handle: JsonArray,
}

impl<'a> JsonArrayView<'a> {
    pub fn get(self, idx: u32) -> Option<JsonValueView<'a>> {
        self.document
            .array_element(self.handle, idx)
            .map(|handle| JsonValueView::wrap(self.document, handle))
    }

    pub fn len(self) -> u32 {
        self.handle.len()
    }

    pub fn iter(self) -> impl Iterator<Item = JsonValueView<'a>> + 'a {
        (0..self.len()).map_while(move |i| self.get(i))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct JsonStringView<'a> {
    pub document: &'a JsonDocument,
    pub intern: Intern,
    pub text: &'a str,
}

impl Deref for JsonStringView<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.text
    }
}

// === JsonDocumentParser === //

#[derive(Debug)]
struct JsonDocumentParser {
    interner: Interner,
    map: HashMap<JsonKey, JsonValue>,
    gen: u32,
}

#[derive(Debug)]
struct ObjectOrArrayBuilder {
    id: u32,
    len: u32,
}

impl ParseDelegate<'_> for &'_ mut JsonDocumentParser {
    type Value = JsonValue;
    type Object = ObjectOrArrayBuilder;
    type Array = ObjectOrArrayBuilder;
    type Key = Intern;
    type Error = anyhow::Error;

    fn null(&mut self) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Null)
    }

    fn boolean(&mut self, value: bool) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::Boolean(value))
    }

    fn number(&mut self, value: justjson::JsonNumber<'_>) -> Result<Self::Value, Self::Error> {
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

    fn string(&mut self, value: justjson::JsonString<'_>) -> Result<Self::Value, Self::Error> {
        Ok(JsonValue::String(
            self.interner.intern_iter(value.decoded()),
        ))
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
        key: justjson::JsonString<'_>,
    ) -> Result<Self::Key, Self::Error> {
        Ok(self.interner.intern_iter(key.decoded()))
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
                key: key.id(),
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

// === Schema Integration === //

// Codec
pub struct JsonSchema;

impl Codec for JsonSchema {}

impl SchemaDecodeCodec for JsonSchema {
    type Document = JsonDocument;
    type AnyRef = JsonValue;
    type ObjectRef = JsonObject;
}

impl SchemaDocument for JsonDocument {
    type AnyRef = JsonValue;
    type ObjectRef = JsonObject;

    fn root(&self) -> Self::AnyRef {
        self.root()
    }

    fn any_ref_as_object(&self, any_ref: Self::AnyRef) -> Result<Self::ObjectRef, Self::AnyRef> {
        match any_ref {
            JsonValue::Object(obj) => Ok(obj),
            value @ _ => Err(value),
        }
    }

    fn object_entry(&self, obj: &Self::ObjectRef, key: &str) -> Option<Self::AnyRef> {
        self.object_field(*obj, key)
    }
}

// Option
impl<T> DeserializeSchema<JsonSchema, ()> for Option<T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Shortcut = Option<T::Shortcut>;
    type View<'a> = Option<T::View<'a>>;

    fn make_shortcut(
        document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            None | Some(JsonValue::Null) => Ok(None),
            value @ _ => Ok(Some(T::make_shortcut(document, object)?)),
        }
    }

    fn view_shortcut<'a>(
        document: &'a <JsonSchema as SchemaDecodeCodec>::Document,
        shortcut: Self::Shortcut,
        args: (),
    ) -> Self::View<'a> {
        shortcut.map(|shortcut| T::view_shortcut(document, shortcut, args))
    }
}

impl<V: SchemaView<JsonSchema, ()>> SchemaView<JsonSchema, ()> for Option<V> {
    type Reified = Option<V::Reified>;
    type Shortcut = Option<V::Shortcut>;

    fn validate_deep(&self) -> anyhow::Result<()> {
        if let Some(inner) = self {
            inner.validate_deep()?;
        }
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.map(|v| v.as_shortcut())
    }

    fn reify(&self) -> anyhow::Result<Self::Reified> {
        if let Some(inner) = self {
            Ok(Some(inner.reify()?))
        } else {
            Ok(None)
        }
    }
}

// Array
impl<T> DeserializeSchema<JsonSchema, ()> for Vec<T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Shortcut = JsonArray;
    type View<'a> = ArrayView<'a, T>;

    fn make_shortcut(
        _document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            Some(JsonValue::Array(array)) => Ok(array),
            value @ _ => anyhow::bail!("Expected array, got {value:?}"),
        }
    }

    fn view_shortcut<'a>(
        document: &'a <JsonSchema as SchemaDecodeCodec>::Document,
        shortcut: Self::Shortcut,
        args: (),
    ) -> Self::View<'a> {
        ArrayView {
            _ty: PhantomData,
            view: JsonArrayView {
                document,
                handle: shortcut,
            },
        }
    }
}

#[derive_where(Copy, Clone)]
pub struct ArrayView<'a, T> {
    _ty: PhantomData<fn() -> T>,
    view: JsonArrayView<'a>,
}

impl<T> fmt::Debug for ArrayView<'_, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<'a, T> ArrayView<'a, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    pub fn get(self, i: u32) -> Option<anyhow::Result<T::View<'a>>> {
        self.view
            .get(i)
            .map(|object| T::view_object(self.view.document, Some(object.unwrap()), ()))
    }

    pub fn len(self) -> u32 {
        self.view.len()
    }

    pub fn iter(self) -> impl Iterator<Item = anyhow::Result<T::View<'a>>> {
        (0..self.len()).map_while(move |i| self.get(i))
    }
}

impl<T> SchemaView<JsonSchema, ()> for ArrayView<'_, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Reified = Vec<T>;
    type Shortcut = JsonArray;

    fn validate_deep(&self) -> anyhow::Result<()> {
        for elem in self.iter() {
            elem?.validate_deep()?;
        }
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.view.handle
    }

    fn reify(&self) -> anyhow::Result<Self::Reified> {
        let mut out = Vec::<T>::with_capacity(self.len() as usize);
        for elem in self.iter() {
            out.push(elem?.reify()?);
        }
        Ok(out)
    }
}

// Number
// TODO

// Boolean
impl DeserializeSchema<JsonSchema, ()> for bool {
    type Shortcut = bool;
    type View<'a> = bool;

    fn make_shortcut(
        document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            Some(JsonValue::Boolean(value)) => Ok(value),
            value @ _ => anyhow::bail!("Expected boolean, got {value:?}"),
        }
    }

    fn view_shortcut<'a>(
        _document: &'a JsonDocument,
        shortcut: Self::Shortcut,
        _args: (),
    ) -> Self::View<'a> {
        shortcut
    }
}

impl SchemaView<JsonSchema, ()> for bool {
    type Reified = bool;
    type Shortcut = bool;

    fn validate_deep(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        *self
    }

    fn reify(&self) -> anyhow::Result<Self::Reified> {
        Ok(*self)
    }
}

// String
#[derive(Debug, Copy, Clone)]
pub struct StringView<'a> {
    intern: Intern,
    text: &'a str,
}

impl DeserializeSchema<JsonSchema, ()> for String {
    type Shortcut = Intern;
    type View<'a> = StringView<'a>;

    fn make_shortcut(
        _document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            Some(JsonValue::String(intern)) => Ok(intern),
            value @ _ => anyhow::bail!("Expected string, got {value:?}."),
        }
    }

    fn view_shortcut<'a>(document: &'a JsonDocument, shortcut: Intern, args: ()) -> Self::View<'a> {
        StringView {
            intern: shortcut,
            text: document.string_value(shortcut),
        }
    }
}

impl SchemaView<JsonSchema, ()> for StringView<'_> {
    type Reified = String;
    type Shortcut = Intern;

    fn validate_deep(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.intern
    }

    fn reify(&self) -> anyhow::Result<Self::Reified> {
        Ok(self.text.to_string())
    }
}

impl Deref for StringView<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.text
    }
}
