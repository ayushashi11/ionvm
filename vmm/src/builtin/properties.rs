use crate::lang::types::{Protection, Value};

pub fn get_property(val: &Value, key: &str, _is_this: bool) -> Option<Value> {
    match val {
        Value::Object(obj) => obj.read().unwrap().get_property(key),
        Value::String(s) => match key {
            "length" => Some(Value::Int(s.len() as i64)),
            _ => None,
        },
        Value::Float(f) => match key {
            "is_nan" => Some(Value::Bool(f.is_nan())),
            "is_infinite" => Some(Value::Bool(f.is_infinite())),
            _ => None,
        },
        Value::Array(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            _ => None,
        },
        Value::IntArray(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            _ => None,
        },
        Value::FloatArray(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            _ => None,
        },
        _ => None,
    }
}

pub fn set_property(val: &Value, key: &str, new_val: Value, _is_this: bool) -> Result<(), &'static str> {
    match val {
        Value::Object(obj) => {
            obj.write().unwrap().set_property(key, new_val, Protection::Writeable);
            Ok(())
        }
        _ => Err("can only set properties on objects"),
    }
}
