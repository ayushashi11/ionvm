use crate::lang::types::Value;

pub fn add(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(a.wrapping_add(*b))),
        (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 + b)),
        (Value::Float(a), Value::Int(b)) => Some(Value::Float(a + *b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(Value::Float(a + b)),
        (Value::String(a), Value::String(b)) => {
            let mut s = String::with_capacity(a.len() + b.len());
            s.push_str(a);
            s.push_str(b);
            Some(Value::String(s))
        }
        _ => None,
    }
}

pub fn sub(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(a.wrapping_sub(*b))),
        (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 - b)),
        (Value::Float(a), Value::Int(b)) => Some(Value::Float(a - *b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(Value::Float(a - b)),
        _ => None,
    }
}

pub fn mul(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Some(Value::Int(a.wrapping_mul(*b))),
        (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 * b)),
        (Value::Float(a), Value::Int(b)) => Some(Value::Float(a * *b as f64)),
        (Value::Float(a), Value::Float(b)) => Some(Value::Float(a * b)),
        (Value::String(s), Value::Int(n)) if *n >= 0 => {
            let repeated = s.repeat(*n as usize);
            Some(Value::String(repeated))
        }
        _ => None,
    }
}

pub fn div(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                None
            } else {
                Some(Value::Int(a.wrapping_div(*b)))
            }
        }
        (Value::Int(a), Value::Float(b)) => {
            if *b == 0.0 {
                None
            } else {
                Some(Value::Float(*a as f64 / b))
            }
        }
        (Value::Float(a), Value::Int(b)) => {
            if *b == 0 {
                None
            } else {
                Some(Value::Float(a / *b as f64))
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            if *b == 0.0 {
                None
            } else {
                Some(Value::Float(a / b))
            }
        }
        _ => None,
    }
}

pub fn rem(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                None
            } else {
                Some(Value::Int(a.wrapping_rem(*b)))
            }
        }
        (Value::Float(a), Value::Float(b)) => {
            if *b == 0.0 {
                None
            } else {
                Some(Value::Float(a % b))
            }
        }
        _ => None,
    }
}

pub fn neg(val: &Value) -> Option<Value> {
    match val {
        Value::Int(a) => Some(Value::Int(-a)),
        Value::Float(a) => Some(Value::Float(-a)),
        _ => None,
    }
}
