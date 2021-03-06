//! Root module of the rush library crate.

#![allow(unknown_lints)]  // for Clippy

// NOTE: `nom` has to be declared before `log` because both define an error!
// macro, and we want to use the one from `log`.
#[macro_use] extern crate nom;
#[macro_use] extern crate log;

             extern crate conv;
             extern crate csv;
             extern crate fnv;
#[macro_use] extern crate lazy_static;
#[macro_use] extern crate mopa;
             extern crate rand;
             extern crate regex;
             extern crate rustc_serialize;
             extern crate unicode_categories;
             extern crate unicode_normalization;
             extern crate unicode_segmentation;
             extern crate unidecode;


mod eval;
mod parse;
mod wrappers;


pub use self::eval::*;
pub use self::parse::parse;
pub use self::wrappers::*;
