//! Bridge between VM and FFI value systems
//!
//! This module provides conversion functions between the VM's value types
//! and the FFI's value types to enable seamless integration.

use crate::{FfiError, FfiResult, FfiValue};

/// Convert from VM Value to FFI Value
pub trait ToFfiValue {
    fn to_ffi(&self) -> FfiValue;
}

/// Convert from FFI Value to VM Value  
pub trait FromFfiValue: Sized {
    fn from_ffi(value: FfiValue) -> Result<Self, FfiError>;
}

// Note: These implementations will need to be added to the VMM crate
// since we can't implement foreign traits for foreign types here.
// This is just the interface definition.

/// Helper function to convert VM value collections to FFI arrays
pub fn vm_values_to_ffi_array<T: ToFfiValue>(values: Vec<T>) -> FfiValue {
    let ffi_values: Vec<FfiValue> = values.iter().map(|v| v.to_ffi()).collect();
    FfiValue::Array(ffi_values)
}

/// Helper function to convert FFI arrays to VM value collections
pub fn ffi_array_to_vm_values<T: FromFfiValue>(ffi_value: FfiValue) -> Result<Vec<T>, FfiError> {
    match ffi_value {
        FfiValue::Array(arr) => {
            let mut result = Vec::new();
            for ffi_val in arr {
                result.push(T::from_ffi(ffi_val)?);
            }
            Ok(result)
        }
        _ => Err(FfiError::ArgumentType {
            expected: "Array".to_string(),
            got: ffi_value.type_name().to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock VM value type for testing
    #[derive(Debug, PartialEq)]
    struct MockVmValue(i32);

    impl ToFfiValue for MockVmValue {
        fn to_ffi(&self) -> FfiValue {
            FfiValue::Number(self.0 as f64)
        }
    }

    impl FromFfiValue for MockVmValue {
        fn from_ffi(value: FfiValue) -> Result<Self, FfiError> {
            match value {
                FfiValue::Number(n) => Ok(MockVmValue(n as i32)),
                _ => Err(FfiError::ArgumentType {
                    expected: "Number".to_string(),
                    got: value.type_name().to_string(),
                }),
            }
        }
    }

    #[test]
    fn test_vm_values_to_ffi_array() {
        let vm_values = vec![MockVmValue(1), MockVmValue(2), MockVmValue(3)];
        let ffi_array = vm_values_to_ffi_array(vm_values);

        match ffi_array {
            FfiValue::Array(arr) => {
                assert_eq!(arr.len(), 3);
                assert_eq!(arr[0], FfiValue::Number(1.0));
                assert_eq!(arr[1], FfiValue::Number(2.0));
                assert_eq!(arr[2], FfiValue::Number(3.0));
            }
            _ => panic!("Expected array"),
        }
    }

    #[test]
    fn test_ffi_array_to_vm_values() {
        let ffi_array = FfiValue::Array(vec![
            FfiValue::Number(1.0),
            FfiValue::Number(2.0),
            FfiValue::Number(3.0),
        ]);

        let vm_values: Vec<MockVmValue> = ffi_array_to_vm_values(ffi_array).unwrap();
        assert_eq!(vm_values.len(), 3);
        assert_eq!(vm_values[0], MockVmValue(1));
        assert_eq!(vm_values[1], MockVmValue(2));
        assert_eq!(vm_values[2], MockVmValue(3));
    }

    #[test]
    fn test_ffi_array_conversion_error() {
        let result: Result<Vec<MockVmValue>, _> =
            ffi_array_to_vm_values(FfiValue::String("not an array".to_string()));
        assert!(matches!(result, Err(FfiError::ArgumentType { .. })));
    }
}
