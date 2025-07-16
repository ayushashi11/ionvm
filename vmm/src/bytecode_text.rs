//! Textual representation of IonVM bytecode
//! 
//! This module provides functionality to convert bytecode to and from 
//! a human-readable textual format for debugging and development.

use crate::vm::{Instruction, Pattern};
use crate::value::{Value, Primitive, Function, FunctionType};
use std::fmt::{self, Write};

/// Error type for bytecode text parsing
#[derive(Debug, Clone)]
pub enum BytecodeTextError {
    InvalidInstruction(String),
    InvalidValue(String),
    InvalidFormat(String),
    ParseError(String),
}

impl fmt::Display for BytecodeTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BytecodeTextError::InvalidInstruction(s) => write!(f, "Invalid instruction: {}", s),
            BytecodeTextError::InvalidValue(s) => write!(f, "Invalid value: {}", s),
            BytecodeTextError::InvalidFormat(s) => write!(f, "Invalid format: {}", s),
            BytecodeTextError::ParseError(s) => write!(f, "Parse error: {}", s),
        }
    }
}

impl std::error::Error for BytecodeTextError {}

/// Convert an instruction to textual representation
pub fn instruction_to_text(instr: &Instruction) -> String {
    match instr {
        Instruction::ObjectInit(dst, kvs) => {
            let args_str = kvs.iter().map(|(k, v)| {
                match v {
                    crate::value::ObjectInitArg::RegisterWithFlags(r, w, e, c) => format!("{}:r{}:{}:{}:{}", k, r, w, e, c),
                    crate::value::ObjectInitArg::ValueWithFlags(val, w, e, c) => format!("{}:{}:{}:{}:{}", k, value_to_text(val), w, e, c),
                    crate::value::ObjectInitArg::Register(r) => format!("{}:r{}:true:true:true", k, r),
                    crate::value::ObjectInitArg::Value(val) => format!("{}:{}:true:true:true", k, value_to_text(val)),
                }
            }).collect::<Vec<_>>().join(", ");
            format!("OBJECT_INIT r{}, {{{}}}", dst, args_str)
        },
        Instruction::LoadConst(reg, val) => format!("LOAD_CONST r{}, {}", reg, value_to_text(val)),
        Instruction::Move(dst, src) => format!("MOVE r{}, r{}", dst, src),
        Instruction::Add(dst, a, b) => format!("ADD r{}, r{}, r{}", dst, a, b),
        Instruction::Sub(dst, a, b) => format!("SUB r{}, r{}, r{}", dst, a, b),
        Instruction::Mul(dst, a, b) => format!("MUL r{}, r{}, r{}", dst, a, b),
        Instruction::Div(dst, a, b) => format!("DIV r{}, r{}, r{}", dst, a, b),
        Instruction::GetProp(dst, obj, key) => format!("GET_PROP r{}, r{}, r{}", dst, obj, key),
        Instruction::SetProp(obj, key, val) => format!("SET_PROP r{}, r{}, r{}", obj, key, val),
        Instruction::Call(dst, func, args) => {
            let args_str = args.iter()
                .map(|r| format!("r{}", r))
                .collect::<Vec<_>>()
                .join(", ");
            format!("CALL r{}, r{}, [{}]", dst, func, args_str)
        },
        Instruction::Return(reg) => format!("RETURN r{}", reg),
        Instruction::Jump(offset) => format!("JUMP {}", offset),
        Instruction::JumpIfTrue(cond, offset) => format!("JUMP_IF_TRUE r{}, {}", cond, offset),
        Instruction::JumpIfFalse(cond, offset) => format!("JUMP_IF_FALSE r{}, {}", cond, offset),
        Instruction::Spawn(dst, func, args) => {
            let args_str = args.iter()
                .map(|r| format!("r{}", r))
                .collect::<Vec<_>>()
                .join(", ");
            format!("SPAWN r{}, r{}, [{}]", dst, func, args_str)
        },
        Instruction::Send(proc, msg) => format!("SEND r{}, r{}", proc, msg),
        Instruction::Receive(dst) => format!("RECEIVE r{}", dst),
        Instruction::ReceiveWithTimeout(dst, timeout, result) => format!("RECEIVE_WITH_TIMEOUT r{}, r{}, r{}", dst, timeout, result),
        Instruction::Link(proc) => format!("LINK r{}", proc),
        Instruction::Match(src, patterns) => {
            let patterns_str = patterns.iter()
                .map(|(pat, offset)| format!("({}, {})", pattern_to_text(pat), offset))
                .collect::<Vec<_>>()
                .join(", ");
            format!("MATCH r{}, [{}]", src, patterns_str)
        },
        Instruction::Yield => "YIELD".to_string(),
        Instruction::Nop => "NOP".to_string(),
        // Comparison operations
        Instruction::Equal(dst, a, b) => format!("EQUAL r{}, r{}, r{}", dst, a, b),
        Instruction::NotEqual(dst, a, b) => format!("NOT_EQUAL r{}, r{}, r{}", dst, a, b),
        Instruction::LessThan(dst, a, b) => format!("LESS_THAN r{}, r{}, r{}", dst, a, b),
        Instruction::LessEqual(dst, a, b) => format!("LESS_EQUAL r{}, r{}, r{}", dst, a, b),
        Instruction::GreaterThan(dst, a, b) => format!("GREATER_THAN r{}, r{}, r{}", dst, a, b),
        Instruction::GreaterEqual(dst, a, b) => format!("GREATER_EQUAL r{}, r{}, r{}", dst, a, b),
        // Logical operations
        Instruction::And(dst, a, b) => format!("AND r{}, r{}, r{}", dst, a, b),
        Instruction::Or(dst, a, b) => format!("OR r{}, r{}, r{}", dst, a, b),
        Instruction::Not(dst, src) => format!("NOT r{}, r{}", dst, src),
    }
}

/// Convert a value to textual representation
pub fn value_to_text(val: &Value) -> String {
    match val {
        Value::Primitive(Primitive::Number(n)) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{}", *n as i64)
            } else {
                format!("{}", n)
            }
        },
        Value::Primitive(Primitive::Boolean(b)) => b.to_string(),
        Value::Primitive(Primitive::String(s)) => format!("\"{}\"", s.replace("\\", "\\\\").replace("\"", "\\\"")),
        Value::Primitive(Primitive::Atom(s)) => format!("'{}'", s.replace("'", "\\'")),
        Value::Primitive(Primitive::Unit) => "()".to_string(),
        Value::Primitive(Primitive::Undefined) => "undefined".to_string(),
        Value::Array(a) => {
            let elems_str = a.borrow().iter()
                .map(value_to_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", elems_str)
        },
        Value::Object(obj) => {
            let props_str = obj.borrow().properties.iter()
                .map(|(k, v)| format!("{}: {}", k, value_to_text(&v.value.clone())))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {} }}", props_str)
        },
        Value::Tuple(tup) => {
            let elems_str = tup.iter()
                .map(value_to_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", elems_str)
        },
        Value::TaggedEnum(_) => "TaggedEnum".to_string(),
        Value::Function(_) => "Function".to_string(),
        Value::Closure(_) => "Closure".to_string(),
        Value::Process(_) => "Process".to_string(),
    }
}

/// Convert a pattern to textual representation
pub fn pattern_to_text(pat: &Pattern) -> String {
    match pat {
        Pattern::Value(val) => value_to_text(val),
        Pattern::Wildcard => "_".to_string(),
        Pattern::Tuple(patterns) => {
            let pats_str = patterns.iter()
                .map(pattern_to_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", pats_str)
        },
        Pattern::Array(patterns) => {
            let pats_str = patterns.iter()
                .map(pattern_to_text)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}]", pats_str)
        },
        Pattern::TaggedEnum(tag, inner) => {
            format!("{}({})", tag, pattern_to_text(inner))
        },
    }
}

/// Convert a function to textual representation
pub fn function_to_text(func: &Function) -> String {
    let mut result = String::new();
    
    // Function header
    let name = func.name.as_deref().unwrap_or("anonymous");
    writeln!(result, "function {} (arity: {}, extra_regs: {}) {{", name, func.arity, func.extra_regs).unwrap();
    
    match &func.function_type {
        FunctionType::Bytecode { bytecode } => {
            for (i, instr) in bytecode.iter().enumerate() {
                writeln!(result, "  {:3}: {}", i, instruction_to_text(instr)).unwrap();
            }
        },
        FunctionType::Ffi { function_name } => {
            writeln!(result, "  FFI: {}", function_name).unwrap();
        },
    }
    
    writeln!(result, "}}").unwrap();
    result
}

/// Convert bytecode to complete textual representation
pub fn bytecode_to_text(bytecode: &[Instruction]) -> String {
    let mut result = String::new();
    writeln!(result, ".bytecode").unwrap();
    
    for (i, instr) in bytecode.iter().enumerate() {
        writeln!(result, "{:4}: {}", i, instruction_to_text(instr)).unwrap();
    }
    
    writeln!(result, ".end").unwrap();
    result
}

/// Parse textual instruction into Instruction enum
pub fn parse_instruction(line: &str) -> Result<Instruction, BytecodeTextError> {
    if line.starts_with("OBJECT_INIT") {
        // Format: OBJECT_INIT rX, {key1:rY, key2:val, ...}
        let rest = line["OBJECT_INIT".len()..].trim();
        let mut parts = rest.splitn(2, ',');
        let reg_part = parts.next().ok_or(BytecodeTextError::InvalidFormat("Missing register in OBJECT_INIT".to_string()))?.trim();
        let kvs_part = parts.next().ok_or(BytecodeTextError::InvalidFormat("Missing kvs in OBJECT_INIT".to_string()))?.trim();
        let dst = parse_register(reg_part)?;
        let kvs_str = kvs_part.trim_start_matches('{').trim_end_matches('}').trim();
        let mut kvs = Vec::new();
        if !kvs_str.is_empty() {
            for pair in kvs_str.split(',') {
                let pair = pair.trim();
                let mut kv = pair.splitn(2, ':');
                let key = kv.next().ok_or(BytecodeTextError::InvalidFormat("Missing key in OBJECT_INIT kv pair".to_string()))?.trim().to_string();
                let val = kv.next().ok_or(BytecodeTextError::InvalidFormat("Missing value in OBJECT_INIT kv pair".to_string()))?.trim();
                if val.starts_with('r') {
                    let reg = parse_register(val)?;
                    kvs.push((key, crate::value::ObjectInitArg::Register(reg)));
                } else {
                    let value = parse_value(val)?;
                    kvs.push((key, crate::value::ObjectInitArg::Value(value)));
                }
            }
        }
        return Ok(Instruction::ObjectInit(dst, kvs));
    }
    let line = line.trim();
    let parts: Vec<&str> = line.split_whitespace().collect();
    
    if parts.is_empty() {
        return Err(BytecodeTextError::InvalidFormat("Empty instruction".to_string()));
    }
    
    match parts[0] {
        "LOAD_CONST" => {
            if parts.len() != 3 {
                return Err(BytecodeTextError::InvalidFormat("LOAD_CONST requires 2 arguments".to_string()));
            }
            let reg = parse_register(parts[1].trim_end_matches(','))?;
            let val = parse_value(parts[2])?;
            Ok(Instruction::LoadConst(reg, val))
        },
        "MOVE" => {
            if parts.len() != 3 {
                return Err(BytecodeTextError::InvalidFormat("MOVE requires 2 arguments".to_string()));
            }
            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let src = parse_register(parts[2])?;
            Ok(Instruction::Move(dst, src))
        },
        "ADD" => {
            if parts.len() != 4 {
                return Err(BytecodeTextError::InvalidFormat("ADD requires 3 arguments".to_string()));
            }
            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let a = parse_register(parts[2].trim_end_matches(','))?;
            let b = parse_register(parts[3])?;
            Ok(Instruction::Add(dst, a, b))
        },
        "SUB" => {
            if parts.len() != 4 {
                return Err(BytecodeTextError::InvalidFormat("SUB requires 3 arguments".to_string()));
            }
            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let a = parse_register(parts[2].trim_end_matches(','))?;
            let b = parse_register(parts[3])?;
            Ok(Instruction::Sub(dst, a, b))
        },
        "MUL" => {
            if parts.len() != 4 {
                return Err(BytecodeTextError::InvalidFormat("MUL requires 3 arguments".to_string()));
            }
            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let a = parse_register(parts[2].trim_end_matches(','))?;
            let b = parse_register(parts[3])?;
            Ok(Instruction::Mul(dst, a, b))
        },
        "DIV" => {
            if parts.len() != 4 {
                return Err(BytecodeTextError::InvalidFormat("DIV requires 3 arguments".to_string()));
            }
            let dst = parse_register(parts[1].trim_end_matches(','))?;
            let a = parse_register(parts[2].trim_end_matches(','))?;
            let b = parse_register(parts[3])?;
            Ok(Instruction::Div(dst, a, b))
        },
        "RETURN" => {
            if parts.len() != 2 {
                return Err(BytecodeTextError::InvalidFormat("RETURN requires 1 argument".to_string()));
            }
            let reg = parse_register(parts[1])?;
            Ok(Instruction::Return(reg))
        },
        "JUMP" => {
            if parts.len() != 2 {
                return Err(BytecodeTextError::InvalidFormat("JUMP requires 1 argument".to_string()));
            }
            let offset = parts[1].parse::<isize>()
                .map_err(|_| BytecodeTextError::ParseError("Invalid jump offset".to_string()))?;
            Ok(Instruction::Jump(offset))
        },
        "YIELD" => Ok(Instruction::Yield),
        "NOP" => Ok(Instruction::Nop),
        _ => Err(BytecodeTextError::InvalidInstruction(parts[0].to_string())),
    }
}

/// Parse register notation (e.g., "r0", "r15")
fn parse_register(s: &str) -> Result<usize, BytecodeTextError> {
    if !s.starts_with('r') {
        return Err(BytecodeTextError::InvalidFormat(format!("Invalid register format: {}", s)));
    }
    
    s[1..].parse::<usize>()
        .map_err(|_| BytecodeTextError::ParseError(format!("Invalid register number: {}", s)))
}

/// Parse value notation
fn parse_value(s: &str) -> Result<Value, BytecodeTextError> {
    if s == "()" {
        return Ok(Value::Primitive(Primitive::Unit));
    }
    if s == "undefined" {
        return Ok(Value::Primitive(Primitive::Undefined));
    }
    if s == "true" {
        return Ok(Value::Primitive(Primitive::Boolean(true)));
    }
    if s == "false" {
        return Ok(Value::Primitive(Primitive::Boolean(false)));
    }
    
    // Try parsing as number
    if let Ok(n) = s.parse::<f64>() {
        return Ok(Value::Primitive(Primitive::Number(n)));
    }
    
    // Try parsing as quoted string
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        let content = &s[1..s.len()-1];
        let unescaped = content.replace("\\'", "'");
        return Ok(Value::Primitive(Primitive::Atom(unescaped)));
    }
    
    Err(BytecodeTextError::InvalidValue(s.to_string()))
}

/// Parse complete textual bytecode
pub fn parse_bytecode_text(text: &str) -> Result<Vec<Instruction>, BytecodeTextError> {
    let mut instructions = Vec::new();
    let mut in_bytecode_section = false;
    
    for line in text.lines() {
        let line = line.trim();
        
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        
        if line == ".bytecode" {
            in_bytecode_section = true;
            continue;
        }
        
        if line == ".end" {
            in_bytecode_section = false;
            continue;
        }
        
        if in_bytecode_section {
            // Remove line number prefix if present (e.g., "   0: LOAD_CONST r0, 42")
            let instruction_part = if let Some(colon_pos) = line.find(':') {
                line[colon_pos + 1..].trim()
            } else {
                line
            };
            
            let instr = parse_instruction(instruction_part)?;
            instructions.push(instr);
        }
    }
    
    Ok(instructions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Instruction;
    use crate::value::{Value, Primitive};

    #[test]
    fn test_instruction_to_text() {
        let instr = Instruction::LoadConst(0, Value::Primitive(Primitive::Number(42.0)));
        assert_eq!(instruction_to_text(&instr), "LOAD_CONST r0, 42");

        let instr = Instruction::Add(2, 0, 1);
        assert_eq!(instruction_to_text(&instr), "ADD r2, r0, r1");

        let instr = Instruction::Return(5);
        assert_eq!(instruction_to_text(&instr), "RETURN r5");
    }

    #[test]
    fn test_value_to_text() {
        assert_eq!(value_to_text(&Value::Primitive(Primitive::Number(42.0))), "42");
        assert_eq!(value_to_text(&Value::Primitive(Primitive::Boolean(true))), "true");
        assert_eq!(value_to_text(&Value::Primitive(Primitive::Atom("hello".to_string()))), "'hello'");
        assert_eq!(value_to_text(&Value::Primitive(Primitive::Unit)), "()");
    }

    #[test]
    fn test_parse_instruction() {
        let instr = parse_instruction("LOAD_CONST r0, 42").unwrap();
        assert!(matches!(instr, Instruction::LoadConst(0, Value::Primitive(Primitive::Number(n))) if n == 42.0));

        let instr = parse_instruction("ADD r2, r0, r1").unwrap();
        assert!(matches!(instr, Instruction::Add(2, 0, 1)));

        let instr = parse_instruction("RETURN r5").unwrap();
        assert!(matches!(instr, Instruction::Return(5)));
    }

    #[test]
    fn test_bytecode_roundtrip() {
        let original = vec![
            Instruction::LoadConst(0, Value::Primitive(Primitive::Number(2.0))),
            Instruction::LoadConst(1, Value::Primitive(Primitive::Number(3.0))),
            Instruction::Add(2, 0, 1),
            Instruction::Return(2),
        ];

        let text = bytecode_to_text(&original);
        let parsed = parse_bytecode_text(&text).unwrap();

        assert_eq!(original.len(), parsed.len());
        // Note: We can't directly compare due to Value not implementing PartialEq in all cases
        // But we can check the structure
        assert!(matches!(parsed[0], Instruction::LoadConst(0, _)));
        assert!(matches!(parsed[1], Instruction::LoadConst(1, _)));
        assert!(matches!(parsed[2], Instruction::Add(2, 0, 1)));
        assert!(matches!(parsed[3], Instruction::Return(2)));
    }

    #[test]
    fn test_function_with_extra_regs() {
        // Test function with arity 2 and 3 extra registers
        let func = Function::new_bytecode(
            Some("test_func".to_string()),
            2, // arity: r0, r1 are arguments
            3, // extra_regs: r2, r3, r4 are extra registers for calculations
            vec![
                Instruction::Add(2, 0, 1),    // r2 = r0 + r1 (using args)
                Instruction::LoadConst(3, Value::Primitive(Primitive::Number(10.0))), // r3 = 10
                Instruction::Mul(4, 2, 3),    // r4 = r2 * r3 (using extra regs)
                Instruction::Return(4),       // return r4
            ]
        );

        assert_eq!(func.arity, 2);
        assert_eq!(func.extra_regs, 3);
        assert_eq!(func.total_registers(), 5); // 2 + 3 = 5 total registers needed

        // Test text representation includes extra_regs
        let text = function_to_text(&func);
        assert!(text.contains("arity: 2, extra_regs: 3"));
    }

    #[test]
    fn test_function_default_extra_regs() {
        // Test that default constructor sets extra_regs to 0
        let func = Function::new_bytecode(
            Some("simple_func".to_string()),
            1,
            0, // Default: no extra registers
            vec![Instruction::Return(0)]
        );

        assert_eq!(func.arity, 1);
        assert_eq!(func.extra_regs, 0);
        assert_eq!(func.total_registers(), 1);

        let text = function_to_text(&func);
        assert!(text.contains("arity: 1, extra_regs: 0"));
    }
}
