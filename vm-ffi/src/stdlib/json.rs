//! JSON parsing and serialization functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiType, FfiValue, RawFfiFunction};
use std::collections::HashMap;

macro_rules! json_function {
    ($name:ident, $arity:expr, $ret_type:expr, $description:expr, |$args:ident| $body:expr) => {
        pub struct $name;

        impl RawFfiFunction for $name {
            fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
                if args.len() != $arity {
                    return Err(FfiError::ArgumentCount {
                        expected: $arity,
                        got: args.len(),
                    });
                }
                let handler = |$args: Vec<FfiValue>| -> Result<FfiValue, FfiError> { $body };
                handler(args).map(Into::into)
            }

            fn name(&self) -> &str {
                stringify!($name)
            }

            fn arity(&self) -> usize {
                $arity
            }

            fn return_type(&self) -> FfiType {
                $ret_type
            }

            fn description(&self) -> Option<&str> {
                Some($description)
            }
        }

        impl FfiFunction for $name {}
    };
}

pub fn json_to_ffi(val: serde_json::Value) -> FfiValue {
    match val {
        serde_json::Value::Null => FfiValue::Unit,
        serde_json::Value::Bool(b) => FfiValue::Boolean(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                FfiValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                FfiValue::Float(f)
            } else {
                FfiValue::Number(0.0)
            }
        }
        serde_json::Value::String(s) => FfiValue::String(s),
        serde_json::Value::Array(arr) => {
            FfiValue::Array(arr.into_iter().map(json_to_ffi).collect())
        }
        serde_json::Value::Object(map) => {
            let mut obj = HashMap::new();
            for (k, v) in map {
                obj.insert(k, json_to_ffi(v));
            }
            FfiValue::Object(obj)
        }
    }
}

pub fn ffi_to_json(val: &FfiValue) -> serde_json::Value {
    match val {
        FfiValue::Integer(i) => serde_json::Value::Number((*i).into()),
        FfiValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        FfiValue::Number(n) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        FfiValue::Boolean(b) => serde_json::Value::Bool(*b),
        FfiValue::String(s) | FfiValue::Atom(s) => serde_json::Value::String(s.clone()),
        FfiValue::Unit | FfiValue::Undefined => serde_json::Value::Null,
        FfiValue::Array(arr) | FfiValue::Tuple(arr) => {
            serde_json::Value::Array(arr.iter().map(ffi_to_json).collect())
        }
        FfiValue::Object(map) => {
            let mut m = serde_json::Map::new();
            for (k, v) in map {
                m.insert(k.clone(), ffi_to_json(v));
            }
            serde_json::Value::Object(m)
        }
        _ => serde_json::Value::Null,
    }
}

json_function!(JsonParse, 1, FfiType::Object, "Parses JSON string into native object or array", |args| {
    let s = match &args[0] {
        FfiValue::String(s) => s,
        FfiValue::Atom(s) => s,
        other => return Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: other.type_name().to_string(),
        }),
    };

    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(json_val) => Ok(json_to_ffi(json_val)),
        Err(e) => Err(FfiError::RuntimeError(format!("JSON parse error: {}", e))),
    }
});

json_function!(JsonStringify, 1, FfiType::String, "Serializes value to JSON string", |args| {
    let json_val = ffi_to_json(&args[0]);
    match serde_json::to_string(&json_val) {
        Ok(s) => Ok(FfiValue::String(s)),
        Err(e) => Err(FfiError::RuntimeError(format!("JSON stringify error: {}", e))),
    }
});

json_function!(JsonGet, 2, FfiType::String, "Extracts a key from a JSON string without full decoding", |args| {
    let json_str = match &args[0] {
        FfiValue::String(s) => s,
        other => return Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: other.type_name().to_string(),
        }),
    };
    let key = match &args[1] {
        FfiValue::String(s) => s,
        other => return Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: other.type_name().to_string(),
        }),
    };

    if let Ok(mut current) = serde_json::from_str::<serde_json::Value>(json_str) {
        for part in key.split('.') {
            match current {
                serde_json::Value::Object(mut map) => {
                    if let Some(val) = map.remove(part) {
                        current = val;
                    } else {
                        return Ok(FfiValue::Unit);
                    }
                }
                _ => return Ok(FfiValue::Unit),
            }
        }
        return Ok(json_to_ffi(current));
    }
    Ok(FfiValue::Unit)
});

/// Register all JSON functions
pub fn register_json_functions(registry: &mut FfiRegistry) {
    registry.register(JsonParse);
    registry.register(JsonStringify);
    registry.register(JsonGet);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FfiReturn;

    struct MockContext;
    impl crate::FfiContext for MockContext {
        fn get_this(&self) -> Option<FfiValue> {
            None
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let mut reg = FfiRegistry::new();
        register_json_functions(&mut reg);
        let mut ctx = MockContext;

        let raw = r#"{"jsonrpc":"2.0","method":"initialize","id":1}"#;
        let parsed = reg.call(&mut ctx, "JsonParse", vec![FfiValue::String(raw.to_string())]).unwrap();

        let obj_val = match parsed {
            FfiReturn::Value(val) => val,
            _ => panic!("Expected Value"),
        };

        match &obj_val {
            FfiValue::Object(m) => {
                assert_eq!(m.get("jsonrpc"), Some(&FfiValue::String("2.0".to_string())));
                assert_eq!(m.get("method"), Some(&FfiValue::String("initialize".to_string())));
                assert_eq!(m.get("id"), Some(&FfiValue::Integer(1)));
            }
            _ => panic!("Expected Object"),
        }

        let stringified = reg.call(&mut ctx, "JsonStringify", vec![obj_val]).unwrap();
        match stringified {
            FfiReturn::Value(FfiValue::String(s)) => {
                assert!(s.contains("\"jsonrpc\":\"2.0\""));
            }
            _ => panic!("Expected String"),
        }
    }
}
