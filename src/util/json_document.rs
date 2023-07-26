use hashbrown::HashMap;
use justjson::parser::{JsonKind, ParseDelegate, Parser};

use super::interner::{Intern, Interner};

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
    pub fn parse(text: &[u8]) -> anyhow::Result<Self> {
        // N.B. this check is necessary to allow us to use u32s everywhere.
        assert!(text.len() <= u32::MAX as usize);

        let mut delegate = JsonDocumentParser {
            interner: Interner::default(),
            map: HashMap::default(),
            gen: 0,
        };

        let root = Parser::parse_json_bytes(text, &mut delegate)?;

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
    String(&'a str),
    Number(JsonNumber),
    Boolean(bool),
    Null,
}

impl<'a> JsonValueView<'a> {
    pub fn wrap(doc: &'a JsonDocument, value: JsonValue) -> Self {
        match value {
            JsonValue::Object(handle) => Self::Object(JsonObjectView { doc, handle }),
            JsonValue::Array(handle) => Self::Array(JsonArrayView { doc, handle }),
            JsonValue::String(text) => Self::String(doc.string_value(text)),
            JsonValue::Number(number) => Self::Number(number),
            JsonValue::Boolean(bool) => Self::Boolean(bool),
            JsonValue::Null => Self::Null,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct JsonObjectView<'a> {
    pub doc: &'a JsonDocument,
    pub handle: JsonObject,
}

impl<'a> JsonObjectView<'a> {
    pub fn get(self, key: &str) -> Option<JsonValueView<'a>> {
        self.doc
            .object_field(self.handle, key)
            .map(|handle| JsonValueView::wrap(self.doc, handle))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct JsonArrayView<'a> {
    pub doc: &'a JsonDocument,
    pub handle: JsonArray,
}

impl<'a> JsonArrayView<'a> {
    pub fn get(self, idx: u32) -> Option<JsonValueView<'a>> {
        self.doc
            .array_element(self.handle, idx)
            .map(|handle| JsonValueView::wrap(self.doc, handle))
    }

    pub fn len(self) -> u32 {
        self.handle.len()
    }

    pub fn iter(self) -> impl Iterator<Item = JsonValueView<'a>> + 'a {
        (0..self.len()).map_while(move |i| self.get(i))
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
