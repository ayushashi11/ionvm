pub mod value;
pub mod vm;
pub mod ffi_integration;
pub mod bytecode_binary;
pub mod ionpack;
pub mod bytecode_text;
pub mod vm_timeout_tests;

#[cfg(test)]
mod integration_tests;

// Re-export commonly used types
pub use value::{Value, Primitive, Function, Object, Process};
pub use vm::{IonVM, Instruction, ExecutionResult};
pub use ionpack::{IonPackBuilder, IonPackReader, IonPackError, Manifest};
pub use bytecode_binary::{serialize_function, deserialize_function, deserialize_bytecode, BytecodeError};

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
            ]
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
