//! Typed builder parameters: one declaration generates the struct, its parser and its catalog rows.

use crate::json::Value;
use crate::model::{Params, fmt_num};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// What a field's catalog row and parser need beyond its type. `key` renames the JSON key (for Rust keywords); empty `key`/`def` mean unset, `also` lists keys that
/// share this row, `row: false` hides the row, `def` replaces the generated default text, `hint` extends the "is required" message
/// of a length.
#[derive(Clone, Copy)]
pub struct Spec {
    pub desc: &'static str,
    pub hint: &'static str,
    pub def: &'static str,
    pub key: &'static str,
    pub also: &'static [&'static str],
    pub row: bool,
}

impl Spec {
    pub const NONE: Spec = Spec { desc: "", hint: "", def: "", key: "", also: &[], row: true };
}

pub struct Row {
    pub names: Vec<&'static str>,
    pub def: String,
    pub desc: &'static str,
}

impl Row {
    pub fn name_text(&self) -> String {
        self.names.join(", ")
    }
}

pub trait ParamSet: Sized {
    fn parse_all(p: &mut Params<'_>) -> Self;
    fn rows() -> Vec<Row>;
    /// `rows().iter().any(|r| r.names.contains(&k))` without building the rows.
    fn has_key(k: &str) -> bool;
}

pub trait ParamEnum: Copy + Sized + 'static {
    const TAGS: &'static [&'static str];
    fn from_tag(s: &str) -> Option<Self>;
    fn tag(self) -> &'static str;
    fn first() -> Self;
}

pub trait ParamText {
    fn param_text(&self) -> String;
}

impl ParamText for bool {
    fn param_text(&self) -> String {
        self.to_string()
    }
}
impl ParamText for f64 {
    fn param_text(&self) -> String {
        fmt_num(*self)
    }
}
impl ParamText for &str {
    fn param_text(&self) -> String {
        (*self).to_string()
    }
}
impl<E: ParamEnum> ParamText for E {
    fn param_text(&self) -> String {
        self.tag().to_string()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Num {
    Plain,
    Len,
    LenPos,
}

pub fn read_bool(p: &mut Params<'_>, key: &str, default: bool) -> bool {
    p.boolean(key, default)
}

pub fn read_num(p: &mut Params<'_>, key: &str, kind: Num, default: Option<f64>, hint: &str) -> f64 {
    let v = match kind {
        Num::Plain => p.num(key, default),
        Num::Len => p.len(key, default, hint),
        Num::LenPos => p.len_pos(key, default, hint),
    };
    v.or(default).unwrap_or(0.0)
}

pub fn read_opt_num(p: &mut Params<'_>, key: &str, kind: Num, hint: &str) -> Option<f64> {
    if !p.has(key) {
        return None;
    }
    match kind {
        Num::Plain => p.num(key, None),
        Num::Len => p.len(key, None, hint),
        Num::LenPos => p.len_pos(key, None, hint),
    }
}

pub fn read_int(p: &mut Params<'_>, key: &str, default: Option<i64>, min: i64, max: i64) -> i64 {
    p.int(key, default, min, max).or(default).unwrap_or(0)
}

pub fn read_opt_int(p: &mut Params<'_>, key: &str, min: i64, max: i64) -> Option<i64> {
    if !p.has(key) {
        return None;
    }
    p.int(key, None, min, max)
}

pub fn read_str(p: &mut Params<'_>, key: &str, default: Option<&str>) -> String {
    if !p.has(key)
        && let Some(d) = default
    {
        return d.to_string();
    }
    p.str(key, None).map(String::from).or_else(|| default.map(String::from)).unwrap_or_default()
}

pub fn read_raw(p: &mut Params<'_>, key: &str, default: Option<Value>) -> Value {
    if let Some(v) = p.raw(key) {
        return v.clone();
    }
    if let Some(d) = default {
        return d;
    }
    let m = alloc::format!("param '{key}' is required for type {}", p.ty);
    p.fail(key, m);
    Value::Null
}

pub fn read_opt_bool(p: &mut Params<'_>, key: &str) -> Option<bool> {
    if !p.has(key) {
        return None;
    }
    Some(p.boolean(key, false))
}

pub fn read_opt_str(p: &mut Params<'_>, key: &str) -> Option<String> {
    if !p.has(key) {
        return None;
    }
    p.str(key, None).map(String::from)
}

pub fn read_opt_raw(p: &mut Params<'_>, key: &str) -> Option<Value> {
    p.raw(key).cloned()
}

pub fn read_enum<E: ParamEnum>(p: &mut Params<'_>, key: &str, default: Option<E>) -> E {
    let d = default.map(E::tag);
    let got = p.choice(key, d, E::TAGS).and_then(E::from_tag);
    got.or(default).unwrap_or_else(E::first)
}

pub fn read_opt_enum<E: ParamEnum>(p: &mut Params<'_>, key: &str) -> Option<E> {
    if !p.has(key) {
        return None;
    }
    p.choice(key, None, E::TAGS).and_then(E::from_tag)
}

/// Declares a closed set of string tags for an enum parameter.
///
/// ```ignore
/// param_enum! { pub enum Material { Osb = "osb", Plywood = "plywood" } }
/// ```
macro_rules! param_enum {
    ($(#[$m:meta])* $vis:vis enum $E:ident { $($V:ident = $tag:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        $vis enum $E { $($V),+ }
        impl $crate::params::ParamEnum for $E {
            const TAGS: &'static [&'static str] = &[$($tag),+];
            fn from_tag(s: &str) -> Option<Self> {
                match s { $($tag => Some(Self::$V),)+ _ => None }
            }
            fn tag(self) -> &'static str {
                match self { $(Self::$V => $tag),+ }
            }
            fn first() -> Self {
                [$(Self::$V),+][0]
            }
        }
    };
}

macro_rules! field_ty {
    (Bool) => { bool };
    (OptBool) => { Option<bool> };
    (Str) => { alloc::string::String };
    (OptStr) => { Option<alloc::string::String> };
    (Num) => { f64 };
    (Len) => { f64 };
    (LenPos) => { f64 };
    (OptNum) => { Option<f64> };
    (OptLen) => { Option<f64> };
    (OptLenPos) => { Option<f64> };
    (Int($t:ty, $min:expr, $max:expr)) => { $t };
    (OptInt($t:ty, $min:expr, $max:expr)) => { Option<$t> };
    (Raw) => { $crate::json::Value };
    (OptRaw) => { Option<$crate::json::Value> };
    (Enum($e:ty)) => { $e };
    (OptEnum($e:ty)) => { Option<$e> };
}

macro_rules! field_read {
    ($p:ident, $k:ident, $s:ident, Bool, [$($d:expr)?]) => { $crate::params::read_bool($p, $k, true $(&& $d)?) };
    ($p:ident, $k:ident, $s:ident, OptBool, []) => { $crate::params::read_opt_bool($p, $k) };
    ($p:ident, $k:ident, $s:ident, Str, [$($d:expr)?]) => { $crate::params::read_str($p, $k, None $(.or(Some($d)))?) };
    ($p:ident, $k:ident, $s:ident, OptStr, []) => { $crate::params::read_opt_str($p, $k) };
    ($p:ident, $k:ident, $s:ident, Num, [$($d:expr)?]) => { $crate::params::read_num($p, $k, $crate::params::Num::Plain, None $(.or(Some($d)))?, $s.hint) };
    ($p:ident, $k:ident, $s:ident, Len, [$($d:expr)?]) => { $crate::params::read_num($p, $k, $crate::params::Num::Len, None $(.or(Some($d)))?, $s.hint) };
    ($p:ident, $k:ident, $s:ident, LenPos, [$($d:expr)?]) => { $crate::params::read_num($p, $k, $crate::params::Num::LenPos, None $(.or(Some($d)))?, $s.hint) };
    ($p:ident, $k:ident, $s:ident, OptNum, []) => { $crate::params::read_opt_num($p, $k, $crate::params::Num::Plain, $s.hint) };
    ($p:ident, $k:ident, $s:ident, OptLen, []) => { $crate::params::read_opt_num($p, $k, $crate::params::Num::Len, $s.hint) };
    ($p:ident, $k:ident, $s:ident, OptLenPos, []) => { $crate::params::read_opt_num($p, $k, $crate::params::Num::LenPos, $s.hint) };
    ($p:ident, $k:ident, $s:ident, Int($t:ty, $min:expr, $max:expr), [$($d:expr)?]) => {
        <$t>::try_from($crate::params::read_int($p, $k, None $(.or(Some(($d as $t) as i64)))?, $min, $max)).unwrap_or_default()
    };
    ($p:ident, $k:ident, $s:ident, OptInt($t:ty, $min:expr, $max:expr), []) => {
        $crate::params::read_opt_int($p, $k, $min, $max).and_then(|v| <$t>::try_from(v).ok())
    };
    ($p:ident, $k:ident, $s:ident, Raw, [$($d:expr)?]) => { $crate::params::read_raw($p, $k, None $(.or(Some($d)))?) };
    ($p:ident, $k:ident, $s:ident, OptRaw, []) => { $crate::params::read_opt_raw($p, $k) };
    ($p:ident, $k:ident, $s:ident, Enum($e:ty), [$($d:expr)?]) => { $crate::params::read_enum::<$e>($p, $k, None $(.or(Some($d)))?) };
    ($p:ident, $k:ident, $s:ident, OptEnum($e:ty), []) => { $crate::params::read_opt_enum::<$e>($p, $k) };
}

macro_rules! field_def {
    (OptBool, []) => {
        "null".to_string()
    };
    (OptStr, []) => {
        "null".to_string()
    };
    (OptNum, []) => {
        "null".to_string()
    };
    (OptLen, []) => {
        "null".to_string()
    };
    (OptLenPos, []) => {
        "null".to_string()
    };
    (OptInt($($a:tt)*), []) => {
        "null".to_string()
    };
    (OptRaw, []) => {
        "null".to_string()
    };
    (Raw, [$d:expr]) => {
        alloc::string::String::new()
    };
    (OptEnum($($a:tt)*), []) => {
        "null".to_string()
    };
    (Int($t:ty, $min:expr, $max:expr), [$d:expr]) => {
        (($d as $t) as i64).to_string()
    };
    ($kind:ident $(($($a:tt)*))?, []) => {
        "required".to_string()
    };
    ($kind:ident $(($($a:tt)*))?, [$d:expr]) => {
        $crate::params::ParamText::param_text(&$d)
    };
}

/// Declares one builder's parameters; generates the struct, `ParamSet::parse_all` (every problem reported in one pass, in field
/// order) and `ParamSet::rows` (the catalog rows).
///
/// ```ignore
/// param_enum! { pub enum Run { X = "x", Y = "y" } }
///
/// params! {
///     pub struct Params {
///         material: Enum(Material) = Material::Osb => { desc: "osb | plywood" },
///         thickness: LenPos => { hint: "e.g. 0.4375", desc: "thickness" },
///         length: OptRaw => { def: "required", desc: "in-plane extent" },
///         plies: Int(u8, 1, 8) = 1 => { desc: "built-up members" },
///         width: OptLenPos => { also: &["height"], def: "rect: required", desc: "box size" },
///         height: OptLenPos => { row: false, desc: "box size" },
///         in_: OptRaw => { key: "in", desc: "host zone" },
///     }
/// }
/// ```
/// Kinds: `Bool Str Num Len LenPos Int(ty,min,max) Raw Enum(E)` (required unless `= default`) and `OptBool OptStr OptNum OptLen OptLenPos
/// OptInt(..) OptRaw OptEnum(E)` (`Option`, never required). `Len*` parse lengths ("1 1/2", 0.5); `Num` is a plain number.
macro_rules! params {
    (
        $(#[$m:meta])* $vis:vis struct $S:ident {
            $( $name:ident : $kind:ident $( ( $($ka:tt)* ) )? $( = $def:expr )? => { $($sk:ident : $sv:expr),* $(,)? } ),* $(,)?
        }
    ) => {
        $(#[$m])*
        // Every declared key is parsed (validated and documented in the catalog); a builder reads the ones it draws from.
        #[allow(dead_code)]
        #[derive(Clone, Debug)]
        $vis struct $S {
            $( pub $name: $crate::params::field_ty!($kind $( ( $($ka)* ) )?), )*
        }

        impl $crate::params::ParamSet for $S {
            fn parse_all(p: &mut $crate::model::Params<'_>) -> Self {
                Self {
                    $( $name: {
                        let s = $crate::params::Spec { $($sk: $sv,)* ..$crate::params::Spec::NONE };
                        let k = if s.key.is_empty() { stringify!($name) } else { s.key };
                        $crate::params::field_read!(p, k, s, $kind $( ( $($ka)* ) )?, [$($def)?])
                    }, )*
                }
            }

            fn rows() -> alloc::vec::Vec<$crate::params::Row> {
                #[allow(unused_imports)]
                use alloc::string::ToString;
                let mut out = alloc::vec::Vec::new();
                $( {
                    let s = $crate::params::Spec { $($sk: $sv,)* ..$crate::params::Spec::NONE };
                    if s.row {
                        let mut names = alloc::vec![if s.key.is_empty() { stringify!($name) } else { s.key }];
                        names.extend_from_slice(s.also);
                        let def = if s.def.is_empty() {
                            $crate::params::field_def!($kind $( ( $($ka)* ) )?, [$($def)?])
                        } else {
                            s.def.to_string()
                        };
                        out.push($crate::params::Row { names, def, desc: s.desc });
                    }
                } )*
                out
            }

            fn has_key(k: &str) -> bool {
                $( {
                    let s = $crate::params::Spec { $($sk: $sv,)* ..$crate::params::Spec::NONE };
                    if s.row && ((if s.key.is_empty() { stringify!($name) } else { s.key }) == k || s.also.contains(&k)) {
                        return true;
                    }
                } )*
                false
            }
        }
    };
}

pub(crate) use {field_def, field_read, field_ty, param_enum, params};
