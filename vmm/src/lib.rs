//! # IonVM - Actor Model Virtual Machine
//! 
//! IonVM is a research virtual machine implementing the actor model of computation with support for 
//! prototype-based objects, message passing, and preemptive scheduling.
//! 
//! ## Quick Start
//! 
//! ```rust
//! use vmm::{IonVM, Function, Instruction, Value, Primitive};
//! use std::rc::Rc;
//! 
//! let mut vm = IonVM::new();
//! 
//! // Create a simple function that returns 42
//! let function = Function::new_bytecode(
//!     Some("main".to_string()),
//!     0, // arity
//!     1, // extra registers
//!     vec![
//!         Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
//!         Instruction::Return(0),
//!     ],
//! );
//! 
//! let result = vm.spawn_main_process(function).unwrap();
//! ```
//! 
//! ## Features
//! 
//! - **Actor Model**: Lightweight processes with message passing
//! - **Preemptive Scheduling**: Fair, configurable timeslice-based scheduling  
//! - **Prototype Objects**: Dynamic objects with property descriptors
//! - **FFI Integration**: Bridge to native Rust functions
//! - **IonPack Format**: ZIP-based packaging for distribution

pub mod bytecode_binary;
pub mod bytecode_text;
pub mod ffi_integration;
pub mod ionpack;
pub mod value;
pub mod vm;
#[cfg(test)]
pub mod vm_timeout_tests;

#[cfg(test)]
mod integration_tests;

// Re-export commonly used types
pub use bytecode_binary::{
    BytecodeError, deserialize_bytecode, deserialize_function, serialize_function,
};
pub use ionpack::{IonPackBuilder, IonPackError, IonPackReader, Manifest};
pub use value::{Function, Object, Primitive, Process, Value};
pub use vm::{ExecutionResult, Instruction, IonVM};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_basic_math() {
        let mut vm = IonVM::new();

        // Create a simple function that adds two numbers
        let function = Function::new_bytecode(
            Some("add_test".to_string()),
            2,
            1, // extra_regs - arity 2 + 1 extra register (for register 2)
            vec![
                Instruction::LoadConst(0, Value::Primitive(Primitive::Number(10.0))),
                Instruction::LoadConst(1, Value::Primitive(Primitive::Number(32.0))),
                Instruction::Add(2, 0, 1),
                Instruction::Return(2),
            ],
        );

        let pid = vm.spawn_process(std::rc::Rc::new(function), vec![]);
        vm.run();

        // Check that the process computed the result
        if let Some(proc_ref) = vm.processes.get(&pid) {
            let proc = proc_ref.borrow();
            if let Some(Value::Primitive(Primitive::Number(result))) = &proc.last_result {
                assert_eq!(*result, 42.0);
            }
        }
    }
}
