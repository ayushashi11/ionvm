pub mod arithmetic;
pub mod comparisons;
pub mod ffi;
pub mod io;
pub mod logical;
pub mod native;
pub use native as native_lib;
pub mod net;
pub mod properties;

use crate::lang::types::{ValType, Value};

pub fn call_native(name: &str, args: &[Value]) -> Option<Value> {
    match name {
        "format" => {
            if args.is_empty() {
                None
            } else {
                let spec = if args.len() >= 2 {
                    match &args[1] {
                        Value::String(s) => s.as_str(),
                        _ => "",
                    }
                } else {
                    ""
                };
                Some(Value::String(crate::lang::format::format_value_with_spec(&args[0], spec)))
            }
        }
        "println" => io::println(args).ok(),
        "print" => io::print(args).ok(),
        "time" => Some(io::time()),
        "len" => {
            if args.is_empty() {
                None
            } else {
                match &args[0] {
                    Value::Array(arr) => Some(Value::Int(arr.read().unwrap().len() as i64)),
                    Value::IntArray(arr) => Some(Value::Int(arr.read().unwrap().len() as i64)),
                    Value::FloatArray(arr) => Some(Value::Int(arr.read().unwrap().len() as i64)),
                    Value::String(s) => Some(Value::Int(s.len() as i64)),
                    _ => None,
                }
            }
        }
        "push" => {
            if args.len() >= 2 {
                if let Value::Array(arr) = &args[0] {
                    arr.write().unwrap().push(args[1].clone());
                    Some(Value::Nil)
                } else {
                    None
                }
            } else {
                None
            }
        }
        "pop" => {
            if !args.is_empty() {
                if let Value::Array(arr) = &args[0] {
                    arr.write().unwrap().pop()
                } else {
                    None
                }
            } else {
                None
            }
        }
        "tcp_listen" => net::tcp_listen(args).ok(),
        "tcp_accept" => net::tcp_accept(args).ok(),
        "tcp_recv" => net::tcp_recv(args).ok(),
        "tcp_send" => net::tcp_send(args).ok(),
        "tcp_close" => net::tcp_close(args).ok(),
        _ => ffi::call_ffi(name, args),
    }
}

pub fn get_native_return_type(name: &str) -> Option<ValType> {
    match name {
        "time" => Some(ValType::Float),
        "print" | "println" => Some(ValType::Unknown),
        "tcp_listen" | "tcp_accept" | "tcp_send" | "tcp_close" => Some(ValType::Int),
        "tcp_recv" => Some(ValType::String),
        _ => ffi::get_ffi_return_type(name).map(|t| match t {
            vm_ffi::FfiType::Float => ValType::Float,
            vm_ffi::FfiType::Int => ValType::Int,
            vm_ffi::FfiType::Boolean => ValType::Bool,
            vm_ffi::FfiType::String => ValType::String,
            vm_ffi::FfiType::Array => ValType::Array,
            vm_ffi::FfiType::Object => ValType::Object,
            _ => ValType::Unknown,
        }),
    }
}

