use std::fmt;

use super::core::Codec;

// === Codec Traits === //

pub trait SchemaDecodeCodec: Codec {
    type Document: SchemaDocument<AnyRef = Self::DocumentRef>;
    type DocumentRef: 'static + fmt::Debug + Clone;
}

pub trait SchemaDocument: Sized + 'static {
    type AnyRef: 'static + fmt::Debug + Clone;

    fn root(&self) -> Self::AnyRef;
}

// === Deserialize Traits === //

pub trait DeserializeSchema<C: SchemaDecodeCodec, A>: Sized + 'static {
    type Shortcut: 'static + fmt::Debug + Clone;
    type View<'a>: SchemaView<C, A, Reified = Self>;

    fn make_shortcut(
        document: &C::Document,
        object: Option<C::DocumentRef>,
    ) -> anyhow::Result<Self::Shortcut>;

    fn view_shortcut<'a>(
        document: &'a C::Document,
        shortcut: Self::Shortcut,
        args: A,
    ) -> Self::View<'a>;

    fn view_object<'a>(
        document: &'a C::Document,
        object: Option<C::DocumentRef>,
        args: A,
    ) -> anyhow::Result<Self::View<'a>> {
        Self::make_shortcut(document, object)
            .map(|shortcut| Self::view_shortcut(document, shortcut, args))
    }
}

pub trait SchemaView<C: SchemaDecodeCodec, A>: fmt::Debug {
    type Reified: DeserializeSchema<C, A, Shortcut = Self::Shortcut>;
    type Shortcut: 'static + fmt::Debug + Clone;

    fn validate_deep(&self) -> anyhow::Result<()>;

    fn as_shortcut(&self) -> Self::Shortcut;

    fn reify(&self) -> anyhow::Result<Self::Reified>;
}

// === Derivation Macro === //

// TODO
