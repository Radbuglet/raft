use std::{error::Error, fmt};

#[derive(Copy, Clone)]
pub enum NeverError {}

impl fmt::Debug for NeverError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for NeverError {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            _ => unreachable!(),
        }
    }
}

impl Error for NeverError {}
