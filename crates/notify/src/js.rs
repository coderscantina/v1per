//! A JSON value that behaves like a JavaScript object inside Liquid templates.
//!
//! Kuma renders with liquidjs, where a missing key or a key on `null` yields `undefined` and
//! prints nothing. Rust's `liquid` errors on both. `Js` answers every key lookup (with `Null`
//! when absent) and prints values the way JavaScript's `String()` does. Objects keep Kuma's key
//! order, so `json` output and webhook bodies match `JSON.stringify`.

// liquid's view traits are written against kstring's deprecated `KStringCow`.
#![allow(deprecated)]

use std::fmt;

use liquid_core::model::{
    ArrayView, DisplayCow, KStringCow, ObjectView, ScalarCow, State, Value, ValueView,
};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Serialize, Serializer};

#[derive(Clone, Debug, PartialEq)]
pub enum Js {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<Js>),
    Object(Vec<(String, Js)>),
}

pub(crate) static NULL: Js = Js::Null;

impl Js {
    pub fn object<K: Into<String>>(entries: impl IntoIterator<Item = (K, Js)>) -> Self {
        Self::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    pub fn get(&self, key: &str) -> &Js {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find_map(|(k, v)| (k == key).then_some(v))
                .unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// JavaScript truthiness.
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Int(i) => *i != 0,
            Self::Str(s) => !s.is_empty(),
            Self::Array(_) | Self::Object(_) => true,
        }
    }

    /// Applies `f` to every string, recursively.
    pub fn map_strings(&self, f: &impl Fn(&str) -> String) -> Js {
        match self {
            Self::Str(s) => Self::Str(f(s)),
            Self::Array(items) => Self::Array(items.iter().map(|v| v.map_strings(f)).collect()),
            Self::Object(entries) => Self::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.map_strings(f)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

impl From<&str> for Js {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}

impl From<String> for Js {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

impl From<bool> for Js {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<i64> for Js {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl<T: Into<Js>> From<Option<T>> for Js {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

impl<T: Into<Js>> From<Vec<T>> for Js {
    fn from(items: Vec<T>) -> Self {
        Self::Array(items.into_iter().map(Into::into).collect())
    }
}

/// JavaScript `String(value)`.
impl fmt::Display for Js {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Null => Ok(()),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Int(i) => write!(f, "{i}"),
            Self::Str(s) => f.write_str(s),
            // liquidjs joins array items without a separator.
            Self::Array(items) => items.iter().try_for_each(|item| write!(f, "{item}")),
            Self::Object(_) => f.write_str("[object Object]"),
        }
    }
}

impl Serialize for Js {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(b) => serializer.serialize_bool(*b),
            Self::Int(i) => serializer.serialize_i64(*i),
            Self::Str(s) => serializer.serialize_str(s),
            Self::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                items
                    .iter()
                    .try_for_each(|item| seq.serialize_element(item))?;
                seq.end()
            }
            Self::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                entries
                    .iter()
                    .try_for_each(|(k, v)| map.serialize_entry(k, v))?;
                map.end()
            }
        }
    }
}

impl ValueView for Js {
    fn as_debug(&self) -> &dyn fmt::Debug {
        self
    }

    fn render(&self) -> DisplayCow<'_> {
        DisplayCow::Borrowed(self)
    }

    fn source(&self) -> DisplayCow<'_> {
        DisplayCow::Owned(Box::new(serde_json::to_string(self).unwrap_or_default()))
    }

    fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "nil",
            Self::Bool(_) => "boolean",
            Self::Int(_) => "whole number",
            Self::Str(_) => "string",
            Self::Array(_) => "array",
            Self::Object(_) => "object",
        }
    }

    /// liquidjs semantics: only `false`, `null` and `undefined` are falsy; `default` also
    /// replaces empty strings and arrays.
    fn query_state(&self, state: State) -> bool {
        let empty = match self {
            Self::Null => true,
            Self::Str(s) => s.is_empty(),
            Self::Array(items) => items.is_empty(),
            Self::Object(entries) => entries.is_empty(),
            Self::Bool(_) | Self::Int(_) => false,
        };
        match state {
            State::Truthy => !matches!(self, Self::Null | Self::Bool(false)),
            State::DefaultValue => match self {
                Self::Null | Self::Bool(false) => true,
                Self::Str(_) | Self::Array(_) => empty,
                _ => false,
            },
            State::Empty => empty,
            State::Blank => match self {
                Self::Bool(b) => !b,
                Self::Str(s) => s.trim().is_empty(),
                _ => empty,
            },
        }
    }

    fn to_kstr(&self) -> KStringCow<'_> {
        match self {
            Self::Str(s) => KStringCow::from_ref(s),
            other => KStringCow::from_string(other.to_string()),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Null => Value::Nil,
            Self::Bool(b) => Value::scalar(*b),
            Self::Int(i) => Value::scalar(*i),
            Self::Str(s) => Value::scalar(s.clone()),
            Self::Array(items) => Value::Array(items.iter().map(Js::to_value).collect()),
            Self::Object(entries) => Value::Object(
                entries
                    .iter()
                    .map(|(k, v)| (k.clone().into(), v.to_value()))
                    .collect(),
            ),
        }
    }

    fn as_scalar(&self) -> Option<ScalarCow<'_>> {
        match self {
            Self::Bool(b) => Some(ScalarCow::new(*b)),
            Self::Int(i) => Some(ScalarCow::new(*i)),
            Self::Str(s) => Some(ScalarCow::new(s.as_str())),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&dyn ArrayView> {
        matches!(self, Self::Array(_)).then_some(self)
    }

    /// `Null` also acts as an object, so `monitorJSON.name` on a null monitor is `Null`.
    fn as_object(&self) -> Option<&dyn ObjectView> {
        matches!(self, Self::Object(_) | Self::Null).then_some(self)
    }

    fn is_nil(&self) -> bool {
        self.is_null()
    }
}

impl ArrayView for Js {
    fn as_value(&self) -> &dyn ValueView {
        self
    }

    fn size(&self) -> i64 {
        match self {
            Self::Array(items) => items.len() as i64,
            _ => 0,
        }
    }

    fn values<'k>(&'k self) -> Box<dyn Iterator<Item = &'k dyn ValueView> + 'k> {
        match self {
            Self::Array(items) => Box::new(items.iter().map(|v| v as &dyn ValueView)),
            _ => Box::new(std::iter::empty()),
        }
    }

    fn contains_key(&self, _index: i64) -> bool {
        true
    }

    fn get(&self, index: i64) -> Option<&dyn ValueView> {
        let Self::Array(items) = self else {
            return Some(&NULL);
        };
        let index = if index < 0 {
            items.len() as i64 + index
        } else {
            index
        };
        let item = usize::try_from(index)
            .ok()
            .and_then(|i| items.as_slice().get(i));
        Some(item.unwrap_or(&NULL))
    }
}

impl ObjectView for Js {
    fn as_value(&self) -> &dyn ValueView {
        self
    }

    fn size(&self) -> i64 {
        match self {
            Self::Object(entries) => entries.len() as i64,
            _ => 0,
        }
    }

    fn keys<'k>(&'k self) -> Box<dyn Iterator<Item = KStringCow<'k>> + 'k> {
        Box::new(self.entries().map(|(k, _)| KStringCow::from_ref(k)))
    }

    fn values<'k>(&'k self) -> Box<dyn Iterator<Item = &'k dyn ValueView> + 'k> {
        Box::new(self.entries().map(|(_, v)| v as &dyn ValueView))
    }

    fn iter<'k>(&'k self) -> Box<dyn Iterator<Item = (KStringCow<'k>, &'k dyn ValueView)> + 'k> {
        Box::new(
            self.entries()
                .map(|(k, v)| (KStringCow::from_ref(k), v as &dyn ValueView)),
        )
    }

    /// Every key exists; absent ones read as `Null`, like `undefined` in liquidjs.
    fn contains_key(&self, _index: &str) -> bool {
        true
    }

    fn get<'s>(&'s self, index: &str) -> Option<&'s dyn ValueView> {
        let value = Js::get(self, index);
        // `size` on objects is liquid's computed size unless the object has that key.
        if value.is_null() && index == "size" && matches!(self, Self::Object(_)) {
            return None;
        }
        Some(value)
    }
}

impl Js {
    fn entries(&self) -> impl Iterator<Item = &(String, Js)> {
        match self {
            Self::Object(entries) => entries.iter(),
            _ => [].iter(),
        }
    }
}
