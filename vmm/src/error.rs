use thiserror::Error;

#[derive(Error, Debug)]
pub enum BytecodeError {
    #[error("Invalid magic number in bytecode header")]
    InvalidMagic,
    #[error("Unsupported bytecode version: {0}")]
    UnsupportedVersion(u32),
    #[error("Truncated bytecode: unexpected end of stream")]
    TruncatedData,
    #[error("Invalid UTF-8 string: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
    #[error("I/O error reading bytecode: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid opcode byte: {0}")]
    InvalidOpcode(u8),
    #[error("Invalid operand tag: {0}")]
    InvalidOperandTag(u8),
    #[error("Bytecode parse error: {0}")]
    Custom(String),
}

#[derive(Error, Debug)]
pub enum VmError {
    #[error("Function '{0}' not found in bytecode")]
    FunctionNotFound(String),
    #[error("Process {0} execution error: {1}")]
    ProcessError(usize, String),
    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch {
        expected: &'static str,
        found: String,
    },
    #[error("Division by zero")]
    DivisionByZero,
    #[error("Index out of bounds: index {0}")]
    IndexOutOfBounds(usize),
    #[error("Property '{0}' not found")]
    PropertyNotFound(String),
    #[error("Cannot mutate read-only property '{0}'")]
    ReadOnlyProperty(String),
    #[error("Native I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("VM error: {0}")]
    Custom(String),
}

#[derive(Error, Debug)]
pub enum JitError {
    #[error("Unsupported instruction in function '{func}': {opcode}")]
    UnsupportedOpcode { func: String, opcode: String },
    #[error("LLVM translation error: {0}")]
    LlvmError(String),
    #[error("Execution engine creation failed: {0}")]
    EngineError(String),
}

pub type Result<T> = anyhow::Result<T>;
