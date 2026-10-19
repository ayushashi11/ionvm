use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, Pattern, Value};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JumpCase {
    pub tag: i64,
    pub offset: i64,
}

#[derive(Clone, PartialEq, Debug)]
pub enum OperandType {
    Register(usize),
    Constant(Value),
    Argument(usize),
    Function(String),
    Offset(i64),
    Property(String),
    This,
    SelfProc,
    Pattern(Pattern),
    RegisterList(Vec<usize>),
    PropertyList(Vec<String>),
    ConstantList(Vec<Value>),
    PayloadKeyList(Vec<OperandType>),
    JumpTable(Vec<JumpCase>),
}

#[derive(Clone, PartialEq, Debug)]
pub struct Instruction {
    pub opcode: Opcode,
    pub operands: Vec<OperandType>,
}

impl Instruction {
    pub fn new(opcode: Opcode, operands: Vec<OperandType>) -> Self {
        Instruction { opcode, operands }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Bytecode {
    pub functions: Vec<Function>,
    pub entry_point: String,
}
