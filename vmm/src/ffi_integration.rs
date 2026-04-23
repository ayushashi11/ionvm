//! FFI Integration for the VM
//!
//! This module provides the bridge between the VM and the vm-ffi library,
//! allowing the VM to call external Rust and Python functions.

use crate::value::{Primitive, Value, Process};
use crate::vm::IonVM;
use std::cell::RefCell;
use std::rc::Rc;
use vm_ffi::{
    FfiError, FfiRegistry, FfiValue,
    bridge::{FromFfiValue, ToFfiValue},
};

pub struct VmFfiContext<'a> {
    pub vm: &'a IonVM,
    pub proc: &'a Process,
    pub bound_this: Option<Value>,
}

impl<'a> vm_ffi::FfiContext for VmFfiContext<'a> {
    fn get_this(&self) -> Option<FfiValue> {
        self.bound_this.as_ref().map(|v| v.to_ffi())
    }
}

/// Implement conversion from VM Value to FFI Value
impl ToFfiValue for Value {
    fn to_ffi(&self) -> FfiValue {
        match self {
            Value::Primitive(Primitive::Number(n)) => FfiValue::Number(*n),
            Value::Primitive(Primitive::Boolean(b)) => FfiValue::Boolean(*b),
            Value::Primitive(Primitive::String(s)) => FfiValue::String(s.clone()),
            Value::Primitive(Primitive::Complex(c)) => FfiValue::Complex(c.clone()),
            Value::Primitive(Primitive::Atom(s)) => FfiValue::Atom(s.clone()),
            Value::Primitive(Primitive::Unit) => FfiValue::Unit,
            Value::Primitive(Primitive::Undefined) => FfiValue::Undefined,

            Value::Native(native_rc) => FfiValue::Native(Rc::new(crate::value::NativeObjectWrapper {
                inner: native_rc.clone(),
            })),

            // For complex types, we could serialize them or provide references
            // For now, we'll convert them to strings
            Value::Tuple(arr) => {
                // Convert tuple elements
                let arr_borrow = arr.clone();
                let ffi_values: Vec<FfiValue> = arr_borrow.iter().map(|v| v.to_ffi()).collect();
                FfiValue::Tuple(ffi_values)
            }
            Value::Array(arr) => {
                // Convert array elements
                let arr_borrow = arr.borrow();
                let ffi_values: Vec<FfiValue> = arr_borrow.iter().map(|v| v.to_ffi()).collect();
                FfiValue::Array(ffi_values)
            }
            Value::Object(obj) => {
                // Convert object properties to FFI values
                // use crate::value::{Object, PropertyDescriptor};
                // use std::cell::RefCell;
                // use std::rc::Rc;

                let obj_borrow = obj.borrow();
                let mut ffi_obj = std::collections::HashMap::new();
                for (key, desc) in &obj_borrow.properties {
                    ffi_obj.insert(key.clone(), desc.value.to_ffi());
                }
                FfiValue::Object(ffi_obj)
            }
            Value::Function(f) => FfiValue::String(format!("[Function: {}]", f.borrow().name.clone().unwrap_or("<unnamed>".to_string()))),
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
            FfiValue::Atom(s) => Ok(Value::Primitive(Primitive::Atom(s))),
            FfiValue::String(s) => Ok(Value::Primitive(Primitive::String(s))),
            FfiValue::Complex(c) => Ok(Value::Primitive(Primitive::Complex(c))),
            FfiValue::Unit => Ok(Value::Primitive(Primitive::Unit)),
            FfiValue::Undefined => Ok(Value::Primitive(Primitive::Undefined)),
            FfiValue::Native(any_rc) => {
                if let Ok(wrapper) = any_rc.downcast::<crate::value::NativeObjectWrapper>() {
                    return Ok(Value::Native(wrapper.inner.clone()));
                }
                Err(FfiError::RuntimeError(
                    "Unsupported native object from FFI".to_string(),
                ))
            }
            FfiValue::Tuple(arr) => {
                // Convert FFI tuple to VM tuples
                use std::rc::Rc;

                let mut vm_values = Vec::new();
                for ffi_val in arr {
                    vm_values.push(Value::from_ffi(ffi_val)?);
                }
                Ok(Value::Tuple(Rc::new(vm_values)))
            }
            FfiValue::Array(arr) => {
                use std::cell::RefCell;
                use std::rc::Rc;

                let mut vm_values = Vec::new();
                for ffi_val in arr {
                    vm_values.push(Value::from_ffi(ffi_val)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(vm_values))))
            }

            FfiValue::Object(obj) => {
                // Convert FFI object to VM object
                use crate::value::{Object, PropertyDescriptor};
                use std::cell::RefCell;
                use std::rc::Rc;

                let mut vm_obj = Object::new(None);
                for (key, ffi_val) in obj {
                    let vm_val = Value::from_ffi(ffi_val)?;
                    vm_obj
                        .properties
                        .insert(key, PropertyDescriptor::public(vm_val));
                }
                Ok(Value::Object(Rc::new(RefCell::new(vm_obj))))
            }
        }
    }
}

/// FFI call result
#[derive(Debug)]
pub enum FfiCallResult {
    Success(Value),
    Error(String),
    Yield(Value), // Used for iterators
    Deferred,     // Call is handled by pushing a frame
}

/// Helper function to call an FFI function with VM values
pub fn call_ffi_function(
    vm: &mut IonVM,
    proc: &mut Process,
    bound_this: Option<Value>,
    function_name: &str,
    args: Vec<Value>,
    dst_reg: usize,
) -> FfiCallResult {
    // If bound_this is a native object, give it first crack at the call
    if let Some(Value::Native(native_rc)) = &bound_this {
        let result = native_rc
            .borrow_mut()
            .call_method(vm, proc, bound_this.clone(), function_name, args, dst_reg);
        match result {
            crate::vm::ExecutionResult::Continue => {
                return FfiCallResult::Deferred;
            }
            crate::vm::ExecutionResult::Exited(v) => return FfiCallResult::Success(v),
            crate::vm::ExecutionResult::Error(e) => return FfiCallResult::Error(e),
            _ => return FfiCallResult::Success(Value::Primitive(Primitive::Unit)),
        }
    }

    // Convert VM values to FFI values
    let ffi_args: Vec<FfiValue> = args.iter().map(|v| v.to_ffi()).collect();

    let mut ctx = VmFfiContext {
        vm,
        proc,
        bound_this,
    };

    // Call the FFI function
    match vm.ffi_registry.call(&mut ctx, function_name, ffi_args) {
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
    use std::rc::Rc;

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
        let mut vm = IonVM::new();
        let mut proc = Process::new(1, Rc::new(RefCell::new(crate::value::Function::new_ffi(None, 0, "".to_string()))), vec![]);

        let args = vec![Value::Primitive(Primitive::Number(16.0))];

        let result = call_ffi_function(&mut vm, &mut proc, None, "Sqrt", args, 0);

        match result {
            FfiCallResult::Success(Value::Primitive(Primitive::Number(n))) => {
                assert_eq!(n, 4.0);
            }
            _ => panic!("Expected successful sqrt result"),
        }
    }

    #[test]
    fn test_ffi_function_call_error() {
        let mut vm = IonVM::new();
        let mut proc = Process::new(1, Rc::new(RefCell::new(crate::value::Function::new_ffi(None, 0, "".to_string()))), vec![]);

        let args = vec![Value::Primitive(Primitive::Atom(
            "not a number".to_string(),
        ))];

        let result = call_ffi_function(&mut vm, &mut proc, None, "Sqrt", args, 0);

        match result {
            FfiCallResult::Error(_) => {
                // Expected error due to wrong argument type
            }
            _ => panic!("Expected error for wrong argument type"),
        }
    }

    #[test]
    fn test_string_functions() {
        let mut vm = IonVM::new();
        let mut proc = Process::new(1, Rc::new(RefCell::new(crate::value::Function::new_ffi(None, 0, "".to_string()))), vec![]);

        let args = vec![Value::Primitive(Primitive::String(
            "hello world".to_string(),
        ))];

        let result = call_ffi_function(&mut vm, &mut proc, None, "StrLength", args, 0);

        match result {
            FfiCallResult::Success(Value::Primitive(Primitive::Number(n))) => {
                assert_eq!(n, 11.0);
            }
            _ => panic!("Expected successful string length result"),
        }
    }
}
