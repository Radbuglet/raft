use std::{fmt, marker::PhantomData, ops::Deref};

use derive_where::derive_where;
use either::Either;
use hashbrown::HashMap;
use justjson::parser::{JsonKind, ParseDelegate, Parser};

use crate::util::interner::{Intern, Interner};

use super::{
    core::Codec,
    decode_schema::{
        DeserializeSchema, SchemaDecodeCodec, SchemaDocument, SchemaView, ValidatedSchemaView,
    },
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

impl JsonNumber {
    pub fn as_uint(self) -> anyhow::Result<u64> {
        match self {
            // TODO: Check this logic
            JsonNumber::F64(v) => Ok(v as u64),
            JsonNumber::U64(v) => Ok(v),
            JsonNumber::I64(v) => Ok(u64::try_from(v)?),
        }
    }

    pub fn as_int(self) -> anyhow::Result<i64> {
        match self {
            // TODO: Check this logic
            JsonNumber::F64(v) => Ok(v as i64),
            JsonNumber::U64(v) => Ok(i64::try_from(v)?),
            JsonNumber::I64(v) => Ok(v),
        }
    }

    pub fn as_float(self) -> anyhow::Result<f64> {
        match self {
            JsonNumber::F64(v) => Ok(v),
            JsonNumber::U64(v) => Ok(v as f64),
            JsonNumber::I64(v) => Ok(v as f64),
        }
    }
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
    type ValidatedView<'a> = Option<T::ValidatedView<'a>>;

    fn make_shortcut(
        document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            None | Some(JsonValue::Null) => Ok(None),
            Some(value) => Ok(Some(T::make_shortcut(document, Some(value))?)),
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
    type Validated = Option<V::Validated>;

    fn assume_valid(self) -> Self::Validated {
        self.map(|v| v.assume_valid())
    }

    fn validate_deep(&self) -> anyhow::Result<()> {
        if let Some(inner) = self {
            inner.validate_deep()?;
        }
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.as_ref().map(|v| v.as_shortcut())
    }

    fn try_reify(&self) -> anyhow::Result<Self::Reified> {
        if let Some(inner) = self {
            Ok(Some(inner.try_reify()?))
        } else {
            Ok(None)
        }
    }
}

impl<V: ValidatedSchemaView<JsonSchema, ()>> ValidatedSchemaView<JsonSchema, ()> for Option<V> {
    type Reified = Option<V::Reified>;
    type Shortcut = Option<V::Shortcut>;
    type RawView = Option<V::RawView>;

    fn unwrap_validation(self) -> Self::RawView {
        self.map(|v| v.unwrap_validation())
    }

    fn as_shortcut_validated(&self) -> Self::Shortcut {
        self.as_ref().map(|v| v.as_shortcut_validated())
    }

    fn reify(&self) -> Self::Reified {
        self.as_ref().map(|v| v.reify())
    }
}

// Array
impl<T> DeserializeSchema<JsonSchema, ()> for Vec<T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Shortcut = JsonArray;
    type View<'a> = ArrayView<'a, T>;
    type ValidatedView<'a> = ValidatedArrayView<'a, T>;

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
        _args: (),
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

impl<'a, T> SchemaView<JsonSchema, ()> for ArrayView<'a, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Reified = Vec<T>;
    type Shortcut = JsonArray;
    type Validated = ValidatedArrayView<'a, T>;

    fn assume_valid(self) -> Self::Validated {
        ValidatedArrayView(self)
    }

    fn validate_deep(&self) -> anyhow::Result<()> {
        for elem in self.iter() {
            elem?.validate_deep()?;
        }
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.view.handle
    }

    fn try_reify(&self) -> anyhow::Result<Self::Reified> {
        let mut out = Vec::<T>::with_capacity(self.len() as usize);
        for elem in self.iter() {
            out.push(elem?.try_reify()?);
        }
        Ok(out)
    }
}

#[derive_where(Debug; T: DeserializeSchema<JsonSchema, ()>)]
#[derive_where(Copy, Clone)]
pub struct ValidatedArrayView<'a, T>(pub ArrayView<'a, T>);

impl<'a, T> ValidatedArrayView<'a, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    pub fn get(self, i: u32) -> Option<T::View<'a>> {
        self.0.get(i).map(|v| v.unwrap())
    }

    pub fn len(self) -> u32 {
        self.0.len()
    }

    pub fn iter(self) -> impl Iterator<Item = T::View<'a>> {
        self.0.iter().map(|v| v.unwrap())
    }
}

impl<'a, T> ValidatedSchemaView<JsonSchema, ()> for ValidatedArrayView<'a, T>
where
    T: DeserializeSchema<JsonSchema, ()>,
{
    type Reified = Vec<T>;
    type Shortcut = JsonArray;
    type RawView = ArrayView<'a, T>;

    fn unwrap_validation(self) -> Self::RawView {
        self.0
    }

    fn as_shortcut_validated(&self) -> Self::Shortcut {
        self.0.as_shortcut()
    }

    fn reify(&self) -> Self::Reified {
        self.0.try_reify().unwrap()
    }
}

// Number
macro_rules! impl_numerics {
    ($converter:ident; $($ty:ty),*$(,)?) => {$(
        impl DeserializeSchema<JsonSchema, ()> for $ty {
            type Shortcut = $ty;
            type View<'a> = $ty;
            type ValidatedView<'a> = $ty;

            fn make_shortcut(
                _document: &JsonDocument,
                object: Option<JsonValue>,
            ) -> anyhow::Result<Self::Shortcut> {
                match object {
                    Some(JsonValue::Number(number)) => Ok(<$ty>::try_from(number.$converter()?)?),
                    value @ _ => anyhow::bail!("Expected number, got {value:?}"),
                }
            }

            fn view_shortcut<'a>(
                _document: &'a <JsonSchema as SchemaDecodeCodec>::Document,
                shortcut: Self::Shortcut,
                _args: (),
            ) -> Self::View<'a> {
                shortcut
            }
        }

        impl SchemaView<JsonSchema, ()> for $ty {
            type Reified = $ty;
            type Shortcut = $ty;
            type Validated = $ty;

            fn assume_valid(self) -> Self::Validated {
                self
            }

            fn validate_deep(&self) -> anyhow::Result<()> {
                Ok(())
            }

            fn as_shortcut(&self) -> Self::Shortcut {
                *self
            }

            fn try_reify(&self) -> anyhow::Result<Self::Reified> {
                Ok(*self)
            }
        }

        impl ValidatedSchemaView<JsonSchema, ()> for $ty {
            type Reified = $ty;
            type Shortcut = $ty;
            type RawView = $ty;

            fn unwrap_validation(self) -> Self::RawView {
                self
            }

            fn as_shortcut_validated(&self) -> Self::Shortcut {
                *self
            }

            fn reify(&self) -> Self::Reified {
                *self
            }
        }
    )*};
}

impl_numerics!(as_uint; u8, u16, u32, u64);
impl_numerics!(as_int; i8, i16, i32, i64);
impl_numerics!(as_float; f64);

// Boolean
impl DeserializeSchema<JsonSchema, ()> for bool {
    type Shortcut = bool;
    type View<'a> = bool;
    type ValidatedView<'a> = bool;

    fn make_shortcut(
        _document: &JsonDocument,
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
    type Validated = Self;

    fn assume_valid(self) -> Self::Validated {
        self
    }

    fn validate_deep(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        *self
    }

    fn try_reify(&self) -> anyhow::Result<Self::Reified> {
        Ok(*self)
    }
}

impl ValidatedSchemaView<JsonSchema, ()> for bool {
    type Reified = bool;
    type Shortcut = bool;
    type RawView = Self;

    fn unwrap_validation(self) -> Self::RawView {
        self
    }

    fn as_shortcut_validated(&self) -> Self::Shortcut {
        *self
    }

    fn reify(&self) -> Self::Reified {
        *self
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
    type ValidatedView<'a> = StringView<'a>;

    fn make_shortcut(
        _document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        match object {
            Some(JsonValue::String(intern)) => Ok(intern),
            value @ _ => anyhow::bail!("Expected string, got {value:?}."),
        }
    }

    fn view_shortcut<'a>(
        document: &'a JsonDocument,
        shortcut: Intern,
        _args: (),
    ) -> Self::View<'a> {
        StringView {
            intern: shortcut,
            text: document.string_value(shortcut),
        }
    }
}

impl SchemaView<JsonSchema, ()> for StringView<'_> {
    type Reified = String;
    type Shortcut = Intern;
    type Validated = Self;

    fn assume_valid(self) -> Self::Validated {
        self
    }

    fn validate_deep(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.intern
    }

    fn try_reify(&self) -> anyhow::Result<Self::Reified> {
        Ok(self.text.to_string())
    }
}

impl ValidatedSchemaView<JsonSchema, ()> for StringView<'_> {
    type Reified = String;
    type Shortcut = Intern;
    type RawView = Self;

    fn unwrap_validation(self) -> Self::RawView {
        self
    }

    fn as_shortcut_validated(&self) -> Self::Shortcut {
        self.intern
    }

    fn reify(&self) -> Self::Reified {
        self.text.to_string()
    }
}

impl Deref for StringView<'_> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.text
    }
}

// === Schema Utilities === //

impl<L, R> DeserializeSchema<JsonSchema, ()> for Either<L, R>
where
    L: DeserializeSchema<JsonSchema, ()>,
    R: DeserializeSchema<JsonSchema, ()>,
{
    type Shortcut = Either<L::Shortcut, R::Shortcut>;
    type View<'a> = Either<L::View<'a>, R::View<'a>>;
    type ValidatedView<'a> = Either<L::ValidatedView<'a>, R::ValidatedView<'a>>;

    fn make_shortcut(
        document: &JsonDocument,
        object: Option<JsonValue>,
    ) -> anyhow::Result<Self::Shortcut> {
        let err_left = match L::make_shortcut(document, object) {
            Ok(shortcut) => return Ok(Either::Left(shortcut)),
            Err(err) => err,
        };

        let err_right = match R::make_shortcut(document, object) {
            Ok(shortcut) => return Ok(Either::Right(shortcut)),
            Err(err) => err,
        };

        anyhow::bail!("Failed to parse either left or right. Left error: {err_left:#?}. Right error: {err_right:#?}");
    }

    fn view_shortcut<'a>(
        document: &'a JsonDocument,
        shortcut: Self::Shortcut,
        _args: (),
    ) -> Self::View<'a> {
        match shortcut {
            Either::Left(left) => Either::Left(L::view_shortcut(document, left, ())),
            Either::Right(right) => Either::Right(R::view_shortcut(document, right, ())),
        }
    }
}

impl<L, R> SchemaView<JsonSchema, ()> for Either<L, R>
where
    L: SchemaView<JsonSchema, ()>,
    R: SchemaView<JsonSchema, ()>,
{
    type Reified = Either<L::Reified, R::Reified>;
    type Shortcut = Either<L::Shortcut, R::Shortcut>;
    type Validated = Either<L::Validated, R::Validated>;

    fn assume_valid(self) -> Self::Validated {
        self.map_either(SchemaView::assume_valid, SchemaView::assume_valid)
    }

    fn validate_deep(&self) -> anyhow::Result<()> {
        self.as_ref()
            .either(SchemaView::validate_deep, SchemaView::validate_deep)
    }

    fn as_shortcut(&self) -> Self::Shortcut {
        self.as_ref()
            .map_either(SchemaView::as_shortcut, SchemaView::as_shortcut)
    }

    fn try_reify(&self) -> anyhow::Result<Self::Reified> {
        match self {
            Either::Left(left) => Ok(Either::Left(left.try_reify()?)),
            Either::Right(right) => Ok(Either::Right(right.try_reify()?)),
        }
    }
}

impl<L, R> ValidatedSchemaView<JsonSchema, ()> for Either<L, R>
where
    L: ValidatedSchemaView<JsonSchema, ()>,
    R: ValidatedSchemaView<JsonSchema, ()>,
{
    type Reified = Either<L::Reified, R::Reified>;
    type Shortcut = Either<L::Shortcut, R::Shortcut>;
    type RawView = Either<L::RawView, R::RawView>;

    fn unwrap_validation(self) -> Self::RawView {
        self.map_either(
            ValidatedSchemaView::unwrap_validation,
            ValidatedSchemaView::unwrap_validation,
        )
    }

    fn as_shortcut_validated(&self) -> Self::Shortcut {
        self.as_ref().map_either(
            ValidatedSchemaView::as_shortcut_validated,
            ValidatedSchemaView::as_shortcut_validated,
        )
    }

    fn reify(&self) -> Self::Reified {
        self.as_ref()
            .map_either(ValidatedSchemaView::reify, ValidatedSchemaView::reify)
    }
}
