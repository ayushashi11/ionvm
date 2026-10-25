//! String manipulation functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiType, FfiValue, RawFfiFunction};

macro_rules! string_function {
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

fn extract_string(val: &FfiValue) -> Result<String, FfiError> {
    match val {
        FfiValue::String(s) => Ok(s.clone()),
        FfiValue::Atom(s) => Ok(s.clone()),
        _ => Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: val.type_name().to_string(),
        }),
    }
}

fn extract_int(val: &FfiValue) -> Result<i64, FfiError> {
    match val {
        FfiValue::Integer(i) => Ok(*i),
        FfiValue::Number(n) => Ok(*n as i64),
        _ => Err(FfiError::ArgumentType {
            expected: "Integer".to_string(),
            got: val.type_name().to_string(),
        }),
    }
}

string_function!(StrSplit, 2, FfiType::Array, "Splits a string by a delimiter into an array of strings", |args| {
    let s = extract_string(&args[0])?;
    let delim = extract_string(&args[1])?;
    let parts: Vec<FfiValue> = s.split(&delim).map(|p| FfiValue::String(p.to_string())).collect();
    Ok(FfiValue::Array(parts))
});

string_function!(StrSlice, 3, FfiType::String, "Slices a string from start to end index", |args| {
    let s = extract_string(&args[0])?;
    let start = extract_int(&args[1])?;
    let end = extract_int(&args[2])?;

    let chars: Vec<char> = s.chars().collect();
    let len = chars.len() as i64;

    let start_idx = if start < 0 { (len + start).max(0) as usize } else { (start.min(len)) as usize };
    let end_idx = if end < 0 { (len + end).max(0) as usize } else { (end.min(len)) as usize };

    if start_idx >= end_idx || start_idx >= chars.len() {
        return Ok(FfiValue::String(String::new()));
    }

    let sliced: String = chars[start_idx..end_idx].iter().collect();
    Ok(FfiValue::String(sliced))
});

string_function!(StrTrim, 1, FfiType::String, "Trims leading and trailing whitespace", |args| {
    let s = extract_string(&args[0])?;
    Ok(FfiValue::String(s.trim().to_string()))
});

string_function!(StrTrimStart, 1, FfiType::String, "Trims leading whitespace", |args| {
    let s = extract_string(&args[0])?;
    Ok(FfiValue::String(s.trim_start().to_string()))
});

string_function!(StrTrimEnd, 1, FfiType::String, "Trims trailing whitespace", |args| {
    let s = extract_string(&args[0])?;
    Ok(FfiValue::String(s.trim_end().to_string()))
});

string_function!(StrStartsWith, 2, FfiType::Boolean, "Checks if string starts with prefix", |args| {
    let s = extract_string(&args[0])?;
    let prefix = extract_string(&args[1])?;
    Ok(FfiValue::Boolean(s.starts_with(&prefix)))
});

string_function!(StrEndsWith, 2, FfiType::Boolean, "Checks if string ends with suffix", |args| {
    let s = extract_string(&args[0])?;
    let suffix = extract_string(&args[1])?;
    Ok(FfiValue::Boolean(s.ends_with(&suffix)))
});

string_function!(StrIndexOf, 2, FfiType::Int, "Finds first index of needle in string or -1", |args| {
    let s = extract_string(&args[0])?;
    let needle = extract_string(&args[1])?;
    let idx = s.find(&needle).map(|i| i as i64).unwrap_or(-1);
    Ok(FfiValue::Integer(idx))
});

string_function!(StrReplace, 3, FfiType::String, "Replaces all occurrences of from with to", |args| {
    let s = extract_string(&args[0])?;
    let from = extract_string(&args[1])?;
    let to = extract_string(&args[2])?;
    Ok(FfiValue::String(s.replace(&from, &to)))
});

string_function!(StrCharAt, 2, FfiType::String, "Returns single char at character index as string", |args| {
    let s = extract_string(&args[0])?;
    let idx = extract_int(&args[1])?;
    let chars: Vec<char> = s.chars().collect();
    if idx >= 0 && (idx as usize) < chars.len() {
        Ok(FfiValue::String(chars[idx as usize].to_string()))
    } else {
        Ok(FfiValue::String(String::new()))
    }
});

string_function!(StrRepeat, 2, FfiType::String, "Repeats string n times", |args| {
    let s = extract_string(&args[0])?;
    let count = extract_int(&args[1])?;
    let n = count.max(0) as usize;
    Ok(FfiValue::String(s.repeat(n)))
});

pub struct StrToInt;

impl RawFfiFunction for StrToInt {
    fn call(&self, ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        let s = extract_string(&args[0])?;
        let result = match s.trim().parse::<i64>() {
            Ok(value) => ctx.make_enum("Result", "ok", Some(FfiValue::Integer(value))),
            Err(error) => ctx.make_enum(
                "Result",
                "err",
                Some(FfiValue::String(error.to_string())),
            ),
        };
        Ok(result.into())
    }

    fn name(&self) -> &str {
        "StrToInt"
    }

    fn arity(&self) -> usize {
        1
    }

    fn return_type(&self) -> FfiType {
        FfiType::Any
    }

    fn description(&self) -> Option<&str> {
        Some("Parses an integer and returns Result[Int, String]")
    }
}

impl FfiFunction for StrToInt {}

string_function!(StrLen, 1, FfiType::Int, "Returns character count of string", |args| {
    let s = extract_string(&args[0])?;
    Ok(FfiValue::Integer(s.chars().count() as i64))
});

string_function!(ToString, 1, FfiType::String, "Converts value to its string representation", |args| {
    let repr = match &args[0] {
        FfiValue::String(s) => s.clone(),
        FfiValue::Atom(s) => s.clone(),
        FfiValue::Integer(i) => i.to_string(),
        FfiValue::Float(f) => f.to_string(),
        FfiValue::Number(n) => n.to_string(),
        FfiValue::Boolean(b) => b.to_string(),
        FfiValue::Unit | FfiValue::Undefined => "nil".to_string(),
        FfiValue::Array(arr) => format!("[Array len={}]", arr.len()),
        FfiValue::Object(_) => "[Object]".to_string(),
        FfiValue::Tuple(t) => format!("[Tuple len={}]", t.len()),
        FfiValue::Complex(c) => format!("{}", c),
        FfiValue::Native(_) => "[Native]".to_string(),
    };
    Ok(FfiValue::String(repr))
});

/// Register all string manipulation functions
pub fn register_string_functions(registry: &mut FfiRegistry) {
    registry.register(StrSplit);
    registry.register(StrSlice);
    registry.register(StrTrim);
    registry.register(StrTrimStart);
    registry.register(StrTrimEnd);
    registry.register(StrStartsWith);
    registry.register(StrEndsWith);
    registry.register(StrIndexOf);
    registry.register(StrReplace);
    registry.register(StrCharAt);
    registry.register(StrRepeat);
    registry.register(StrToInt);
    registry.register(StrLen);
    registry.register(ToString);
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
    fn test_string_operations() {
        let mut reg = FfiRegistry::new();
        register_string_functions(&mut reg);
        let mut ctx = MockContext;

        // StrSplit
        let res = reg.call(&mut ctx, "StrSplit", vec![
            FfiValue::String("a:b:c".to_string()),
            FfiValue::String(":".to_string()),
        ]).unwrap();
        assert_eq!(
            res,
            FfiReturn::Value(FfiValue::Array(vec![
                FfiValue::String("a".to_string()),
                FfiValue::String("b".to_string()),
                FfiValue::String("c".to_string()),
            ]))
        );

        // StrTrim & StrRepeat
        let res = reg.call(&mut ctx, "StrTrim", vec![
            FfiValue::String("  hello  ".to_string()),
        ]).unwrap();
        assert_eq!(res, FfiReturn::Value(FfiValue::String("hello".to_string())));

        let res = reg.call(&mut ctx, "StrRepeat", vec![
            FfiValue::String(" ".to_string()),
            FfiValue::Integer(4),
        ]).unwrap();
        assert_eq!(res, FfiReturn::Value(FfiValue::String("    ".to_string())));
    }
}
