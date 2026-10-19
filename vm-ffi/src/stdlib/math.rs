//! Math functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue};

/// Macro to create math FFI functions with automatic argument checking
macro_rules! math_function {
    ($name:ident, $arity:expr, $description:expr, |$args:ident| $body:expr) => {
        pub struct $name;

        impl FfiFunction for $name {
            fn call(&self, _ctx: &mut dyn crate::FfiContext, $args: Vec<FfiValue>) -> FfiResult {
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

            fn return_type(&self) -> crate::FfiType {
                crate::FfiType::Float
            }

            fn description(&self) -> Option<&str> {
                Some($description)
            }
        }
    };
}

#[inline(always)]
fn extract_number(val: &FfiValue) -> Option<f64> {
    match val {
        FfiValue::Float(f) => Some(*f),
        FfiValue::Integer(i) => Some(*i as f64),
        FfiValue::Number(n) => Some(*n),
        _ => None,
    }
}

// Math functions
math_function!(Sqrt, 1, "Square root of a number", |args| {
    match extract_number(&args[0]) {
        Some(n) => {
            if n < 0.0 {
                Err(FfiError::RuntimeError(
                    "Cannot take square root of negative number".to_string(),
                ))
            } else {
                Ok(FfiValue::Float(n.sqrt()))
            }
        }
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Abs, 1, "Absolute value of a number", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.abs())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Sin, 1, "Sine of a number (in radians)", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.sin())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Cos, 1, "Cosine of a number (in radians)", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.cos())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Floor, 1, "Floor of a number", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.floor())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Ceil, 1, "Ceiling of a number", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.ceil())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Round, 1, "Round a number to nearest integer", |args| {
    match extract_number(&args[0]) {
        Some(n) => Ok(FfiValue::Float(n.round())),
        None => Err(FfiError::ArgumentType {
            expected: "Number".to_string(),
            got: args[0].type_name().to_string(),
        }),
    }
});

math_function!(Max, 2, "Maximum of two numbers", |args| {
    match (extract_number(&args[0]), extract_number(&args[1])) {
        (Some(a), Some(b)) => Ok(FfiValue::Float(a.max(b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Min, 2, "Minimum of two numbers", |args| {
    match (extract_number(&args[0]), extract_number(&args[1])) {
        (Some(a), Some(b)) => Ok(FfiValue::Float(a.min(b))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Pow, 2, "Raise a number to a power", |args| {
    match (extract_number(&args[0]), extract_number(&args[1])) {
        (Some(base), Some(exp)) => Ok(FfiValue::Float(base.powf(exp))),
        _ => Err(FfiError::ArgumentType {
            expected: "Number, Number".to_string(),
            got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
        }),
    }
});

math_function!(Log, 1, "Natural logarithm of a number", |args| {
    match extract_number(&args[0]) {
        Some(n) => {
            if n <= 0.0 {
                Ok(FfiValue::Float(f64::NAN))
            } else {
                Ok(FfiValue::Float(n.ln()))
            }
        }
        None => Err(FfiError::ArgumentType {
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
