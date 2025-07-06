//! FFI Integration for the VM
//! 
//! This module provides the bridge between the VM and the vm-ffi library,
//! allowing the VM to call external Rust and Python functions.

use crate::value::{Value, Primitive};
use vm_ffi::{FfiValue, FfiError, FfiRegistry, bridge::{ToFfiValue, FromFfiValue}};

/// Implement conversion from VM Value to FFI Value
impl ToFfiValue for Value {
    fn to_ffi(&self) -> FfiValue {
        match self {
            Value::Primitive(Primitive::Number(n)) => FfiValue::Number(*n),
            Value::Primitive(Primitive::Boolean(b)) => FfiValue::Boolean(*b),
            Value::Primitive(Primitive::Atom(s)) => FfiValue::String(s.clone()),
            Value::Primitive(Primitive::Unit) => FfiValue::Unit,
            Value::Primitive(Primitive::Undefined) => FfiValue::Undefined,
            
            // For complex types, we could serialize them or provide references
            // For now, we'll convert them to strings
            Value::Tuple(_) => FfiValue::String("[Tuple]".to_string()),
            Value::Array(arr) => {
                // Convert array elements
                let arr_borrow = arr.borrow();
                let ffi_values: Vec<FfiValue> = arr_borrow.iter().map(|v| v.to_ffi()).collect();
                FfiValue::Array(ffi_values)
            },
            Value::Object(_) => FfiValue::String("[Object]".to_string()),
            Value::TaggedEnum(_) => FfiValue::String("[TaggedEnum]".to_string()),
            Value::Function(_) => FfiValue::String("[Function]".to_string()),
            Value::Closure(_) => FfiValue::String("[Closure]".to_string()),
            Value::Process(_) => FfiValue::String("[Process]".to_string()),
        }
    }
}

/// Implement conversion from FFI Value to VM Value
impl FromFfiValue for Value {
    fn from_ffi(value: FfiValue) -> Result<Self, FfiError> {
        match value {
            FfiValue::Number(n) => Ok(Value::Primitive(Primitive::Number(n))),
            FfiValue::Boolean(b) => Ok(Value::Primitive(Primitive::Boolean(b))),
            FfiValue::String(s) => Ok(Value::Primitive(Primitive::Atom(s))),
            FfiValue::Unit => Ok(Value::Primitive(Primitive::Unit)),
            FfiValue::Undefined => Ok(Value::Primitive(Primitive::Undefined)),
            
            FfiValue::Array(arr) => {
                use std::cell::RefCell;
                use std::rc::Rc;
                
                let mut vm_values = Vec::new();
                for ffi_val in arr {
                    vm_values.push(Value::from_ffi(ffi_val)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vm_values))))
            },
            
            FfiValue::Object(obj) => {
                // Convert FFI object to VM object
                use crate::value::{Object, PropertyDescriptor};
                use std::cell::RefCell;
                use std::rc::Rc;
                
                let mut vm_obj = Object::new(None);
                for (key, ffi_val) in obj {
                    let vm_val = Value::from_ffi(ffi_val)?;
                    vm_obj.properties.insert(key, PropertyDescriptor {
                        value: vm_val,
                        writable: true,
                        enumerable: true,
                        configurable: true,
                    });
                }
                Ok(Value::Object(Rc::new(RefCell::new(vm_obj))))
            },
        }
    }
}

/// FFI call result
#[derive(Debug)]
pub enum FfiCallResult {
    Success(Value),
    Error(String),
}

/// Helper function to call an FFI function with VM values
pub fn call_ffi_function(
    registry: &FfiRegistry,
    function_name: &str,
    args: Vec<Value>,
) -> FfiCallResult {
    // Convert VM values to FFI values
    let ffi_args: Vec<FfiValue> = args.iter().map(|v| v.to_ffi()).collect();
    
    // Call the FFI function
    match registry.call(function_name, ffi_args) {
        Ok(ffi_result) => {
            // Convert FFI result back to VM value
            match Value::from_ffi(ffi_result) {
                Ok(vm_result) => FfiCallResult::Success(vm_result),
                Err(err) => FfiCallResult::Error(format!("FFI conversion error: {}", err)),
            }
        }
        Err(err) => FfiCallResult::Error(format!("FFI call error: {}", err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vm_ffi::FfiRegistry;

    #[test]
    fn test_value_conversion_roundtrip() {
        // Test primitive values
        let vm_num = Value::Primitive(Primitive::Number(42.0));
        let ffi_val = vm_num.to_ffi();
        let vm_result = Value::from_ffi(ffi_val).unwrap();
        assert_eq!(vm_num, vm_result);

        let vm_bool = Value::Primitive(Primitive::Boolean(true));
        let ffi_val = vm_bool.to_ffi();
        let vm_result = Value::from_ffi(ffi_val).unwrap();
        assert_eq!(vm_bool, vm_result);

        let vm_str = Value::Primitive(Primitive::Atom("hello".to_string()));
        let ffi_val = vm_str.to_ffi();
        let vm_result = Value::from_ffi(ffi_val).unwrap();
        assert_eq!(vm_str, vm_result);
    }

    #[test]
    fn test_array_conversion() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let vm_array = Value::Array(Rc::new(RefCell::new(vec![
            Value::Primitive(Primitive::Number(1.0)),
            Value::Primitive(Primitive::Number(2.0)),
            Value::Primitive(Primitive::Number(3.0)),
        ])));

        let ffi_val = vm_array.to_ffi();
        let vm_result = Value::from_ffi(ffi_val).unwrap();

        // Check that arrays are equivalent
        if let (Value::Array(orig), Value::Array(result)) = (&vm_array, &vm_result) {
            assert_eq!(*orig.borrow(), *result.borrow());
        } else {
            panic!("Expected arrays");
        }
    }

    #[test]
    fn test_ffi_function_call() {
        let registry = FfiRegistry::with_stdlib();
        
        let args = vec![
            Value::Primitive(Primitive::Number(16.0))
        ];
        
        let result = call_ffi_function(&registry, "Sqrt", args);
        
        match result {
            FfiCallResult::Success(Value::Primitive(Primitive::Number(n))) => {
                assert_eq!(n, 4.0);
            }
            _ => panic!("Expected successful sqrt result"),
        }
    }

    #[test]
    fn test_ffi_function_call_error() {
        let registry = FfiRegistry::with_stdlib();
        
        let args = vec![
            Value::Primitive(Primitive::Atom("not a number".to_string()))
        ];
        
        let result = call_ffi_function(&registry, "Sqrt", args);
        
        match result {
            FfiCallResult::Error(_) => {
                // Expected error due to wrong argument type
            }
            _ => panic!("Expected error for wrong argument type"),
        }
    }

    #[test]
    fn test_string_functions() {
        let registry = FfiRegistry::with_stdlib();
        
        let args = vec![
            Value::Primitive(Primitive::Atom("hello world".to_string()))
        ];
        
        let result = call_ffi_function(&registry, "StrLength", args);
        
        match result {
            FfiCallResult::Success(Value::Primitive(Primitive::Number(n))) => {
                assert_eq!(n, 11.0);
            }
            _ => panic!("Expected successful string length result"),
        }
    }
}
