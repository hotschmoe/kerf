//! Order-preserving JSON value tree. (The parser and writer land here too.)

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub key: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Vec<Member>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.iter().find(|m| m.key == key).map(|m| &m.value),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        match self {
            Value::Object(m) => m.iter_mut().find(|m| m.key == key).map(|m| &mut m.value),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        if let Value::String(s) = self { Some(s) } else { None }
    }
    pub fn num(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
    pub fn arr(&self) -> Option<&[Value]> {
        if let Value::Array(a) = self { Some(a) } else { None }
    }
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
    /// The type name used in error messages.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}
