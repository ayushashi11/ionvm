//! I/O functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue, RawFfiFunction};
use regex::Regex;
use std::io::{self, Write};

/// Macro to create I/O FFI functions
macro_rules! io_function {
    ($name:ident, $arity:expr, $description:expr, |$args:ident| $body:expr) => {
        pub struct $name;

        impl RawFfiFunction for $name {
            fn call(&self, _ctx: &mut dyn crate::FfiContext, $args: Vec<FfiValue>) -> FfiResult {
                if $args.len() != $arity {
                    return Err(FfiError::ArgumentCount {
                        expected: $arity,
                        got: $args.len(),
                    });
                }
                let res: Result<FfiValue, FfiError> = $body;
                res.map(Into::into)
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

            fn return_type(&self) -> $crate::FfiType {
                $crate::FfiType::Void
            }
        }

        impl FfiFunction for $name {}
    };

    ($name:ident,  $arity:expr, $variadic:ident, $description:expr, |$args:ident| $body:expr) => {
        pub struct $name;

        impl RawFfiFunction for $name {
            fn call(&self, _ctx: &mut dyn crate::FfiContext, $args: Vec<FfiValue>) -> FfiResult {
                if $args.len() < $arity {
                    return Err(FfiError::ArgumentCount {
                        expected: $arity,
                        got: $args.len(),
                    });
                }
                let res: Result<FfiValue, FfiError> = $body;
                res.map(Into::into)
            }

            fn name(&self) -> &str {
                stringify!($name)
            }

            fn is_variadic(&self) -> bool {
                $variadic
            }
            fn arity(&self) -> usize {
                $arity
            }

            fn description(&self) -> Option<&str> {
                Some($description)
            }

            fn return_type(&self) -> $crate::FfiType {
                $crate::FfiType::Void
            }
        }

        impl FfiFunction for $name {}
    };
}

/// Convert FFI value to display string
fn format_value(value: &FfiValue) -> String {
    match value {
        FfiValue::Integer(n) => n.to_string(),
        FfiValue::Float(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        FfiValue::Atom(s) => s.clone(),
        FfiValue::String(s) => s.clone(),
        FfiValue::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        }
        FfiValue::Boolean(b) => b.to_string(),
        FfiValue::Unit => "()".to_string(),
        FfiValue::Complex(c) => c.to_string(),
        FfiValue::Undefined => "undefined".to_string(),
        FfiValue::Tuple(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("({})", items.join(", "))
        }
        FfiValue::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        FfiValue::Object(obj) => {
            if obj.contains_key("__tag") {
                let tag = obj.get("__tag").unwrap();
                if let FfiValue::Atom(tag_str) = tag {
                    return format!(
                        ":{}{}",
                        tag_str,
                        obj.iter()
                            .filter(|(k, _)| *k != "__tag")
                            .map(|(_, v)| format_value(v))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            let items: Vec<String> = obj
                .iter()
                .map(|(k, v)| format!("{}: {}", k, format_value(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
        FfiValue::Native(_) => "[NativeObject]".to_string(),
    }
}

fn format_value_for_debug(value: &FfiValue) -> String {
    match value {
        FfiValue::Atom(s) => format!(":{}", s),
        FfiValue::String(s) => format!("\"{}\"", s),
        FfiValue::Native(_) => "[NativeObject]".to_string(),
        _ => format_value(value),
    }
}

// I/O functions
io_function!(
    Print,
    1,
    true,
    "Print a value to stdout without newline",
    |args| {
        let mut output = String::new() + &format_value(&args[0]);
        for arg in &args[1..] {
            output.push_str(&format!(", {}", format_value(arg)));
        }
        print!("{}", output);
        io::stdout()
            .flush()
            .map_err(|e| FfiError::RuntimeError(format!("IO error: {}", e)))?;
        Ok(FfiValue::Unit)
    }
);

io_function!(
    PrintLn,
    1,
    true,
    "Print a value to stdout with newline",
    |args| {
        let mut output = String::new() + &format_value(&args[0]);
        for arg in &args[1..] {
            output.push_str(&format!(", {}", format_value(arg)));
        }
        println!("{}", output);
        Ok(FfiValue::Unit)
    }
);

io_function!(
    PrintF,
    2,
    true,
    "Formatted print with format string and value",
    |args| {
        match &args[0] {
            FfiValue::String(format_str) => {
                // Simple format string replacement - just replace all the {} with values
                let mut args = args[1..].iter().map(format_value).collect::<Vec<_>>();
                let mut formatted = format_str.clone();
                // return an error if there are not enough arguments, match both {}s and {number}s
                let num_placeholders = formatted.matches("{}").count();
                let num_named_placeholders = Regex::new(r"\{\d+\}")
                    .unwrap()
                    .find_iter(&formatted)
                    .count();
                if args.len() < num_placeholders + num_named_placeholders {
                    return Err(FfiError::ArgumentCount {
                        expected: num_placeholders + num_named_placeholders,
                        got: args.len(),
                    });
                }
                // Replace all the {number}s in the format string
                // with the corresponding argument values
                args = args
                    .iter()
                    .enumerate()
                    .filter(|(i, arg)| {
                        let placeholder = format!("{{{}}}", i);
                        let ret = formatted.contains(&placeholder);
                        formatted = formatted.replace(&placeholder, arg);
                        !ret
                    })
                    .map(|(_, arg)| arg.clone())
                    .collect();
                for arg in args {
                    formatted = formatted.replacen("{}", &arg, 1);
                }
                print!("{}", formatted);
                io::stdout()
                    .flush()
                    .map_err(|e| FfiError::RuntimeError(format!("IO error: {}", e)))?;
                Ok(FfiValue::Unit)
            }
            _ => Err(FfiError::ArgumentType {
                expected: "String, ...Any".to_string(),
                got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
            }),
        }
    }
);

io_function!(
    Debug,
    1,
    "Debug print a value with type information",
    |args| {
        let type_name = args[0].type_name();
        let value_str = format_value_for_debug(&args[0]);
        println!("[DEBUG] {}: {}", type_name, value_str);
        Ok(FfiValue::Unit)
    }
);

io_function!(Eprint, 1, "Print a value to stderr", |args| {
    let output = format_value(&args[0]);
    eprintln!("{}", output);
    Ok(FfiValue::Unit)
});

pub struct ReadLine;

impl RawFfiFunction for ReadLine {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            return Err(FfiError::ArgumentCount {
                expected: 0,
                got: args.len(),
            });
        }
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => Ok(FfiValue::String(String::new()).into()),
            Ok(_) => Ok(FfiValue::String(line).into()),
            Err(e) => Err(FfiError::RuntimeError(format!("Failed to read line: {}", e))),
        }
    }

    fn name(&self) -> &str {
        "ReadLine"
    }

    fn arity(&self) -> usize {
        0
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::String
    }

    fn description(&self) -> Option<&str> {
        Some("Reads a line from standard input")
    }
}

impl FfiFunction for ReadLine {}

pub struct ReadStdin;

impl RawFfiFunction for ReadStdin {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        let count = match &args[0] {
            FfiValue::Integer(i) => (*i).max(0) as usize,
            FfiValue::Number(n) => (*n).max(0.0) as usize,
            _ => {
                return Err(FfiError::ArgumentType {
                    expected: "Integer".to_string(),
                    got: args[0].type_name().to_string(),
                })
            }
        };

        use std::io::Read;
        let mut buf = vec![0u8; count];
        let mut stdin = io::stdin();
        match stdin.read_exact(&mut buf) {
            Ok(()) => Ok(FfiValue::String(String::from_utf8_lossy(&buf).to_string()).into()),
            Err(e) => Err(FfiError::RuntimeError(format!("Failed to read stdin: {}", e))),
        }
    }

    fn name(&self) -> &str {
        "ReadStdin"
    }

    fn arity(&self) -> usize {
        1
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::String
    }

    fn description(&self) -> Option<&str> {
        Some("Reads exactly N bytes from standard input as a String")
    }
}

impl FfiFunction for ReadStdin {}

/// Register all I/O functions with the registry
pub fn register_io_functions(registry: &mut FfiRegistry) {
    registry.register(Print);
    registry.register(PrintLn);
    registry.register(PrintF);
    registry.register(Debug);
    registry.register(Eprint);
    registry.register(ReadLine);
    registry.register(ReadStdin);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockContext;
    impl crate::FfiContext for MockContext {
        fn get_this(&self) -> Option<FfiValue> {
            None
        }
    }

    #[test]
    fn test_format_value() {
        assert_eq!(format_value(&FfiValue::Integer(42)), "42");
        assert_eq!(format_value(&FfiValue::Float(3.14)), "3.14");
        assert_eq!(format_value(&FfiValue::Number(42.0)), "42");
        assert_eq!(format_value(&FfiValue::Number(3.14)), "3.14");
        assert_eq!(
            format_value(&FfiValue::String("hello".to_string())),
            "hello"
        );
        assert_eq!(format_value(&FfiValue::Boolean(true)), "true");

        let array = FfiValue::Array(vec![
            FfiValue::Integer(1),
            FfiValue::String("test".to_string()),
            FfiValue::Boolean(false),
        ]);
        assert_eq!(format_value(&array), "[1, test, false]");
    }

    #[test]
    fn test_print_functions() {
        let mut registry = FfiRegistry::new();
        register_io_functions(&mut registry);

        let mut _ctx = MockContext;

        // Test that functions are registered
        assert!(registry.get_function_info("Print").is_some());
        assert!(registry.get_function_info("PrintLn").is_some());
        assert!(registry.get_function_info("Debug").is_some());
    }
}
