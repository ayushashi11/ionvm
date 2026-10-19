use crate::lang::types::Value;

pub fn and(left: &Value, right: &Value) -> Option<Value> {
    and_(left, right)
}

pub fn and_(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a && *b)),
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(*a & *b)),
        _ => None,
    }
}

pub fn or(left: &Value, right: &Value) -> Option<Value> {
    or_(left, right)
}

pub fn or_(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a || *b)),
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(*a | *b)),
        _ => None,
    }
}

pub fn xor(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a ^ *b)),
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(*a ^ *b)),
        _ => None,
    }
}

pub fn shl(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            let shift = (*b as u64) & 63;
            Some(Value::Int(a.wrapping_shl(shift as u32)))
        }
        _ => None,
    }
}

pub fn shr(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            let shift = (*b as u64) & 63;
            Some(Value::Int(a.wrapping_shr(shift as u32)))
        }
        _ => None,
    }
}

pub fn not(val: &Value) -> Option<Value> {
    match val {
        Value::Bool(a) => Some(Value::Bool(!*a)),
        Value::Int(a) => Some(Value::Int(!*a)),
        _ => None,
    }
}
