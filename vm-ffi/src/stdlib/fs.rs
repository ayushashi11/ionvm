//! File system functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue, RawFfiFunction};
use std::fs;
use std::io::Write;
use std::path::Path;

pub struct ReadFile;

impl RawFfiFunction for ReadFile {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match &args[0] {
            FfiValue::String(path) => match fs::read_to_string(path) {
                Ok(content) => Ok(FfiValue::String(content).into()),
                Err(e) => Err(FfiError::RuntimeError(format!("Failed to read file '{}': {}", path, e))),
            },
            _ => Err(FfiError::ArgumentType {
                expected: "String".to_string(),
                got: args[0].type_name().to_string(),
            }),
        }
    }

    fn name(&self) -> &str {
        "ReadFile"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("Read entire file contents as a string")
    }
}

impl FfiFunction for ReadFile {}

pub struct WriteFile;

impl RawFfiFunction for WriteFile {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 2 {
            return Err(FfiError::ArgumentCount {
                expected: 2,
                got: args.len(),
            });
        }
        match (&args[0], &args[1]) {
            (FfiValue::String(path), FfiValue::String(content)) => match fs::write(path, content) {
                Ok(()) => Ok(FfiValue::Number(content.len() as f64).into()),
                Err(e) => Err(FfiError::RuntimeError(format!("Failed to write file '{}': {}", path, e))),
            },
            _ => Err(FfiError::ArgumentType {
                expected: "String, String".to_string(),
                got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
            }),
        }
    }

    fn name(&self) -> &str {
        "WriteFile"
    }

    fn arity(&self) -> usize {
        2
    }

    fn description(&self) -> Option<&str> {
        Some("Write string content to a file, returning bytes written")
    }
}

impl FfiFunction for WriteFile {}

pub struct AppendFile;

impl RawFfiFunction for AppendFile {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 2 {
            return Err(FfiError::ArgumentCount {
                expected: 2,
                got: args.len(),
            });
        }
        match (&args[0], &args[1]) {
            (FfiValue::String(path), FfiValue::String(content)) => {
                match fs::OpenOptions::new().create(true).append(true).open(path) {
                    Ok(mut file) => match file.write_all(content.as_bytes()) {
                        Ok(()) => Ok(FfiValue::Number(content.len() as f64).into()),
                        Err(e) => Err(FfiError::RuntimeError(format!("Failed to append to file '{}': {}", path, e))),
                    },
                    Err(e) => Err(FfiError::RuntimeError(format!("Failed to open file '{}': {}", path, e))),
                }
            }
            _ => Err(FfiError::ArgumentType {
                expected: "String, String".to_string(),
                got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
            }),
        }
    }

    fn name(&self) -> &str {
        "AppendFile"
    }

    fn arity(&self) -> usize {
        2
    }

    fn description(&self) -> Option<&str> {
        Some("Append string content to an existing file")
    }
}

impl FfiFunction for AppendFile {}

pub struct FileExists;

impl RawFfiFunction for FileExists {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match &args[0] {
            FfiValue::String(path) => Ok(FfiValue::Boolean(Path::new(path).exists()).into()),
            _ => Err(FfiError::ArgumentType {
                expected: "String".to_string(),
                got: args[0].type_name().to_string(),
            }),
        }
    }

    fn name(&self) -> &str {
        "FileExists"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("Check whether a file or directory exists")
    }
}

impl FfiFunction for FileExists {}

pub struct RemoveFile;

impl RawFfiFunction for RemoveFile {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match &args[0] {
            FfiValue::String(path) => match fs::remove_file(path) {
                Ok(()) => Ok(FfiValue::Boolean(true).into()),
                Err(e) => Err(FfiError::RuntimeError(format!("Failed to remove file '{}': {}", path, e))),
            },
            _ => Err(FfiError::ArgumentType {
                expected: "String".to_string(),
                got: args[0].type_name().to_string(),
            }),
        }
    }

    fn name(&self) -> &str {
        "RemoveFile"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("Delete a file at the given path")
    }
}

impl FfiFunction for RemoveFile {}

pub fn register_fs_functions(registry: &mut FfiRegistry) {
    registry.register(ReadFile);
    registry.register(WriteFile);
    registry.register(AppendFile);
    registry.register(FileExists);
    registry.register(RemoveFile);
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
    fn test_fs_functions() {
        let mut registry = FfiRegistry::new();
        register_fs_functions(&mut registry);
        let mut ctx = MockContext;

        let tmp_path = format!("/tmp/ionvm_test_fs_{}.txt", std::process::id());

        // WriteFile
        let res = registry
            .call(
                &mut ctx,
                "WriteFile",
                vec![
                    FfiValue::String(tmp_path.clone()),
                    FfiValue::String("hello ffi fs".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(res, FfiValue::Number(12.0));

        // FileExists
        let res = registry
            .call(&mut ctx, "FileExists", vec![FfiValue::String(tmp_path.clone())])
            .unwrap();
        assert_eq!(res, FfiValue::Boolean(true));

        // AppendFile
        let res = registry
            .call(
                &mut ctx,
                "AppendFile",
                vec![
                    FfiValue::String(tmp_path.clone()),
                    FfiValue::String(" appended".to_string()),
                ],
            )
            .unwrap();
        assert_eq!(res, FfiValue::Number(9.0));

        // ReadFile
        let res = registry
            .call(&mut ctx, "ReadFile", vec![FfiValue::String(tmp_path.clone())])
            .unwrap();
        assert_eq!(res, FfiValue::String("hello ffi fs appended".to_string()));

        // RemoveFile
        let res = registry
            .call(&mut ctx, "RemoveFile", vec![FfiValue::String(tmp_path.clone())])
            .unwrap();
        assert_eq!(res, FfiValue::Boolean(true));

        // FileExists should be false now
        let res = registry
            .call(&mut ctx, "FileExists", vec![FfiValue::String(tmp_path)])
            .unwrap();
        assert_eq!(res, FfiValue::Boolean(false));
    }
}
