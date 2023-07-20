use std::fmt;

// === Display Func === //

#[derive(Copy, Clone)]
pub struct DisplayFunc<F>(pub F);

impl<F> fmt::Display for DisplayFunc<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        (self.0)(f)
    }
}

pub fn display_func<F>(f: F) -> DisplayFunc<F>
where
    F: Fn(&mut fmt::Formatter) -> fmt::Result,
{
    DisplayFunc(f)
}

// === lazy_format! === //

#[doc(hidden)]
pub mod lazy_format_internals {
    pub use {super::display_func, std::write};
}

macro_rules! lazy_format {
	($($tt:tt)*) => {
		$crate::util::format::lazy_format_internals::display_func(move |f| {
			$crate::util::format::lazy_format_internals::write!(f, $($tt)*)
		})
	};
}

pub(crate) use lazy_format;
