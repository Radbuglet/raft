use std::fmt;

#[derive(Copy, Clone)]
pub struct FormatterFn<F>(pub F);

impl<F> fmt::Display for FormatterFn<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.0)(f)
    }
}

pub fn format_closure<F>(f: F) -> FormatterFn<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    FormatterFn(f)
}

#[doc(hidden)]
pub mod lazy_format_internals {
    pub use {
        super::format_closure,
        std::{fmt::Formatter, write},
    };
}

macro_rules! lazy_format {
	($($tt:tt)*) => {
		$crate::util::format::lazy_format_internals::format_closure(move |f: &mut $crate::util::format::lazy_format_internals::Formatter| {
			$crate::util::format::lazy_format_internals::write!(f, $($tt)*)
		})
	};
}

pub(crate) use lazy_format;

#[derive(Debug)]
pub struct FmtRepeat<T>(pub T, pub usize);

impl<T: fmt::Display> fmt::Display for FmtRepeat<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&FmtIter::new((0..self.1).map(|_| &self.0)), f)
    }
}

#[derive(Debug)]
pub struct FmtIter<I>(pub I);

impl<I> FmtIter<I> {
    pub fn new(iter: impl IntoIterator<IntoIter = I>) -> Self {
        Self(iter.into_iter())
    }
}

impl<I> fmt::Display for FmtIter<I>
where
    I: Clone + Iterator,
    I::Item: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in self.0.clone() {
            fmt::Display::fmt(&i, f)?;
        }
        Ok(())
    }
}
