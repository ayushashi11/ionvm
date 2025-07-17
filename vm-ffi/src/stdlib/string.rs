//! String functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue};

/// Macro to create string FFI functions with automatic argument checking
macro_rules! string_function {
    ($name:ident, $arity:expr, $description:expr, |$args:ident| $body:expr) => {
        pub struct $name;

        impl FfiFunction for $name {
            fn call(&self, $args: Vec<FfiValue>) -> FfiResult {
                if $args.len() != $arity {
                    return Err(FfiError::ArgumentCount {
                        expected: $arity,
                        got: $args.len(),
                    });
                }
                $body
            }

            fn name(&self) -> &str {
                stringify!($name)
            }

            fn arity(&self) -> usize {
                $arity
            }

            fn description(&self) -> Option<&str> {
                Some($description)
            }
        }
    };
}

// String functions
string_function!(StrLength, 1, "Length of a string", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::Number(s.len() as f64)),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

string_function!(StrUpper, 1, "Convert string to uppercase", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::String(s.to_uppercase())),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

string_function!(StrLower, 1, "Convert string to lowercase", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::String(s.to_lowercase())),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

string_function!(StrConcat, 2, "Concatenate two strings", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::String(a), FfiValue::String(b)) => Ok(FfiValue::String(format!("{}{}", a, b))),
        _ => Err(FfiError::ArgumentType {
            expected: "String, String".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

string_function!(StrSplit, 2, "Split string by delimiter", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::String(s), FfiValue::String(delimiter)) => {
            let parts: Vec<FfiValue> = s
                .split(delimiter)
                .map(|part| FfiValue::String(part.to_string()))
                .collect();
            Ok(FfiValue::Array(parts))
        }
        _ => Err(FfiError::ArgumentType {
            expected: "String, String".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

string_function!(StrTrim, 1, "Trim whitespace from string", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::String(s.trim().to_string())),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

/// Register all string functions with the registry
pub fn register_string_functions(registry: &mut FfiRegistry) {
    registry.register(StrLength);
    registry.register(StrUpper);
    registry.register(StrLower);
    registry.register(StrConcat);
    registry.register(StrSplit);
    registry.register(StrTrim);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_functions() {
        let mut registry = FfiRegistry::new();
        register_string_functions(&mut registry);

        // Test string length
        let result = registry
            .call("StrLength", vec![FfiValue::String("hello".to_string())])
            .unwrap();
        assert_eq!(result, FfiValue::Number(5.0));

        // Test string concat
        let result = registry
            .call(
                "StrConcat",
                vec![
                    FfiValue::String("hello".to_string()),
                    FfiValue::String(" world".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(result, FfiValue::String("hello world".to_string()));

        // Test string upper
        let result = registry
            .call("StrUpper", vec![FfiValue::String("hello".to_string())])
            .unwrap();
        assert_eq!(result, FfiValue::String("HELLO".to_string()));

        // Test string split
        let result = registry
            .call(
                "StrSplit",
                vec![
                    FfiValue::String("a,b,c".to_string()),
                    FfiValue::String(",".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(
            result,
            FfiValue::Array(vec![
                FfiValue::String("a".to_string()),
                FfiValue::String("b".to_string()),
                FfiValue::String("c".to_string())
            ])
        );
    }
}
