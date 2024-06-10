use std::borrow::Borrow;

use crate::{fixed::Fixed, sys, sys_bool, SaneStr, SaneString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    Bool,
    Int,
    Fixed,
    String,
    Group,
    Button,
    Unknown,
}

impl ValueType {
    pub const fn is_word_sized(&self) -> bool {
        matches!(self, Self::Bool | Self::Int | Self::Fixed)
    }

    pub const fn is_value(&self) -> bool {
        matches!(self, Self::Bool | Self::Int | Self::Fixed | Self::String)
    }
}

impl From<sys::ValueType> for ValueType {
    fn from(value: sys::ValueType) -> Self {
        match value {
            sys::ValueType::Bool => Self::Bool,
            sys::ValueType::Int => Self::Int,
            sys::ValueType::Fixed => Self::Fixed,
            sys::ValueType::String => Self::String,
            sys::ValueType::Group => Self::Group,
            sys::ValueType::Button => Self::Button,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Bool(bool),
    Int(i32),
    Fixed(Fixed),
    String(&'a SaneStr),
}

impl Value<'_> {
    pub const fn type_of(&self) -> ValueType {
        match self {
            Self::Bool(..) => ValueType::Bool,
            Self::Int(..) => ValueType::Int,
            Self::Fixed(..) => ValueType::Fixed,
            Self::String(..) => ValueType::String,
        }
    }

    pub const fn to_word(&self) -> Option<sys::Word> {
        match *self {
            Self::Bool(v) => Some(sys_bool(v)),
            Self::Int(v) => Some(v),
            Self::Fixed(v) => Some(v.to_bits()),
            _ => None,
        }
    }

    pub const fn from_word(word: sys::Word, ty: ValueType) -> Option<Self> {
        match ty {
            ValueType::Bool => Some(Self::Bool(word != sys::FALSE as sys::Word)),
            ValueType::Int => Some(Self::Int(word)),
            ValueType::Fixed => Some(Self::Fixed(Fixed::from_bits(word))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnedValue {
    Bool(bool),
    Int(i32),
    Fixed(Fixed),
    String(SaneString),
}

impl OwnedValue {
    pub const fn type_of(&self) -> ValueType {
        match self {
            Self::Bool(..) => ValueType::Bool,
            Self::Int(..) => ValueType::Int,
            Self::Fixed(..) => ValueType::Fixed,
            Self::String(..) => ValueType::String,
        }
    }

    pub fn as_ref(&self) -> Value {
        match self {
            Self::Bool(v) => Value::Bool(*v),
            Self::Int(v) => Value::Int(*v),
            Self::Fixed(v) => Value::Fixed(*v),
            Self::String(v) => Value::String(v.borrow()),
        }
    }

    pub const fn to_word(&self) -> Option<sys::Word> {
        match *self {
            Self::Bool(v) => Some(sys_bool(v)),
            Self::Int(v) => Some(v),
            Self::Fixed(v) => Some(v.to_bits()),
            _ => None,
        }
    }

    pub const fn from_word(word: sys::Word, ty: ValueType) -> Option<Self> {
        match ty {
            ValueType::Bool => Some(Self::Bool(word != sys::FALSE as sys::Word)),
            ValueType::Int => Some(Self::Int(word)),
            ValueType::Fixed => Some(Self::Fixed(Fixed::from_bits(word))),
            _ => None,
        }
    }
}
