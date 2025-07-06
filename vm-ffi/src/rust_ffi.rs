//! Rust FFI implementation - allows calling Rust functions from the VM

use crate::{FfiFunction, FfiValue, FfiResult, FfiError, FfiRegistry};

/// Macro to create simple FFI functions with automatic argument checking
macro_rules! ffi_function {
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

// Math functions
ffi_function!(Sqrt, 1, "Square root of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => {
            if *n < 0.0 {
                Err(FfiError::RuntimeError("Cannot take square root of negative number".to_string()))
            } else {
                Ok(FfiValue::Number(n.sqrt()))
            }
        }
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Abs, 1, "Absolute value of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.abs())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Sin, 1, "Sine of a number (in radians)", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.sin())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Cos, 1, "Cosine of a number (in radians)", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.cos())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Floor, 1, "Floor of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.floor())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Ceil, 1, "Ceiling of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.ceil())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Round, 1, "Round a number to nearest integer", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.round())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(Max, 2, "Maximum of two numbers", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::Number(a), FfiValue::Number(b)) => Ok(FfiValue::Number(a.max(*b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

ffi_function!(Min, 2, "Minimum of two numbers", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::Number(a), FfiValue::Number(b)) => Ok(FfiValue::Number(a.min(*b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

// String functions
ffi_function!(StrLength, 1, "Length of a string", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::Number(s.len() as f64)),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(StrUpper, 1, "Convert string to uppercase", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::String(s.to_uppercase())),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(StrLower, 1, "Convert string to lowercase", |args| {
    match &args[0] {
        FfiValue::String(s) => Ok(FfiValue::String(s.to_lowercase())),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(StrConcat, 2, "Concatenate two strings", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::String(a), FfiValue::String(b)) => {
            Ok(FfiValue::String(format!("{}{}", a, b)))
        }
        _ => Err(FfiError::ArgumentType {
            expected: "String, String".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

// Array functions
ffi_function!(ArrayLength, 1, "Length of an array", |args| {
    match &args[0] {
        FfiValue::Array(arr) => Ok(FfiValue::Number(arr.len() as f64)),
        _ => Err(FfiError::ArgumentType {
            expected: "Array".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

ffi_function!(ArrayPush, 2, "Push element to end of array", |args| {
    match &args[0] {
        FfiValue::Array(arr) => {
            let mut new_arr = arr.clone();
            new_arr.push(args[1].clone());
            Ok(FfiValue::Array(new_arr))
        }
        _ => Err(FfiError::ArgumentType {
            expected: "Array".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

// I/O functions
ffi_function!(Print, 1, "Print a value to stdout", |args| {
    let output = match &args[0] {
        FfiValue::String(s) => s.clone(),
        FfiValue::Number(n) => n.to_string(),
        FfiValue::Boolean(b) => b.to_string(),
        FfiValue::Unit => "()".to_string(),
        FfiValue::Undefined => "undefined".to_string(),
        FfiValue::Array(arr) => format!("{:?}", arr),
        FfiValue::Object(obj) => format!("{:?}", obj),
    };
    println!("{}", output);
    Ok(FfiValue::Unit)
});

ffi_function!(PrintLn, 1, "Print a value to stdout with newline", |args| {
    let output = match &args[0] {
        FfiValue::String(s) => s.clone(),
        FfiValue::Number(n) => n.to_string(),
        FfiValue::Boolean(b) => b.to_string(),
        FfiValue::Unit => "()".to_string(),
        FfiValue::Undefined => "undefined".to_string(),
        FfiValue::Array(arr) => format!("{:?}", arr),
        FfiValue::Object(obj) => format!("{:?}", obj),
    };
    println!("{}", output);
    Ok(FfiValue::Unit)
});

// Type checking functions
ffi_function!(IsNumber, 1, "Check if value is a number", |args| {
    Ok(FfiValue::Boolean(matches!(args[0], FfiValue::Number(_))))
});

ffi_function!(IsString, 1, "Check if value is a string", |args| {
    Ok(FfiValue::Boolean(matches!(args[0], FfiValue::String(_))))
});

ffi_function!(IsBool, 1, "Check if value is a boolean", |args| {
    Ok(FfiValue::Boolean(matches!(args[0], FfiValue::Boolean(_))))
});

ffi_function!(IsArray, 1, "Check if value is an array", |args| {
    Ok(FfiValue::Boolean(matches!(args[0], FfiValue::Array(_))))
});

ffi_function!(TypeOf, 1, "Get the type name of a value", |args| {
    Ok(FfiValue::String(args[0].type_name().to_string()))
});

// Conversion functions
ffi_function!(ToString, 1, "Convert value to string", |args| {
    let result = match &args[0] {
        FfiValue::String(s) => s.clone(),
        FfiValue::Number(n) => n.to_string(),
        FfiValue::Boolean(b) => b.to_string(),
        FfiValue::Unit => "()".to_string(),
        FfiValue::Undefined => "undefined".to_string(),
        FfiValue::Array(_) => "[Array]".to_string(),
        FfiValue::Object(_) => "[Object]".to_string(),
    };
    Ok(FfiValue::String(result))
});

ffi_function!(ToNumber, 1, "Convert value to number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(*n)),
        FfiValue::Boolean(true) => Ok(FfiValue::Number(1.0)),
        FfiValue::Boolean(false) => Ok(FfiValue::Number(0.0)),
        FfiValue::String(s) => {
            match s.parse::<f64>() {
                Ok(n) => Ok(FfiValue::Number(n)),
                Err(_) => Ok(FfiValue::Number(f64::NAN)),
            }
        }
        _ => Ok(FfiValue::Number(f64::NAN)),
    }
});

/// Register all standard library functions with the registry
pub fn register_stdlib(registry: &mut FfiRegistry) {
    // Math functions
    registry.register(Sqrt);
    registry.register(Abs);
    registry.register(Sin);
    registry.register(Cos);
    registry.register(Floor);
    registry.register(Ceil);
    registry.register(Round);
    registry.register(Max);
    registry.register(Min);
    
    // String functions
    registry.register(StrLength);
    registry.register(StrUpper);
    registry.register(StrLower);
    registry.register(StrConcat);
    
    // Array functions
    registry.register(ArrayLength);
    registry.register(ArrayPush);
    
    // I/O functions
    registry.register(Print);
    registry.register(PrintLn);
    
    // Type checking functions
    registry.register(IsNumber);
    registry.register(IsString);
    registry.register(IsBool);
    registry.register(IsArray);
    registry.register(TypeOf);
    
    // Conversion functions
    registry.register(ToString);
    registry.register(ToNumber);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_math_functions() {
        let mut registry = FfiRegistry::new();
        register_stdlib(&mut registry);

        // Test sqrt
        let result = registry.call("Sqrt", vec![FfiValue::Number(16.0)]).unwrap();
        assert_eq!(result, FfiValue::Number(4.0));

        // Test sqrt with negative number
        let result = registry.call("Sqrt", vec![FfiValue::Number(-1.0)]);
        assert!(matches!(result, Err(FfiError::RuntimeError(_))));

        // Test abs
        let result = registry.call("Abs", vec![FfiValue::Number(-5.0)]).unwrap();
        assert_eq!(result, FfiValue::Number(5.0));

        // Test max
        let result = registry.call("Max", vec![FfiValue::Number(3.0), FfiValue::Number(7.0)]).unwrap();
        assert_eq!(result, FfiValue::Number(7.0));
    }

    #[test]
    fn test_string_functions() {
        let mut registry = FfiRegistry::new();
        register_stdlib(&mut registry);

        // Test string length
        let result = registry.call("StrLength", vec![FfiValue::String("hello".to_string())]).unwrap();
        assert_eq!(result, FfiValue::Number(5.0));

        // Test string concat
        let result = registry.call("StrConcat", vec![
            FfiValue::String("hello".to_string()),
            FfiValue::String(" world".to_string())
        ]).unwrap();
        assert_eq!(result, FfiValue::String("hello world".to_string()));

        // Test string upper
        let result = registry.call("StrUpper", vec![FfiValue::String("hello".to_string())]).unwrap();
        assert_eq!(result, FfiValue::String("HELLO".to_string()));
    }

    #[test]
    fn test_type_functions() {
        let mut registry = FfiRegistry::new();
        register_stdlib(&mut registry);

        // Test type checking
        let result = registry.call("IsNumber", vec![FfiValue::Number(42.0)]).unwrap();
        assert_eq!(result, FfiValue::Boolean(true));

        let result = registry.call("IsString", vec![FfiValue::Number(42.0)]).unwrap();
        assert_eq!(result, FfiValue::Boolean(false));

        // Test typeof
        let result = registry.call("TypeOf", vec![FfiValue::Boolean(true)]).unwrap();
        assert_eq!(result, FfiValue::String("Boolean".to_string()));
    }

    #[test]
    fn test_conversion_functions() {
        let mut registry = FfiRegistry::new();
        register_stdlib(&mut registry);

        // Test toString
        let result = registry.call("ToString", vec![FfiValue::Number(42.0)]).unwrap();
        assert_eq!(result, FfiValue::String("42".to_string()));

        // Test toNumber
        let result = registry.call("ToNumber", vec![FfiValue::String("123".to_string())]).unwrap();
        assert_eq!(result, FfiValue::Number(123.0));

        let result = registry.call("ToNumber", vec![FfiValue::Boolean(true)]).unwrap();
        assert_eq!(result, FfiValue::Number(1.0));
    }

    #[test]
    fn test_argument_validation() {
        let mut registry = FfiRegistry::new();
        register_stdlib(&mut registry);

        // Test wrong argument count
        let result = registry.call("Sqrt", vec![]);
        assert!(matches!(result, Err(FfiError::ArgumentCount { expected: 1, got: 0 })));

        // Test wrong argument type
        let result = registry.call("Sqrt", vec![FfiValue::String("hello".to_string())]);
        assert!(matches!(result, Err(FfiError::ArgumentType { .. })));
    }
}
