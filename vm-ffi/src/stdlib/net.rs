//! Networking functions for the standard library

use crate::{FfiError, FfiFunction, FfiRegistry, FfiResult, FfiType, FfiValue, RawFfiFunction};
use std::net::TcpStream;

macro_rules! net_function {
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

net_function!(TcpConnect, 2, FfiType::Int, "Connects to a TCP server (host: String, port: Int) and returns socket descriptor", |args| {
    let host = match &args[0] {
        FfiValue::String(s) => s.clone(),
        FfiValue::Atom(s) => s.clone(),
        _ => return Err(FfiError::ArgumentType {
            expected: "String".to_string(),
            got: args[0].type_name().to_string(),
        }),
    };
    let port = match &args[1] {
        FfiValue::Integer(i) => *i as u16,
        FfiValue::Number(n) => *n as u16,
        _ => return Err(FfiError::ArgumentType {
            expected: "Integer".to_string(),
            got: args[1].type_name().to_string(),
        }),
    };

    let addr = format!("{}:{}", host, port);
    match TcpStream::connect(&addr) {
        Ok(stream) => {
            #[cfg(unix)]
            {
                use std::os::unix::io::IntoRawFd;
                let fd = stream.into_raw_fd();
                Ok(FfiValue::Integer(fd as i64))
            }
            #[cfg(not(unix))]
            {
                Err(FfiError::RuntimeError("TCP raw socket descriptors only supported on unix".to_string()))
            }
        }
        Err(e) => Err(FfiError::RuntimeError(format!("Failed to connect to '{}': {}", addr, e))),
    }
});

/// Register all networking functions
pub fn register_net_functions(registry: &mut FfiRegistry) {
    registry.register(TcpConnect);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_net() {
        let mut reg = FfiRegistry::new();
        register_net_functions(&mut reg);
        assert!(reg.get_function_info("TcpConnect").is_some());
    }
}
