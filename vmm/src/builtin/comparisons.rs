use crate::lang::types::Value;

pub fn equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Int(a), Value::Float(b)) => *a as f64 == *b,
        (Value::Float(a), Value::Int(b)) => *a == *b as f64,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Nil, Value::Nil) => true,
        _ => false,
    }
}

pub fn less_than(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a < b),
        (Value::Int(a), Value::Float(b)) => Some((*a as f64) < *b),
        (Value::Float(a), Value::Int(b)) => Some(*a < (*b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(a < b),
        _ => None,
    }
}

pub fn greater_than(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a > b),
        (Value::Int(a), Value::Float(b)) => Some((*a as f64) > *b),
        (Value::Float(a), Value::Int(b)) => Some(*a > (*b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(a > b),
        _ => None,
    }
}

pub fn less_equal(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a <= b),
        (Value::Int(a), Value::Float(b)) => Some((*a as f64) <= *b),
        (Value::Float(a), Value::Int(b)) => Some(*a <= (*b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(a <= b),
        _ => None,
    }
}

pub fn greater_equal(left: &Value, right: &Value) -> Option<bool> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(a >= b),
        (Value::Int(a), Value::Float(b)) => Some((*a as f64) >= *b),
        (Value::Float(a), Value::Int(b)) => Some(*a >= (*b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(a >= b),
        _ => None,
    }
}

pub use less_equal as less_than_or_equal;
pub use greater_equal as greater_than_or_equal;

