//! Time functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiValue};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct GetTime;

impl FfiFunction for GetTime {
    fn call(&self, _ctx: &mut dyn crate::FfiContext, args: Vec<FfiValue>) -> FfiResult {
        if !args.is_empty() {
            return Err(FfiError::ArgumentCount {
                expected: 0,
                got: args.len(),
            });
        }

        let start = SystemTime::now();
        let since_the_epoch = start
            .duration_since(UNIX_EPOCH)
            .map_err(|e| FfiError::RuntimeError(format!("Time error: {}", e)))?;

        Ok(FfiValue::Float(since_the_epoch.as_secs_f64()))
    }

    fn name(&self) -> &str {
        "GetTime"
    }

    fn arity(&self) -> usize {
        0
    }

    fn return_type(&self) -> crate::FfiType {
        crate::FfiType::Float
    }

    fn description(&self) -> Option<&str> {
        Some("Get current system time in seconds since Unix epoch")
    }
}

pub fn register_time_functions(registry: &mut FfiRegistry) {
    registry.register(GetTime);
}
