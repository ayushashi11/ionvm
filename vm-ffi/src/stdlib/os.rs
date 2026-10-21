//! OS functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue, RawFfiFunction};

pub struct GetEnv;

impl RawFfiFunction for GetEnv {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 1 {
            return Err(FfiError::ArgumentCount {
                expected: 1,
                got: args.len(),
            });
        }
        match &args[0] {
            FfiValue::String(key) => match std::env::var(key) {
                Ok(val) => Ok(FfiValue::String(val).into()),
                Err(_) => Ok(FfiValue::Undefined.into()),
            },
            _ => Err(FfiError::ArgumentType {
                expected: "String".to_string(),
                got: args[0].type_name().to_string(),
            }),
        }
    }

    fn name(&self) -> &str {
        "GetEnv"
    }

    fn arity(&self) -> usize {
        1
    }

    fn description(&self) -> Option<&str> {
        Some("Get environment variable by name")
    }
}

impl FfiFunction for GetEnv {}

pub struct SetEnv;

impl RawFfiFunction for SetEnv {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if args.len() != 2 {
            return Err(FfiError::ArgumentCount {
                expected: 2,
                got: args.len(),
            });
        }
        match (&args[0], &args[1]) {
            (FfiValue::String(key), FfiValue::String(val)) => {
                // SAFETY: In single or multi-threaded Rust, env mutation is safe via std::env::set_var
                unsafe {
                    std::env::set_var(key, val);
                }
                Ok(FfiValue::Unit.into())
            }
            _ => Err(FfiError::ArgumentType {
                expected: "String, String".to_string(),
                got: format!("{}, {}", args[0].type_name(), args[1].type_name()),
            }),
        }
    }

    fn name(&self) -> &str {
        "SetEnv"
    }

    fn arity(&self) -> usize {
        2
    }

    fn description(&self) -> Option<&str> {
        Some("Set an environment variable")
    }
}

impl FfiFunction for SetEnv {}

pub struct OsName;

impl RawFfiFunction for OsName {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            return Err(FfiError::ArgumentCount {
                expected: 0,
                got: args.len(),
            });
        }
        Ok(FfiValue::String(std::env::consts::OS.to_string()).into())
    }

    fn name(&self) -> &str {
        "OsName"
    }

    fn arity(&self) -> usize {
        0
    }

    fn description(&self) -> Option<&str> {
        Some("Get host operating system name (e.g. linux, windows, macos)")
    }
}

impl FfiFunction for OsName {}

pub struct OsArch;

impl RawFfiFunction for OsArch {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            return Err(FfiError::ArgumentCount {
                expected: 0,
                got: args.len(),
            });
        }
        Ok(FfiValue::String(std::env::consts::ARCH.to_string()).into())
    }

    fn name(&self) -> &str {
        "OsArch"
    }

    fn arity(&self) -> usize {
        0
    }

    fn description(&self) -> Option<&str> {
        Some("Get host CPU architecture (e.g. x86_64, aarch64)")
    }
}

impl FfiFunction for OsArch {}

pub struct GetPid;

impl RawFfiFunction for GetPid {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            return Err(FfiError::ArgumentCount {
                expected: 0,
                got: args.len(),
            });
        }
        Ok(FfiValue::Number(std::process::id() as f64).into())
    }

    fn name(&self) -> &str {
        "GetPid"
    }

    fn arity(&self) -> usize {
        0
    }

    fn description(&self) -> Option<&str> {
        Some("Get the current process ID")
    }
}

impl FfiFunction for GetPid {}

pub fn register_os_functions(registry: &mut FfiRegistry) {
    registry.register(GetEnv);
    registry.register(SetEnv);
    registry.register(OsName);
    registry.register(OsArch);
    registry.register(GetPid);
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
    fn test_os_functions() {
        let mut registry = FfiRegistry::new();
        register_os_functions(&mut registry);
        let mut ctx = MockContext;

        let res = registry.call(&mut ctx, "OsName", vec![]).unwrap();
        assert_eq!(res, FfiValue::String(std::env::consts::OS.to_string()));

        let res = registry.call(&mut ctx, "OsArch", vec![]).unwrap();
        assert_eq!(res, FfiValue::String(std::env::consts::ARCH.to_string()));

        let res = registry.call(&mut ctx, "GetPid", vec![]).unwrap();
        assert!(matches!(res, crate::FfiReturn::Value(FfiValue::Number(_))));

        registry
            .call(
                &mut ctx,
                "SetEnv",
                vec![
                    FfiValue::String("IONVM_TEST_VAR".to_string()),
                    FfiValue::String("hello_ffi".to_string()),
                ],
            )
            .unwrap();

        let val = registry
            .call(
                &mut ctx,
                "GetEnv",
                vec![FfiValue::String("IONVM_TEST_VAR".to_string())],
            )
            .unwrap();
        assert_eq!(val, FfiValue::String("hello_ffi".to_string()));
    }
}
