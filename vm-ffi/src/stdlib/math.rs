//! Math functions for the standard library

use crate::{FfiFunction, FfiValue, FfiResult, FfiError, FfiRegistry};

/// Macro to create math FFI functions with automatic argument checking
macro_rules! math_function {
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
math_function!(Sqrt, 1, "Square root of a number", |args| {
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

math_function!(Abs, 1, "Absolute value of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.abs())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Sin, 1, "Sine of a number (in radians)", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.sin())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Cos, 1, "Cosine of a number (in radians)", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.cos())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Floor, 1, "Floor of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.floor())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Ceil, 1, "Ceiling of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.ceil())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Round, 1, "Round a number to nearest integer", |args| {
    match &args[0] {
        FfiValue::Number(n) => Ok(FfiValue::Number(n.round())),
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Max, 2, "Maximum of two numbers", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::Number(a), FfiValue::Number(b)) => Ok(FfiValue::Number(a.max(*b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Min, 2, "Minimum of two numbers", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::Number(a), FfiValue::Number(b)) => Ok(FfiValue::Number(a.min(*b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Pow, 2, "Raise a number to a power", |args| {
    match (&args[0], &args[1]) {
        (FfiValue::Number(base), FfiValue::Number(exp)) => Ok(FfiValue::Number(base.powf(*exp))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Log, 1, "Natural logarithm of a number", |args| {
    match &args[0] {
        FfiValue::Number(n) => {
            if *n <= 0.0 {
                Ok(FfiValue::Number(f64::NAN))
            } else {
                Ok(FfiValue::Number(n.ln()))
            }
        }
        _ => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

/// Register all math functions with the registry
pub fn register_math_functions(registry: &mut FfiRegistry) {
    registry.register(Sqrt);
    registry.register(Abs);
    registry.register(Sin);
    registry.register(Cos);
    registry.register(Floor);
    registry.register(Ceil);
    registry.register(Round);
    registry.register(Max);
    registry.register(Min);
    registry.register(Pow);
    registry.register(Log);
}
