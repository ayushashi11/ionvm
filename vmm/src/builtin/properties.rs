use std::sync::{Arc, RwLock};
use crate::lang::types::{Function, FunctionType, Protection, Value};

fn native_bound_func(name: &str, arg_count: usize, this_val: &Value) -> Value {
    let mut f = Function::new(name.to_string(), Vec::new(), 0, arg_count, FunctionType::Native);
    f.bound_this = Some(this_val.clone());
    Value::Function(Arc::new(RwLock::new(f)))
}

fn bytecode_bound_func(name: &str, this_val: &Value) -> Value {
    let mut f = Function::new(name.to_string(), Vec::new(), 0, 0, FunctionType::Regular);
    f.bound_this = Some(this_val.clone());
    Value::Function(Arc::new(RwLock::new(f)))
}

pub fn get_property(val: &Value, key: &str, _is_this: bool) -> Option<Value> {
    match val {
        Value::Object(obj) => {
            if let Some(prop_val) = obj.read().unwrap().get_property(key) {
                if let Value::Function(ref func) = prop_val {
                    let mut cloned = func.read().unwrap().clone();
                    cloned.bound_this = Some(val.clone());
                    Some(Value::Function(Arc::new(RwLock::new(cloned))))
                } else {
                    Some(prop_val)
                }
            } else {
                None
            }
        }
        Value::String(s) => match key {
            "length" => Some(Value::Int(s.len() as i64)),
            "trim" => Some(native_bound_func("str_trim", 0, val)),
            "trim_start" | "lstrip" => Some(native_bound_func("str_trim_start", 0, val)),
            "trim_end" | "rstrip" => Some(native_bound_func("str_trim_end", 0, val)),
            "replace" => Some(native_bound_func("str_replace", 2, val)),
            "split" => Some(native_bound_func("str_split", 1, val)),
            "slice" => Some(native_bound_func("str_slice", 2, val)),
            "starts_with" | "startswith" => Some(native_bound_func("str_starts_with", 1, val)),
            "ends_with" | "endswith" => Some(native_bound_func("str_ends_with", 1, val)),
            "index_of" => Some(native_bound_func("str_index_of", 1, val)),
            "repeat" => Some(native_bound_func("str_repeat", 1, val)),
            "to_int" => Some(native_bound_func("str_to_int", 0, val)),
            "char_at" => Some(native_bound_func("str_char_at", 1, val)),
            "to_string" | "__ionvm.prelude.String.#ToString.to_string" => {
                Some(bytecode_bound_func("__ionvm.prelude.String.#ToString.to_string", val))
            }
            _ => None,
        },
        Value::Float(f) => match key {
            "is_nan" => Some(Value::Bool(f.is_nan())),
            "is_infinite" => Some(Value::Bool(f.is_infinite())),
            "to_string" | "__ionvm.prelude.Float.#ToString.to_string" => {
                Some(bytecode_bound_func("__ionvm.prelude.Float.#ToString.to_string", val))
            }
            _ => None,
        },
        Value::Int(_) if key == "to_string" || key == "__ionvm.prelude.Int.#ToString.to_string" => {
            Some(bytecode_bound_func("__ionvm.prelude.Int.#ToString.to_string", val))
        }
        Value::Bool(_) if key == "to_string" || key == "__ionvm.prelude.Bool.#ToString.to_string" => {
            Some(bytecode_bound_func("__ionvm.prelude.Bool.#ToString.to_string", val))
        }
        Value::Array(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            "push" => Some(native_bound_func("push", 1, val)),
            "pop" => Some(native_bound_func("pop", 0, val)),
            "get" => Some(native_bound_func("get", 1, val)),
            _ => None,
        },
        Value::IntArray(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            "pop" => Some(native_bound_func("pop", 0, val)),
            "get" => Some(native_bound_func("get", 1, val)),
            _ => None,
        },
        Value::FloatArray(arr) => match key {
            "length" => Some(Value::Int(arr.read().unwrap().len() as i64)),
            "pop" => Some(native_bound_func("pop", 0, val)),
            "get" => Some(native_bound_func("get", 1, val)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_to_string_properties_resolve_to_prelude_bytecode() {
        for (value, expected_name) in [
            (Value::Int(1), "__ionvm.prelude.Int.#ToString.to_string"),
            (Value::Float(1.0), "__ionvm.prelude.Float.#ToString.to_string"),
            (Value::Bool(true), "__ionvm.prelude.Bool.#ToString.to_string"),
            (Value::String("value".into()), "__ionvm.prelude.String.#ToString.to_string"),
        ] {
            let Some(Value::Function(function)) = get_property(&value, "to_string", false) else {
                panic!("to_string property must resolve to a function");
            };
            let function = function.read().unwrap();
            assert_eq!(function.name, expected_name);
            assert_eq!(function.fn_type, FunctionType::Regular);
            assert_eq!(function.bound_this.as_ref(), Some(&value));
        }
    }
}
