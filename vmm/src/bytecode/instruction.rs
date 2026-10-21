use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, Object, Pattern, Value};

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
    Captured(usize),
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

    pub fn format_value(val: OperandType, spec: OperandType, dst: OperandType) -> Self {
        Instruction::new(Opcode::FormatValue, vec![val, spec, dst])
    }

    pub fn build_string(operands: Vec<OperandType>) -> Self {
        Instruction::new(Opcode::BuildString, operands)
    }

    pub fn make_env(operands: Vec<OperandType>) -> Self {
        Instruction::new(Opcode::MakeEnv, operands)
    }

    pub fn make_closure(func: OperandType, env: OperandType, dst: OperandType) -> Self {
        Instruction::new(Opcode::MakeClosure, vec![func, env, dst])
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct NativeDeclaration {
    pub lib_name: String,
    pub namespace: String,
}

#[derive(Clone, Debug)]
pub struct Bytecode {
    pub functions: Vec<Function>,
    pub entry_point: String,
    pub native_declarations: Vec<NativeDeclaration>,
    pub object_inits: Vec<(String, std::sync::Arc<std::sync::RwLock<Object>>)>,
}

impl PartialEq for Bytecode {
    fn eq(&self, other: &Self) -> bool {
        if self.functions != other.functions
            || self.entry_point != other.entry_point
            || self.native_declarations != other.native_declarations
            || self.object_inits.len() != other.object_inits.len()
        {
            return false;
        }

        let self_names: std::collections::HashMap<usize, &str> = self
            .object_inits
            .iter()
            .map(|(name, object)| (std::sync::Arc::as_ptr(object) as usize, name.as_str()))
            .collect();
        let other_names: std::collections::HashMap<usize, &str> = other
            .object_inits
            .iter()
            .map(|(name, object)| (std::sync::Arc::as_ptr(object) as usize, name.as_str()))
            .collect();

        self.object_inits.iter().all(|(name, object)| {
            let Some((_, other_object)) = other.object_inits.iter().find(|(other_name, _)| other_name == name) else {
                return false;
            };
            let object = object.read().unwrap();
            let other_object = other_object.read().unwrap();
            let prototype_name = object.prototype.as_ref().and_then(|proto| {
                self_names.get(&(std::sync::Arc::as_ptr(proto) as usize))
            });
            let other_prototype_name = other_object.prototype.as_ref().and_then(|proto| {
                other_names.get(&(std::sync::Arc::as_ptr(proto) as usize))
            });
            object.properties == other_object.properties
                && object.indexed == other_object.indexed
                && prototype_name == other_prototype_name
        })
    }
}
