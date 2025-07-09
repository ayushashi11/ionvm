//! Binary bytecode format for IonVM
//! 
//! This module provides functionality to serialize and deserialize
//! bytecode to/from a compact binary format for distribution and storage.

use crate::vm::{Instruction, Pattern};
use crate::value::{Value, Primitive, Function, FunctionType};
use std::cell::RefCell;
use std::io::{self, Read, Write};
use std::collections::HashMap;
use std::rc::Rc;

/// Magic bytes for IonVM bytecode files
pub const BYTECODE_MAGIC: &[u8] = b"IONBC\x01\x00\x00";

/// Version of the bytecode format
pub const BYTECODE_VERSION: u32 = 1;

/// Error type for binary bytecode operations
#[derive(Debug)]
pub enum BytecodeError {
    IoError(io::Error),
    InvalidFormat(String),
    UnsupportedVersion(u32),
    InvalidOpcode(u8),
    InvalidValue(String),
}

impl From<io::Error> for BytecodeError {
    fn from(err: io::Error) -> Self {
        BytecodeError::IoError(err)
    }
}

impl std::fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BytecodeError::IoError(e) => write!(f, "IO error: {}", e),
            BytecodeError::InvalidFormat(s) => write!(f, "Invalid format: {}", s),
            BytecodeError::UnsupportedVersion(v) => write!(f, "Unsupported version: {}", v),
            BytecodeError::InvalidOpcode(op) => write!(f, "Invalid opcode: {}", op),
            BytecodeError::InvalidValue(s) => write!(f, "Invalid value: {}", s),
        }
    }
}

impl std::error::Error for BytecodeError {}

/// Opcode mappings for instructions
#[repr(u8)]
enum Opcode {
    LoadConst = 0x01,
    Move = 0x02,
    Add = 0x03,
    Sub = 0x04,
    Mul = 0x05,
    Div = 0x06,
    GetProp = 0x07,
    SetProp = 0x08,
    Call = 0x09,
    Return = 0x0A,
    Jump = 0x0B,
    JumpIfTrue = 0x0C,
    JumpIfFalse = 0x0D,
    Spawn = 0x0E,
    Send = 0x0F,
    Receive = 0x10,
    Link = 0x11,
    Match = 0x12,
    Yield = 0x13,
    Nop = 0x14,
    // Comparison operations  
    Equal = 0x15,
    NotEqual = 0x16,
    LessThan = 0x17,
    LessEqual = 0x18,
    GreaterThan = 0x19,
    GreaterEqual = 0x1A,
    // Logical operations
    And = 0x1B,
    Or = 0x1C,
    Not = 0x1D,
}

impl TryFrom<u8> for Opcode {
    type Error = BytecodeError;
    
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x01 => Ok(Opcode::LoadConst),
            0x02 => Ok(Opcode::Move),
            0x03 => Ok(Opcode::Add),
            0x04 => Ok(Opcode::Sub),
            0x05 => Ok(Opcode::Mul),
            0x06 => Ok(Opcode::Div),
            0x07 => Ok(Opcode::GetProp),
            0x08 => Ok(Opcode::SetProp),
            0x09 => Ok(Opcode::Call),
            0x0A => Ok(Opcode::Return),
            0x0B => Ok(Opcode::Jump),
            0x0C => Ok(Opcode::JumpIfTrue),
            0x0D => Ok(Opcode::JumpIfFalse),
            0x0E => Ok(Opcode::Spawn),
            0x0F => Ok(Opcode::Send),
            0x10 => Ok(Opcode::Receive),
            0x11 => Ok(Opcode::Link),
            0x12 => Ok(Opcode::Match),
            0x13 => Ok(Opcode::Yield),
            0x14 => Ok(Opcode::Nop),
            0x15 => Ok(Opcode::Equal),
            0x16 => Ok(Opcode::NotEqual),
            0x17 => Ok(Opcode::LessThan),
            0x18 => Ok(Opcode::LessEqual),
            0x19 => Ok(Opcode::GreaterThan),
            0x1A => Ok(Opcode::GreaterEqual),
            0x1B => Ok(Opcode::And),
            0x1C => Ok(Opcode::Or),
            0x1D => Ok(Opcode::Not),
            _ => Err(BytecodeError::InvalidOpcode(value)),
        }
    }
}

/// Value type tags for serialization
#[repr(u8)]
enum ValueTag {
    Number = 0x01,
    Boolean = 0x02,
    Atom = 0x03,
    Unit = 0x04,
    Undefined = 0x05,
    Array = 0x06,
    Object = 0x07,
    Function = 0x08,
    String = 0x09,  // New tag for String type
}

/// Binary writer helper
struct BinaryWriter<W: Write> {
    writer: W,
}

impl<W: Write> BinaryWriter<W> {
    fn new(writer: W) -> Self {
        Self { writer }
    }

    fn write_u8(&mut self, value: u8) -> Result<(), BytecodeError> {
        self.writer.write_all(&[value])?;
        Ok(())
    }

    fn write_u32(&mut self, value: u32) -> Result<(), BytecodeError> {
        self.writer.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_u64(&mut self, value: u64) -> Result<(), BytecodeError> {
        self.writer.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_i32(&mut self, value: i32) -> Result<(), BytecodeError> {
        self.writer.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_f64(&mut self, value: f64) -> Result<(), BytecodeError> {
        self.writer.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_string(&mut self, s: &str) -> Result<(), BytecodeError> {
        let bytes = s.as_bytes();
        self.write_u32(bytes.len() as u32)?;
        self.writer.write_all(bytes)?;
        Ok(())
    }

    fn write_value(&mut self, value: &Value) -> Result<(), BytecodeError> {
        match value {
            Value::Primitive(Primitive::Number(n)) => {
                self.write_u8(ValueTag::Number as u8)?;
                self.write_f64(*n)?;
            },
            Value::Primitive(Primitive::Boolean(b)) => {
                self.write_u8(ValueTag::Boolean as u8)?;
                self.write_u8(if *b { 1 } else { 0 })?;
            },
            Value::Primitive(Primitive::Atom(s)) => {
                self.write_u8(ValueTag::Atom as u8)?;
                self.write_string(s)?;
            },
            Value::Primitive(Primitive::String(s)) => {
                self.write_u8(ValueTag::String as u8)?;
                self.write_string(s)?;
            },
            Value::Primitive(Primitive::Unit) => {
                self.write_u8(ValueTag::Unit as u8)?;
            },
            Value::Primitive(Primitive::Undefined) => {
                self.write_u8(ValueTag::Undefined as u8)?;
            },
            Value::Array(arr) => {
                self.write_u8(ValueTag::Array as u8)?;
                let arr_borrow = arr.borrow();
                self.write_u32(arr_borrow.len() as u32)?;
                for item in arr_borrow.iter() {
                    self.write_value(item)?;
                }
            },
            Value::Object(obj) => {
                self.write_u8(ValueTag::Object as u8)?;
                let obj_borrow = obj.borrow();
                self.write_u32(obj_borrow.properties.len() as u32)?;
                for (key, prop) in &obj_borrow.properties {
                    self.write_string(key)?;
                    self.write_value(&prop.value)?;
                    self.write_u8(if prop.writable { 1 } else { 0 })?;
                    self.write_u8(if prop.enumerable { 1 } else { 0 })?;
                    self.write_u8(if prop.configurable { 1 } else { 0 })?;
                }
            },
            Value::Function(func) => {
                self.write_u8(ValueTag::Function as u8)?;
                // Serialize function reference by name
                if let Some(ref name) = func.borrow().name {
                    self.write_string(name)?;
                } else {
                    self.write_string("anonymous")?;
                }
            },
            Value::Tuple(tuple) => {
                self.write_u8(0x09)?; // New tag for Tuple
                self.write_u32(tuple.len() as u32)?;
                for item in tuple.iter() {
                    self.write_value(item)?;
                }
            },
            _ => {
                return Err(BytecodeError::InvalidValue("Unsupported value type for serialization".to_string()));
            }
        }
        Ok(())
    }

    fn write_pattern(&mut self, pattern: &Pattern) -> Result<(), BytecodeError> {
        match pattern {
            Pattern::Value(val) => {
                self.write_u8(0x01)?; // Value pattern tag
                self.write_value(val)?;
            },
            Pattern::Wildcard => {
                self.write_u8(0x02)?; // Wildcard pattern tag
            },
            Pattern::Tuple(patterns) => {
                self.write_u8(0x03)?; // Tuple pattern tag
                self.write_u32(patterns.len() as u32)?;
                for p in patterns {
                    self.write_pattern(p)?;
                }
            },
            Pattern::Array(patterns) => {
                self.write_u8(0x04)?; // Array pattern tag
                self.write_u32(patterns.len() as u32)?;
                for p in patterns {
                    self.write_pattern(p)?;
                }
            },
            Pattern::TaggedEnum(tag, pattern) => {
                self.write_u8(0x05)?; // TaggedEnum pattern tag
                self.write_string(tag)?;
                self.write_pattern(pattern)?;
            },
        }
        Ok(())
    }

    fn write_instruction(&mut self, instr: &Instruction) -> Result<(), BytecodeError> {
        match instr {
            Instruction::LoadConst(reg, val) => {
                self.write_u8(Opcode::LoadConst as u8)?;
                self.write_u32(*reg as u32)?;
                self.write_value(val)?;
            },
            Instruction::Move(dst, src) => {
                self.write_u8(Opcode::Move as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*src as u32)?;
            },
            Instruction::Add(dst, a, b) => {
                self.write_u8(Opcode::Add as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Sub(dst, a, b) => {
                self.write_u8(Opcode::Sub as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Mul(dst, a, b) => {
                self.write_u8(Opcode::Mul as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Div(dst, a, b) => {
                self.write_u8(Opcode::Div as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Return(reg) => {
                self.write_u8(Opcode::Return as u8)?;
                self.write_u32(*reg as u32)?;
            },
            Instruction::Jump(offset) => {
                self.write_u8(Opcode::Jump as u8)?;
                self.write_i32(*offset as i32)?;
            },
            Instruction::JumpIfTrue(cond, offset) => {
                self.write_u8(Opcode::JumpIfTrue as u8)?;
                self.write_u32(*cond as u32)?;
                self.write_i32(*offset as i32)?;
            },
            Instruction::JumpIfFalse(cond, offset) => {
                self.write_u8(Opcode::JumpIfFalse as u8)?;
                self.write_u32(*cond as u32)?;
                self.write_i32(*offset as i32)?;
            },
            Instruction::Yield => {
                self.write_u8(Opcode::Yield as u8)?;
            },
            Instruction::Nop => {
                self.write_u8(Opcode::Nop as u8)?;
            },
            Instruction::GetProp(dst, obj, key) => {
                self.write_u8(Opcode::GetProp as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*obj as u32)?;
                self.write_u32(*key as u32)?;
            },
            Instruction::SetProp(obj, key, value) => {
                self.write_u8(Opcode::SetProp as u8)?;
                self.write_u32(*obj as u32)?;
                self.write_u32(*key as u32)?;
                self.write_u32(*value as u32)?;
            },
            Instruction::Call(dst, func, args) => {
                self.write_u8(Opcode::Call as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*func as u32)?;
                self.write_u32(args.len() as u32)?;
                for arg in args {
                    self.write_u32(*arg as u32)?;
                }
            },
            Instruction::Spawn(dst, func, args) => {
                self.write_u8(Opcode::Spawn as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*func as u32)?;
                self.write_u32(args.len() as u32)?;
                for arg in args {
                    self.write_u32(*arg as u32)?;
                }
            },
            Instruction::Send(proc, msg) => {
                self.write_u8(Opcode::Send as u8)?;
                self.write_u32(*proc as u32)?;
                self.write_u32(*msg as u32)?;
            },
            Instruction::Receive(dst) => {
                self.write_u8(Opcode::Receive as u8)?;
                self.write_u32(*dst as u32)?;
            },
            Instruction::Link(proc) => {
                self.write_u8(Opcode::Link as u8)?;
                self.write_u32(*proc as u32)?;
            },
            Instruction::Match(src, patterns) => {
                self.write_u8(Opcode::Match as u8)?;
                self.write_u32(*src as u32)?;
                self.write_u32(patterns.len() as u32)?;
                for (pattern, offset) in patterns {
                    self.write_pattern(pattern)?;
                    self.write_i32(*offset as i32)?;
                }
            },
            // Comparison operations
            Instruction::Equal(dst, a, b) => {
                self.write_u8(Opcode::Equal as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::NotEqual(dst, a, b) => {
                self.write_u8(Opcode::NotEqual as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::LessThan(dst, a, b) => {
                self.write_u8(Opcode::LessThan as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::LessEqual(dst, a, b) => {
                self.write_u8(Opcode::LessEqual as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::GreaterThan(dst, a, b) => {
                self.write_u8(Opcode::GreaterThan as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::GreaterEqual(dst, a, b) => {
                self.write_u8(Opcode::GreaterEqual as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            // Logical operations
            Instruction::And(dst, a, b) => {
                self.write_u8(Opcode::And as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Or(dst, a, b) => {
                self.write_u8(Opcode::Or as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*a as u32)?;
                self.write_u32(*b as u32)?;
            },
            Instruction::Not(dst, src) => {
                self.write_u8(Opcode::Not as u8)?;
                self.write_u32(*dst as u32)?;
                self.write_u32(*src as u32)?;
            },
        }
        Ok(())
    }
}

/// Binary reader helper
struct BinaryReader<R: Read> {
    reader: R,
}

impl<R: Read> BinaryReader<R> {
    fn new(reader: R) -> Self {
        Self { reader }
    }

    fn read_u8(&mut self) -> Result<u8, BytecodeError> {
        let mut buf = [0u8; 1];
        self.reader.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    fn read_u32(&mut self) -> Result<u32, BytecodeError> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    fn read_i32(&mut self) -> Result<i32, BytecodeError> {
        let mut buf = [0u8; 4];
        self.reader.read_exact(&mut buf)?;
        Ok(i32::from_le_bytes(buf))
    }

    fn read_f64(&mut self) -> Result<f64, BytecodeError> {
        let mut buf = [0u8; 8];
        self.reader.read_exact(&mut buf)?;
        Ok(f64::from_le_bytes(buf))
    }

    fn read_string(&mut self) -> Result<String, BytecodeError> {
        let len = self.read_u32()? as usize;
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf)?;
        String::from_utf8(buf)
            .map_err(|_| BytecodeError::InvalidFormat("Invalid UTF-8 string".to_string()))
    }

    fn read_value(&mut self) -> Result<Value, BytecodeError> {
        let tag = self.read_u8()?;
        match tag {
            x if x == ValueTag::Number as u8 => {
                let n = self.read_f64()?;
                Ok(Value::Primitive(Primitive::Number(n)))
            },
            x if x == ValueTag::Boolean as u8 => {
                let b = self.read_u8()? != 0;
                Ok(Value::Primitive(Primitive::Boolean(b)))
            },
            x if x == ValueTag::Atom as u8 => {
                let s = self.read_string()?;
                Ok(Value::Primitive(Primitive::Atom(s)))
            },
            x if x == ValueTag::String as u8 => {
                let s = self.read_string()?;
                Ok(Value::Primitive(Primitive::String(s)))
            },
            x if x == ValueTag::Unit as u8 => {
                Ok(Value::Primitive(Primitive::Unit))
            },
            x if x == ValueTag::Undefined as u8 => {
                Ok(Value::Primitive(Primitive::Undefined))
            },
            x if x == ValueTag::Array as u8 => {
                use std::cell::RefCell;
                use std::rc::Rc;
                
                let len = self.read_u32()? as usize;
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(self.read_value()?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            },
            x if x == ValueTag::Object as u8 => {
                use crate::value::{Object, PropertyDescriptor};
                use std::cell::RefCell;
                use std::rc::Rc;
                
                let len = self.read_u32()? as usize;
                let mut obj = Object::new(None);
                for _ in 0..len {
                    let key = self.read_string()?;
                    let value = self.read_value()?;
                    let writable = self.read_u8()? != 0;
                    let enumerable = self.read_u8()? != 0;
                    let configurable = self.read_u8()? != 0;
                    
                    obj.properties.insert(key, PropertyDescriptor {
                        value,
                        writable,
                        enumerable,
                        configurable,
                    });
                }
                Ok(Value::Object(Rc::new(RefCell::new(obj))))
            },
            x if x == ValueTag::Function as u8 => {
                let name = self.read_string()?; // Function name
                // Return a placeholder that indicates this needs function resolution
                // The VM should resolve this by looking up the function name in the class registry
                Ok(Value::Primitive(Primitive::Atom(format!("function:{}", name))))
            },
            0x09 => { // Tuple tag
                use std::rc::Rc;
                
                let len = self.read_u32()? as usize;
                let mut items = Vec::with_capacity(len);
                for _ in 0..len {
                    items.push(self.read_value()?);
                }
                Ok(Value::Tuple(Rc::new(items)))
            },
            _ => Err(BytecodeError::InvalidValue(format!("Unknown value tag: {}", tag))),
        }
    }

    fn read_pattern(&mut self) -> Result<Pattern, BytecodeError> {
        let tag = self.read_u8()?;
        match tag {
            0x01 => { // Value pattern
                let val = self.read_value()?;
                Ok(Pattern::Value(val))
            },
            0x02 => { // Wildcard pattern
                Ok(Pattern::Wildcard)
            },
            0x03 => { // Tuple pattern
                let len = self.read_u32()? as usize;
                let mut patterns = Vec::with_capacity(len);
                for _ in 0..len {
                    patterns.push(self.read_pattern()?);
                }
                Ok(Pattern::Tuple(patterns))
            },
            0x04 => { // Array pattern
                let len = self.read_u32()? as usize;
                let mut patterns = Vec::with_capacity(len);
                for _ in 0..len {
                    patterns.push(self.read_pattern()?);
                }
                Ok(Pattern::Array(patterns))
            },
            0x05 => { // TaggedEnum pattern
                let tag_name = self.read_string()?;
                let pattern = Box::new(self.read_pattern()?);
                Ok(Pattern::TaggedEnum(tag_name, pattern))
            },
            _ => Err(BytecodeError::InvalidValue(format!("Unknown pattern tag: {}", tag))),
        }
    }

    fn read_instruction(&mut self) -> Result<Instruction, BytecodeError> {
        let opcode = Opcode::try_from(self.read_u8()?)?;
        
        match opcode {
            Opcode::LoadConst => {
                let reg = self.read_u32()? as usize;
                let val = self.read_value()?;
                Ok(Instruction::LoadConst(reg, val))
            },
            Opcode::Move => {
                let dst = self.read_u32()? as usize;
                let src = self.read_u32()? as usize;
                Ok(Instruction::Move(dst, src))
            },
            Opcode::Add => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Add(dst, a, b))
            },
            Opcode::Sub => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Sub(dst, a, b))
            },
            Opcode::Mul => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Mul(dst, a, b))
            },
            Opcode::Div => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Div(dst, a, b))
            },
            Opcode::Return => {
                let reg = self.read_u32()? as usize;
                Ok(Instruction::Return(reg))
            },
            Opcode::Jump => {
                let offset = self.read_i32()? as isize;
                Ok(Instruction::Jump(offset))
            },
            Opcode::JumpIfTrue => {
                let cond = self.read_u32()? as usize;
                let offset = self.read_i32()? as isize;
                Ok(Instruction::JumpIfTrue(cond, offset))
            },
            Opcode::JumpIfFalse => {
                let cond = self.read_u32()? as usize;
                let offset = self.read_i32()? as isize;
                Ok(Instruction::JumpIfFalse(cond, offset))
            },
            Opcode::Yield => Ok(Instruction::Yield),
            Opcode::Nop => Ok(Instruction::Nop),
            Opcode::GetProp => {
                let dst = self.read_u32()? as usize;
                let obj = self.read_u32()? as usize;
                let key = self.read_u32()? as usize;
                Ok(Instruction::GetProp(dst, obj, key))
            },
            Opcode::SetProp => {
                let obj = self.read_u32()? as usize;
                let key = self.read_u32()? as usize;
                let value = self.read_u32()? as usize;
                Ok(Instruction::SetProp(obj, key, value))
            },
            Opcode::Call => {
                let dst = self.read_u32()? as usize;
                let func = self.read_u32()? as usize;
                let arg_count = self.read_u32()? as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.read_u32()? as usize);
                }
                Ok(Instruction::Call(dst, func, args))
            },
            Opcode::Spawn => {
                let dst = self.read_u32()? as usize;
                let func = self.read_u32()? as usize;
                let arg_count = self.read_u32()? as usize;
                let mut args = Vec::with_capacity(arg_count);
                for _ in 0..arg_count {
                    args.push(self.read_u32()? as usize);
                }
                Ok(Instruction::Spawn(dst, func, args))
            },
            Opcode::Send => {
                let proc = self.read_u32()? as usize;
                let msg = self.read_u32()? as usize;
                Ok(Instruction::Send(proc, msg))
            },
            Opcode::Receive => {
                let dst = self.read_u32()? as usize;
                Ok(Instruction::Receive(dst))
            },
            Opcode::Link => {
                let proc = self.read_u32()? as usize;
                Ok(Instruction::Link(proc))
            },
            Opcode::Match => {
                let src = self.read_u32()? as usize;
                let pattern_count = self.read_u32()? as usize;
                let mut patterns = Vec::with_capacity(pattern_count);
                for _ in 0..pattern_count {
                    let pattern = self.read_pattern()?;
                    let offset = self.read_i32()? as isize;
                    patterns.push((pattern, offset));
                }
                Ok(Instruction::Match(src, patterns))
            },
            // Comparison operations
            Opcode::Equal => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Equal(dst, a, b))
            },
            Opcode::NotEqual => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::NotEqual(dst, a, b))
            },
            Opcode::LessThan => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::LessThan(dst, a, b))
            },
            Opcode::LessEqual => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::LessEqual(dst, a, b))
            },
            Opcode::GreaterThan => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::GreaterThan(dst, a, b))
            },
            Opcode::GreaterEqual => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::GreaterEqual(dst, a, b))
            },
            // Logical operations
            Opcode::And => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::And(dst, a, b))
            },
            Opcode::Or => {
                let dst = self.read_u32()? as usize;
                let a = self.read_u32()? as usize;
                let b = self.read_u32()? as usize;
                Ok(Instruction::Or(dst, a, b))
            },
            Opcode::Not => {
                let dst = self.read_u32()? as usize;
                let src = self.read_u32()? as usize;
                Ok(Instruction::Not(dst, src))
            },
        }
    }

    fn read_function(&mut self) -> Result<Function, BytecodeError> {
        // Read has_name flag
        let mut flag_buf = [0u8; 1];
        self.reader.read_exact(&mut flag_buf)?;
        let has_name = flag_buf[0] != 0;

        // Read name if present
        let name = if has_name {
            let mut len_buf = [0u8; 4];
            self.reader.read_exact(&mut len_buf)?;
            let name_len = u32::from_le_bytes(len_buf) as usize;
            
            let mut name_buf = vec![0u8; name_len];
            self.reader.read_exact(&mut name_buf)?;
            Some(String::from_utf8(name_buf)
                .map_err(|_| BytecodeError::InvalidFormat("Invalid UTF-8 in function name".to_string()))?)
        } else {
            None
        };

        // Read arity
        let mut arity_buf = [0u8; 4];
        self.reader.read_exact(&mut arity_buf)?;
        let arity = u32::from_le_bytes(arity_buf) as usize;

        // Read extra_regs
        let mut extra_regs_buf = [0u8; 4];
        self.reader.read_exact(&mut extra_regs_buf)?;
        let extra_regs = u32::from_le_bytes(extra_regs_buf) as usize;

        // Read function type
        let mut type_buf = [0u8; 1];
        self.reader.read_exact(&mut type_buf)?;
        let function_type = match type_buf[0] {
            0 => {
                // Bytecode function
                let mut len_buf = [0u8; 4];
                self.reader.read_exact(&mut len_buf)?;
                let count = u32::from_le_bytes(len_buf) as usize;
                
                // Read instructions directly (not using deserialize_bytecode which expects magic header)
                let mut bytecode = Vec::with_capacity(count);
                let mut binary_reader = BinaryReader::new(&mut self.reader);
                for _ in 0..count {
                    bytecode.push(binary_reader.read_instruction()?);
                }
                
                FunctionType::Bytecode { bytecode }
            },
            1 => {
                // FFI function
                let mut len_buf = [0u8; 4];
                self.reader.read_exact(&mut len_buf)?;
                let name_len = u32::from_le_bytes(len_buf) as usize;
                
                let mut name_buf = vec![0u8; name_len];
                self.reader.read_exact(&mut name_buf)?;
                let function_name = String::from_utf8(name_buf)
                    .map_err(|_| BytecodeError::InvalidFormat("Invalid UTF-8 in FFI function name".to_string()))?;
                
                FunctionType::Ffi { function_name }
            },
            _ => return Err(BytecodeError::InvalidFormat("Invalid function type".to_string())),
        };

        Ok(Function {
            name,
            arity,
            extra_regs,
            function_type,
        })
    }
}

/// Serialize bytecode to binary format
pub fn serialize_bytecode<W: Write>(bytecode: &[Instruction], writer: W) -> Result<(), BytecodeError> {
    let mut writer = BinaryWriter::new(writer);
    
    // Write magic bytes and version
    writer.writer.write_all(BYTECODE_MAGIC)?;
    writer.write_u32(BYTECODE_VERSION)?;
    
    // Write instruction count
    writer.write_u32(bytecode.len() as u32)?;
    
    // Write instructions
    for instr in bytecode {
        writer.write_instruction(instr)?;
    }
    
    Ok(())
}

/// Deserialize bytecode from binary format
pub fn deserialize_bytecode<R: Read>(reader: R) -> Result<Vec<Instruction>, BytecodeError> {
    let mut reader = BinaryReader::new(reader);
    
    // Read and verify magic bytes
    let mut magic = vec![0u8; BYTECODE_MAGIC.len()];
    reader.reader.read_exact(&mut magic)?;
    if magic != BYTECODE_MAGIC {
        return Err(BytecodeError::InvalidFormat("Invalid magic bytes".to_string()));
    }
    
    // Read and verify version
    let version = reader.read_u32()?;
    if version != BYTECODE_VERSION {
        return Err(BytecodeError::UnsupportedVersion(version));
    }
    
    // Read instruction count
    let count = reader.read_u32()? as usize;
    
    // Read instructions
    let mut instructions = Vec::with_capacity(count);
    for _ in 0..count {
        instructions.push(reader.read_instruction()?);
    }
    
    Ok(instructions)
}

/// Serialize function to binary format
pub fn serialize_function<W: Write>(function: &Function, writer: W) -> Result<(), BytecodeError> {
    let mut writer = BinaryWriter::new(writer);
    
    // Write function metadata
    match &function.name {
        Some(name) => {
            writer.write_u8(1)?; // Has name
            writer.write_string(name)?;
        },
        None => {
            writer.write_u8(0)?; // No name
        }
    }
    
    writer.write_u32(function.arity as u32)?;
    writer.write_u32(function.extra_regs as u32)?;
    
    // Write function type and bytecode
    match &function.function_type {
        FunctionType::Bytecode { bytecode } => {
            writer.write_u8(0)?; // Bytecode function
            writer.write_u32(bytecode.len() as u32)?;
            for instr in bytecode {
                writer.write_instruction(instr)?;
            }
        },
        FunctionType::Ffi { function_name } => {
            writer.write_u8(1)?; // FFI function
            writer.write_string(function_name)?;
        },
    }
    
    Ok(())
}

/// Serialize multiple functions to binary format (new multi-function format)
pub fn serialize_functions<W: Write>(functions: &[Function], writer: W) -> Result<(), BytecodeError> {
    let mut writer = BinaryWriter::new(writer);
    
    // Write magic header for multi-function format
    writer.writer.write_all(BYTECODE_MAGIC)?;
    writer.write_u32(BYTECODE_VERSION)?;
    
    // Write function count
    writer.write_u32(functions.len() as u32)?;
    
    // Write each function
    for function in functions {
        // Write function metadata
        match &function.name {
            Some(name) => {
                writer.write_u8(1)?; // Has name
                writer.write_string(name)?;
            },
            None => {
                writer.write_u8(0)?; // No name
            }
        }
        
        writer.write_u32(function.arity as u32)?;
        writer.write_u32(function.extra_regs as u32)?;
        
        // Write function type and bytecode
        match &function.function_type {
            FunctionType::Bytecode { bytecode } => {
                writer.write_u8(0)?; // Bytecode function
                writer.write_u32(bytecode.len() as u32)?;
                for instr in bytecode {
                    writer.write_instruction(instr)?;
                }
            },
            FunctionType::Ffi { function_name } => {
                writer.write_u8(1)?; // FFI function
                writer.write_string(function_name)?;
            },
        }
    }
    
    Ok(())
}

/// Deserialize single function from binary format (legacy format)


/// Deserialize multiple functions from binary format (new multi-function format)
pub fn deserialize_functions<R: Read>(mut reader: R) -> Result<Vec<Function>, BytecodeError> {
    // Read and verify magic header
    let mut magic_buf = [0u8; 8];
    reader.read_exact(&mut magic_buf)?;
    if &magic_buf != BYTECODE_MAGIC {
        return Err(BytecodeError::InvalidFormat("Invalid magic header".to_string()));
    }
    
    // Read and verify version
    let mut version_buf = [0u8; 4];
    reader.read_exact(&mut version_buf)?;
    let version = u32::from_le_bytes(version_buf);
    if version != BYTECODE_VERSION {
        return Err(BytecodeError::UnsupportedVersion(version));
    }
    
    // Read function count
    let mut count_buf = [0u8; 4];
    reader.read_exact(&mut count_buf)?;
    let function_count = u32::from_le_bytes(count_buf) as usize;
    
    let mut functions = Vec::with_capacity(function_count);
    let mut binary_reader = BinaryReader::new(reader);
    
    // Read each function
    for _ in 0..function_count {
        // Read has_name flag
        let has_name = binary_reader.read_u8()? != 0;
        
        // Read name if present
        let name = if has_name {
            Some(binary_reader.read_string()?)
        } else {
            None
        };
        
        // Read arity
        let arity = binary_reader.read_u32()? as usize;
        
        // Read extra_regs
        let extra_regs = binary_reader.read_u32()? as usize;
        
        // Read function type
        let function_type = match binary_reader.read_u8()? {
            0 => {
                // Bytecode function
                let instr_count = binary_reader.read_u32()? as usize;
                let mut bytecode = Vec::with_capacity(instr_count);
                for _ in 0..instr_count {
                    bytecode.push(binary_reader.read_instruction()?);
                }
                FunctionType::Bytecode { bytecode }
            },
            1 => {
                // FFI function
                let function_name = binary_reader.read_string()?;
                FunctionType::Ffi { function_name }
            },
            _ => return Err(BytecodeError::InvalidFormat("Invalid function type".to_string())),
        };
        
        functions.push(Function {
            name,
            arity,
            extra_regs,
            function_type,
        });
    }
    
    Ok(functions)
}

/// Try to deserialize as multi-function format first, fallback to single function
pub fn deserialize_functions_auto<R: Read>(mut reader: R) -> Result<Vec<Function>, BytecodeError> {
    // Read the beginning to detect format
    let mut buf = Vec::new();
    reader.read_to_end(&mut buf)?;
    
    // Check if it starts with magic header (multi-function format)
    if buf.len() >= 8 && &buf[0..8] == BYTECODE_MAGIC {
        let cursor = std::io::Cursor::new(buf);
        deserialize_functions(cursor)
    } else {
        // Fallback to single function format
        let cursor = std::io::Cursor::new(buf);
        let function = deserialize_function(cursor)?;
        Ok(vec![function])
    }
}

/// Deserialize function from binary format
pub fn deserialize_function<R: Read>(mut reader: R) -> Result<Function, BytecodeError> {
    // Read has_name flag
    let mut flag_buf = [0u8; 1];
    reader.read_exact(&mut flag_buf)?;
    let has_name = flag_buf[0] != 0;

    // Read name if present
    let name = if has_name {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let name_len = u32::from_le_bytes(len_buf) as usize;
        
        let mut name_buf = vec![0u8; name_len];
        reader.read_exact(&mut name_buf)?;
        Some(String::from_utf8(name_buf)
            .map_err(|_| BytecodeError::InvalidFormat("Invalid UTF-8 in function name".to_string()))?)
    } else {
        None
    };

    // Read arity
    let mut arity_buf = [0u8; 4];
    reader.read_exact(&mut arity_buf)?;
    let arity = u32::from_le_bytes(arity_buf) as usize;

    // Read extra_regs
    let mut extra_regs_buf = [0u8; 4];
    reader.read_exact(&mut extra_regs_buf)?;
    let extra_regs = u32::from_le_bytes(extra_regs_buf) as usize;

    // Read function type
    let mut type_buf = [0u8; 1];
    reader.read_exact(&mut type_buf)?;
    let function_type = match type_buf[0] {
        0 => {
            // Bytecode function
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf)?;
            let count = u32::from_le_bytes(len_buf) as usize;
            
            // Read instructions directly (not using deserialize_bytecode which expects magic header)
            let mut bytecode = Vec::with_capacity(count);
            let mut binary_reader = BinaryReader::new(&mut reader);
            for _ in 0..count {
                bytecode.push(binary_reader.read_instruction()?);
            }
            
            FunctionType::Bytecode { bytecode }
        },
        1 => {
            // FFI function
            let mut len_buf = [0u8; 4];
            reader.read_exact(&mut len_buf)?;
            let name_len = u32::from_le_bytes(len_buf) as usize;
            
            let mut name_buf = vec![0u8; name_len];
            reader.read_exact(&mut name_buf)?;
            let function_name = String::from_utf8(name_buf)
                .map_err(|_| BytecodeError::InvalidFormat("Invalid UTF-8 in FFI function name".to_string()))?;
            
            FunctionType::Ffi { function_name }
        },
        _ => return Err(BytecodeError::InvalidFormat("Invalid function type".to_string())),
    };

    Ok(Function {
        name,
        arity,
        extra_regs,
        function_type,
    })
}

/// Deserialize function from binary format with registry
pub fn deserialize_function_with_registry<R: Read>(
    reader: &mut R, 
    function_registry: &HashMap<String, Rc<RefCell<Function>>>
) -> Result<Function, BytecodeError> {
    // First, deserialize normally
    let mut function = Rc::new(RefCell::new(deserialize_function(reader)?));
    
    // Then resolve any function references in the bytecode
    resolve_function_references(&mut function, function_registry);
    
    Ok(function.borrow().clone())
}

/// Resolve function name references in bytecode to actual Function objects
pub fn resolve_function_references(function: &mut Rc<RefCell<Function>>, function_registry: &HashMap<String, Rc<RefCell<Function>>>) {
    use crate::value::{FunctionType};
    
    if let FunctionType::Bytecode { ref mut bytecode } = function.borrow_mut().function_type {
        for instruction in bytecode.iter_mut() {
            match instruction {
                Instruction::LoadConst(_, value) => {
                    // Check if this is a function name that needs to be resolved
                    if let Value::Primitive(Primitive::Atom(name)) = value {
                        if name.starts_with("__function_ref:") {
                            let function_name = &name[15..]; // Remove "__function_ref:" prefix
                            if let Some(resolved_function) = function_registry.get(function_name) {
                                *value = Value::Function(Rc::clone(resolved_function));
                            }
                        } else if name.starts_with("__stdlib:") {
                            let stdlib_name = &name[9..]; // Remove "__stdlib:" prefix
                            // Create stdlib FFI function
                            match stdlib_name {
                                "debug" => {
                                    let debug_fn = RefCell::new(Function::new_ffi(
                                        Some("debug".to_string()),
                                        1,
                                        "Debug".to_string()  // Use the actual FFI function name
                                    ));
                                    *value = Value::Function(std::rc::Rc::new(debug_fn));
                                },
                                "print" => {
                                    let print_fn = RefCell::new(Function::new_ffi(
                                        Some("print".to_string()),
                                        1,
                                        "Print".to_string()  // Use the actual FFI function name
                                    ));
                                    *value = Value::Function(std::rc::Rc::new(print_fn));
                                },
                                _ => {} // Unknown stdlib function
                            }
                        }
                    }
                }
                _ => {} // Other instructions don't contain function references
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Instruction;
    use crate::value::{Value, Primitive};
    use std::io::Cursor;

    #[test]
    fn test_bytecode_serialization() {
        let bytecode = vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(3.0))),
            Instruction::Add(2, 0, 1),
            Instruction::Return(2),
        ];

        let mut buffer = Vec::new();
        serialize_bytecode(&bytecode, &mut buffer).unwrap();

        let cursor = Cursor::new(buffer);
        let deserialized = deserialize_bytecode(cursor).unwrap();

        assert_eq!(bytecode.len(), deserialized.len());
        // Check instruction types match (can't directly compare due to Value)
        assert!(matches!(deserialized[0], Instruction::LoadConst(0, _)));
        assert!(matches!(deserialized[1], Instruction::LoadConst(1, _)));
        assert!(matches!(deserialized[2], Instruction::Add(2, 0, 1)));
        assert!(matches!(deserialized[3], Instruction::Return(2)));
    }

    #[test]
    fn test_function_serialization() {
        use std::rc::Rc;
        
        let function = Function::new_bytecode(
            Some("test_func".to_string()),
            2,
            1, // extra_regs - arity 2 + 1 extra register (for register 2)
            vec![
                Instruction::Add(2, 0, 1),
                Instruction::Return(2),
            ]
        );

        let mut buffer = Vec::new();
        serialize_function(&function, &mut buffer).unwrap();

        // Test that we can at least serialize without errors
        // Full deserialization test would require implementing deserialize_function
        assert!(!buffer.is_empty());
    }
}
