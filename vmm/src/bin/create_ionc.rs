use std::fs::File;
use vmm::value::{Function, Value, Primitive};
use vmm::vm::Instruction;
use vmm::bytecode_binary::serialize_function;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Creating a standalone .ionc file...");

    // Create a simple function
    let function = Function::new_bytecode(
        Some("test_function".to_string()),
        1, // Takes one argument
        2, // Need r1, r2 for calculation
        vec![
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(10.0))),
            Instruction::Add(2, 0, 1), // r2 = arg + 10
            Instruction::Return(2),
        ]
    );

    // Serialize to .ionc file
    let mut file = File::create("test.ionc")?;
    serialize_function(&function, &mut file)?;

    println!("Created test.ionc successfully!");
    println!("You can disassemble it with: cargo run --bin iondis test.ionc");

    Ok(())
}
