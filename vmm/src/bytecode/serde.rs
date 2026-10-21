use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};
use std::sync::{Arc, RwLock};

use crate::bytecode::instruction::{Bytecode, Instruction, JumpCase, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{ArrayPattern, Function, FunctionType, ListSegment, Object, Pattern, Property, Protection, Value};

#[derive(Debug, thiserror::Error)]
pub enum SerError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid tag: {0}")]
    InvalidTag(String),
    #[error("Invalid enum tag: '{0}'")]
    InvalidEnumTag(String),
    #[error("Unsupported serialization operation")]
    UnsupportedSerialization,
}


fn write_u64<W: Write>(w: &mut W, v: u64) -> Result<(), SerError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn read_u64<R: Read>(r: &mut R) -> Result<u64, SerError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn write_i64<W: Write>(w: &mut W, v: i64) -> Result<(), SerError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn read_i64<R: Read>(r: &mut R) -> Result<i64, SerError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(i64::from_le_bytes(buf))
}

fn write_bool<W: Write>(w: &mut W, v: bool) -> Result<(), SerError> {
    w.write_all(&[if v { 1 } else { 0 }])?;
    Ok(())
}

fn read_bool<R: Read>(r: &mut R) -> Result<bool, SerError> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0] != 0)
}

fn write_f64<W: Write>(w: &mut W, v: f64) -> Result<(), SerError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

fn read_f64<R: Read>(r: &mut R) -> Result<f64, SerError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(f64::from_le_bytes(buf))
}

fn write_string<W: Write>(w: &mut W, s: &str) -> Result<(), SerError> {
    write_u64(w, s.len() as u64)?;
    w.write_all(s.as_bytes())?;
    Ok(())
}

fn read_string<R: Read>(r: &mut R) -> Result<String, SerError> {
    let len = read_u64(r)? as usize;
    if len > 16 * 1024 * 1024 {
        return Err(SerError::InvalidTag(format!("String length {} exceeds 16MB limit", len)));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|_| SerError::InvalidTag("Invalid UTF-8 string".into()))
}


fn write_protection<W: Write>(w: &mut W, p: Protection) -> Result<(), SerError> {
    let s = match p {
        Protection::Writeable => "writeable",
        Protection::Readonly => "readonly",
        Protection::ThisWriteable => "thiswriteable",
        Protection::ThisReadonly => "thisreadonly",
        Protection::Private => "private",
    };
    write_string(w, s)
}

fn read_protection<R: Read>(r: &mut R) -> Result<Protection, SerError> {
    let tag = read_string(r)?;
    match tag.as_str() {
        "writeable" => Ok(Protection::Writeable),
        "readonly" => Ok(Protection::Readonly),
        "thiswriteable" => Ok(Protection::ThisWriteable),
        "thisreadonly" => Ok(Protection::ThisReadonly),
        "private" => Ok(Protection::Private),
        _ => Err(SerError::InvalidEnumTag(tag)),
    }
}

fn write_property<W: Write>(w: &mut W, prop: &Property) -> Result<(), SerError> {
    write_value(w, &prop.value)?;
    write_protection(w, prop.protection)?;
    Ok(())
}

fn read_property<R: Read>(r: &mut R) -> Result<Property, SerError> {
    let value = read_value(r)?;
    let protection = read_protection(r)?;
    Ok(Property { value, protection })
}

pub fn write_object<W: Write>(w: &mut W, obj: &Object) -> Result<(), SerError> {
    write_bool(w, obj.prototype.is_some())?;
    if let Some(proto) = &obj.prototype {
        write_object(w, &proto.read().unwrap())?;
    }
    let property_count = obj.properties.keys().filter(|key| crate::lang::types::parse_numeric_key(key).is_none()).count()
        + obj.indexed.iter().filter(|entry| entry.is_some()).count();
    write_u64(w, property_count as u64)?;
    for (k, v) in &obj.properties {
        if crate::lang::types::parse_numeric_key(k).is_some() {
            continue;
        }
        write_string(w, k)?;
        write_property(w, v)?;
    }
    for (idx, prop) in obj.indexed.iter().enumerate() {
        if let Some(prop) = prop {
            write_string(w, &idx.to_string())?;
            write_property(w, prop)?;
        }
    }
    Ok(())
}

pub fn read_object<R: Read>(r: &mut R) -> Result<Arc<RwLock<Object>>, SerError> {
    let has_proto = read_bool(r)?;
    let proto = if has_proto {
        Some(read_object(r)?)
    } else {
        None
    };
    let count = read_u64(r)? as usize;
    let mut properties = HashMap::new();
    let mut indexed = Vec::new();
    for _ in 0..count {
        let name = read_string(r)?;
        let prop = read_property(r)?;
        if let Some(idx) = crate::lang::types::parse_numeric_key(&name) {
            if idx >= indexed.len() {
                indexed.resize(idx + 1, None);
            }
            indexed[idx] = Some(prop.clone());
        }
        if crate::lang::types::parse_numeric_key(&name).is_none() {
            properties.insert(name, prop);
        }
    }
    Ok(Arc::new(RwLock::new(Object {
        prototype: proto,
        properties,
        indexed,
    })))
}

fn write_load_time_object<W: Write>(
    w: &mut W,
    obj: &Object,
    object_names: &HashMap<usize, String>,
) -> Result<(), SerError> {
    match &obj.prototype {
        Some(proto) => {
            let ptr = Arc::as_ptr(proto) as usize;
            let name = object_names.get(&ptr).ok_or_else(|| {
                SerError::InvalidTag("load-time object prototype is not registered".to_string())
            })?;
            write_bool(w, true)?;
            write_string(w, name)?;
        }
        None => write_bool(w, false)?,
    }

    let property_count = obj.properties.keys().filter(|key| crate::lang::types::parse_numeric_key(key).is_none()).count()
        + obj.indexed.iter().filter(|entry| entry.is_some()).count();
    write_u64(w, property_count as u64)?;
    for (key, prop) in &obj.properties {
        if crate::lang::types::parse_numeric_key(key).is_some() {
            continue;
        }
        write_string(w, key)?;
        write_property(w, prop)?;
    }
    for (idx, prop) in obj.indexed.iter().enumerate() {
        if let Some(prop) = prop {
            write_string(w, &idx.to_string())?;
            write_property(w, prop)?;
        }
    }
    Ok(())
}

fn read_load_time_object<R: Read>(
    r: &mut R,
    objects: &HashMap<String, Arc<RwLock<Object>>>,
) -> Result<Object, SerError> {
    let prototype = if read_bool(r)? {
        let name = read_string(r)?;
        Some(objects.get(&name).cloned().ok_or_else(|| {
            SerError::InvalidTag(format!(
                "load-time object prototype '{}' must be initialized before its subclass",
                name
            ))
        })?)
    } else {
        None
    };

    let count = read_u64(r)? as usize;
    let mut properties = HashMap::new();
    let mut indexed = Vec::new();
    for _ in 0..count {
        let name = read_string(r)?;
        let prop = read_property(r)?;
        if let Some(idx) = crate::lang::types::parse_numeric_key(&name) {
            if idx >= indexed.len() {
                indexed.resize(idx + 1, None);
            }
            indexed[idx] = Some(prop);
        } else {
            properties.insert(name, prop);
        }
    }
    Ok(Object {
        prototype,
        properties,
        indexed,
    })
}

pub fn write_value<W: Write>(w: &mut W, val: &Value) -> Result<(), SerError> {
    match val {
        Value::Int(v) => {
            write_string(w, "int")?;
            write_i64(w, *v)?;
        }
        Value::Float(v) => {
            write_string(w, "float")?;
            write_f64(w, *v)?;
        }
        Value::Bool(v) => {
            write_string(w, "bool")?;
            write_bool(w, *v)?;
        }
        Value::String(s) => {
            write_string(w, "string")?;
            write_string(w, s)?;
        }
        Value::Object(obj) => {
            write_string(w, "object")?;
            write_object(w, &obj.read().unwrap())?;
        }
        Value::Function(f) => {
            write_string(w, "function")?;
            write_function(w, &f.read().unwrap())?;
        }
        Value::Nil => {
            write_string(w, "nil")?;
        }
        Value::Array(arr) => {
            write_string(w, "array")?;
            let borrowed = arr.read().unwrap();
            write_u64(w, borrowed.len() as u64)?;
            for item in borrowed.iter() {
                write_value(w, item)?;
            }
        }
        Value::IntArray(arr) => {
            write_string(w, "int_array")?;
            let borrowed = arr.read().unwrap();
            write_u64(w, borrowed.len() as u64)?;
            for item in borrowed.iter() {
                write_i64(w, *item)?;
            }
        }
        Value::FloatArray(arr) => {
            write_string(w, "float_array")?;
            let borrowed = arr.read().unwrap();
            write_u64(w, borrowed.len() as u64)?;
            for item in borrowed.iter() {
                write_f64(w, *item)?;
            }
        }
    }
    Ok(())
}

pub fn read_value<R: Read>(r: &mut R) -> Result<Value, SerError> {
    let tag = read_string(r)?;
    match tag.as_str() {
        "int" => Ok(Value::Int(read_i64(r)?)),
        "float" => Ok(Value::Float(read_f64(r)?)),
        "bool" => Ok(Value::Bool(read_bool(r)?)),
        "string" => Ok(Value::String(read_string(r)?)),
        "object" => Ok(Value::Object(read_object(r)?)),
        "function" => Ok(Value::Function(Arc::new(RwLock::new(read_function(r)?)))),
        "nil" => Ok(Value::Nil),
        "array" => {
            let len = read_u64(r)? as usize;
            if len > 1024 * 1024 {
                return Err(SerError::InvalidTag(format!("Array length {} exceeds limit", len)));
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(read_value(r)?);
            }
            Ok(Value::Array(Arc::new(RwLock::new(items))))
        }
        "int_array" => {
            let len = read_u64(r)? as usize;
            if len > 10 * 1024 * 1024 {
                return Err(SerError::InvalidTag(format!("IntArray length {} exceeds limit", len)));
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(read_i64(r)?);
            }
            Ok(Value::IntArray(Arc::new(RwLock::new(items))))
        }
        "float_array" => {
            let len = read_u64(r)? as usize;
            if len > 10 * 1024 * 1024 {
                return Err(SerError::InvalidTag(format!("FloatArray length {} exceeds limit", len)));
            }
            let mut items = Vec::with_capacity(len);
            for _ in 0..len {
                items.push(read_f64(r)?);
            }
            Ok(Value::FloatArray(Arc::new(RwLock::new(items))))
        }
        _ => Err(SerError::InvalidTag(tag)),
    }
}

pub fn write_list_segment<W: Write>(w: &mut W, seg: &ListSegment) -> Result<(), SerError> {
    match seg {
        ListSegment::Element(pat) => {
            write_string(w, "elem")?;
            write_pattern(w, pat)
        }
        ListSegment::Elements(pats) => {
            write_string(w, "elems")?;
            write_u64(w, pats.len() as u64)?;
            for p in pats {
                write_pattern(w, p)?;
            }
            Ok(())
        }
        ListSegment::BindList(reg) => {
            write_string(w, "bind_list")?;
            write_u64(w, *reg as u64)
        }
        ListSegment::WildcardList => write_string(w, "wildcard_list"),
    }
}

pub fn read_list_segment<R: Read>(r: &mut R) -> Result<ListSegment, SerError> {
    let tag = read_string(r)?;
    match tag.as_str() {
        "elem" => Ok(ListSegment::Element(read_pattern(r)?)),
        "elems" => {
            let count = read_u64(r)? as usize;
            let mut pats = Vec::with_capacity(count);
            for _ in 0..count {
                pats.push(read_pattern(r)?);
            }
            Ok(ListSegment::Elements(pats))
        }
        "bind_list" => Ok(ListSegment::BindList(read_u64(r)? as usize)),
        "wildcard_list" => Ok(ListSegment::WildcardList),
        _ => Err(SerError::InvalidTag(tag)),
    }
}

pub fn write_array_pattern<W: Write>(w: &mut W, pat: &ArrayPattern) -> Result<(), SerError> {
    write_u64(w, pat.segments.len() as u64)?;
    for seg in &pat.segments {
        write_list_segment(w, seg)?;
    }
    Ok(())
}

pub fn read_array_pattern<R: Read>(r: &mut R) -> Result<ArrayPattern, SerError> {
    let count = read_u64(r)? as usize;
    let mut segments = Vec::with_capacity(count);
    for _ in 0..count {
        segments.push(read_list_segment(r)?);
    }
    Ok(ArrayPattern { segments })
}

pub fn write_pattern<W: Write>(w: &mut W, pattern: &Pattern) -> Result<(), SerError> {
    match pattern {
        Pattern::Wildcard => write_string(w, "wildcard"),
        Pattern::Literal(v) => {
            write_string(w, "literal")?;
            write_value(w, v)
        }
        Pattern::Bind(reg) => {
            write_string(w, "bind")?;
            write_u64(w, *reg as u64)
        }
        Pattern::Tuple(items) => {
            write_string(w, "tuple")?;
            write_u64(w, items.len() as u64)?;
            for item in items {
                write_pattern(w, item)?;
            }
            Ok(())
        }
        Pattern::Object(props) => {
            write_string(w, "object")?;
            write_u64(w, props.len() as u64)?;
            for (key, pat) in props {
                write_string(w, key)?;
                write_pattern(w, pat)?;
            }
            Ok(())
        }
        Pattern::IntRange(start, end, inclusive) => {
            write_string(w, "int_range")?;
            match start {
                Some(s) => {
                    write_bool(w, true)?;
                    write_i64(w, *s)?;
                }
                None => write_bool(w, false)?,
            }
            match end {
                Some(e) => {
                    write_bool(w, true)?;
                    write_i64(w, *e)?;
                }
                None => write_bool(w, false)?,
            }
            write_bool(w, *inclusive)
        }
        Pattern::FloatRange(start, end, inclusive) => {
            write_string(w, "float_range")?;
            match start {
                Some(s) => {
                    write_bool(w, true)?;
                    write_f64(w, *s)?;
                }
                None => write_bool(w, false)?,
            }
            match end {
                Some(e) => {
                    write_bool(w, true)?;
                    write_f64(w, *e)?;
                }
                None => write_bool(w, false)?,
            }
            write_bool(w, *inclusive)
        }
        Pattern::Array(arr_pat) => {
            write_string(w, "array_pat")?;
            write_array_pattern(w, arr_pat)
        }
        Pattern::IntArray(arr_pat) => {
            write_string(w, "int_array_pat")?;
            write_array_pattern(w, arr_pat)
        }
        Pattern::FloatArray(arr_pat) => {
            write_string(w, "float_array_pat")?;
            write_array_pattern(w, arr_pat)
        }
    }
}

pub fn read_pattern<R: Read>(r: &mut R) -> Result<Pattern, SerError> {
    let tag = read_string(r)?;
    match tag.as_str() {
        "wildcard" => Ok(Pattern::Wildcard),
        "literal" => Ok(Pattern::Literal(read_value(r)?)),
        "bind" => Ok(Pattern::Bind(read_u64(r)? as usize)),
        "tuple" => {
            let count = read_u64(r)? as usize;
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(read_pattern(r)?);
            }
            Ok(Pattern::Tuple(items))
        }
        "object" => {
            let count = read_u64(r)? as usize;
            let mut props = Vec::with_capacity(count);
            for _ in 0..count {
                let key = read_string(r)?;
                let pat = read_pattern(r)?;
                props.push((key, pat));
            }
            Ok(Pattern::Object(props))
        }
        "int_range" => {
            let has_start = read_bool(r)?;
            let start = if has_start { Some(read_i64(r)?) } else { None };
            let has_end = read_bool(r)?;
            let end = if has_end { Some(read_i64(r)?) } else { None };
            let inclusive = read_bool(r)?;
            Ok(Pattern::IntRange(start, end, inclusive))
        }
        "float_range" => {
            let has_start = read_bool(r)?;
            let start = if has_start { Some(read_f64(r)?) } else { None };
            let has_end = read_bool(r)?;
            let end = if has_end { Some(read_f64(r)?) } else { None };
            let inclusive = read_bool(r)?;
            Ok(Pattern::FloatRange(start, end, inclusive))
        }
        "array_pat" => Ok(Pattern::Array(read_array_pattern(r)?)),
        "int_array_pat" => Ok(Pattern::IntArray(read_array_pattern(r)?)),
        "float_array_pat" => Ok(Pattern::FloatArray(read_array_pattern(r)?)),
        _ => Err(SerError::InvalidTag(tag)),
    }
}

pub fn write_operand<W: Write>(w: &mut W, op: &OperandType) -> Result<(), SerError> {
    match op {
        OperandType::Register(reg) => {
            write_string(w, "register")?;
            write_u64(w, *reg as u64)?;
        }
        OperandType::Constant(val) => {
            write_string(w, "constant")?;
            write_value(w, val)?;
        }
        OperandType::Argument(arg) => {
            write_string(w, "argument")?;
            write_u64(w, *arg as u64)?;
        }
        OperandType::Function(name) => {
            write_string(w, "function")?;
            write_string(w, name)?;
        }
        OperandType::Offset(offset) => {
            write_string(w, "offset")?;
            write_i64(w, *offset)?;
        }
        OperandType::Property(prop) => {
            write_string(w, "property")?;
            write_string(w, prop)?;
        }
        OperandType::This => {
            write_string(w, "this")?;
        }
        OperandType::SelfProc => {
            write_string(w, "self_proc")?;
        }
        OperandType::Pattern(pat) => {
            write_string(w, "pattern")?;
            write_pattern(w, pat)?;
        }
        OperandType::RegisterList(regs) => {
            write_string(w, "register_list")?;
            write_u64(w, regs.len() as u64)?;
            for r in regs {
                write_u64(w, *r as u64)?;
            }
        }
        OperandType::PropertyList(props) => {
            write_string(w, "property_list")?;
            write_u64(w, props.len() as u64)?;
            for p in props {
                write_string(w, p)?;
            }
        }
        OperandType::ConstantList(consts) => {
            write_string(w, "constant_list")?;
            write_u64(w, consts.len() as u64)?;
            for c in consts {
                write_value(w, c)?;
            }
        }
        OperandType::PayloadKeyList(keys) => {
            write_string(w, "payload_key_list")?;
            write_u64(w, keys.len() as u64)?;
            for k in keys {
                write_operand(w, k)?;
            }
        }
        OperandType::JumpTable(cases) => {
            write_string(w, "jump_table")?;
            write_u64(w, cases.len() as u64)?;
            for c in cases {
                write_i64(w, c.tag)?;
                write_i64(w, c.offset)?;
            }
        }
        OperandType::Captured(slot) => {
            write_string(w, "captured")?;
            write_u64(w, *slot as u64)?;
        }
    }
    Ok(())
}

pub fn read_operand<R: Read>(r: &mut R) -> Result<OperandType, SerError> {
    let tag = read_string(r)?;
    match tag.as_str() {
        "register" => Ok(OperandType::Register(read_u64(r)? as usize)),
        "constant" => Ok(OperandType::Constant(read_value(r)?)),
        "argument" => Ok(OperandType::Argument(read_u64(r)? as usize)),
        "function" => Ok(OperandType::Function(read_string(r)?)),
        "offset" => Ok(OperandType::Offset(read_i64(r)?)),
        "property" => Ok(OperandType::Property(read_string(r)?)),
        "this" => Ok(OperandType::This),
        "self_proc" => Ok(OperandType::SelfProc),
        "pattern" => Ok(OperandType::Pattern(read_pattern(r)?)),
        "register_list" => {
            let count = read_u64(r)? as usize;
            let mut regs = Vec::with_capacity(count);
            for _ in 0..count {
                regs.push(read_u64(r)? as usize);
            }
            Ok(OperandType::RegisterList(regs))
        }
        "property_list" => {
            let count = read_u64(r)? as usize;
            let mut props = Vec::with_capacity(count);
            for _ in 0..count {
                props.push(read_string(r)?);
            }
            Ok(OperandType::PropertyList(props))
        }
        "constant_list" => {
            let count = read_u64(r)? as usize;
            let mut consts = Vec::with_capacity(count);
            for _ in 0..count {
                consts.push(read_value(r)?);
            }
            Ok(OperandType::ConstantList(consts))
        }
        "payload_key_list" => {
            let count = read_u64(r)? as usize;
            let mut keys = Vec::with_capacity(count);
            for _ in 0..count {
                keys.push(read_operand(r)?);
            }
            Ok(OperandType::PayloadKeyList(keys))
        }
        "jump_table" => {
            let count = read_u64(r)? as usize;
            let mut cases = Vec::with_capacity(count);
            for _ in 0..count {
                let tag = read_i64(r)?;
                let offset = read_i64(r)?;
                cases.push(JumpCase { tag, offset });
            }
            Ok(OperandType::JumpTable(cases))
        }
        "captured" => Ok(OperandType::Captured(read_u64(r)? as usize)),
        _ => Err(SerError::InvalidTag(tag)),
    }
}

pub fn write_instruction<W: Write>(w: &mut W, inst: &Instruction) -> Result<(), SerError> {
    write_string(w, inst.opcode.as_str())?;
    write_u64(w, inst.operands.len() as u64)?;
    for op in &inst.operands {
        write_operand(w, op)?;
    }
    Ok(())
}

pub fn read_instruction<R: Read>(r: &mut R) -> Result<Instruction, SerError> {
    let opcode_str = read_string(r)?;
    let opcode = Opcode::from_str(&opcode_str)
        .ok_or_else(|| SerError::InvalidEnumTag(opcode_str))?;
    let count = read_u64(r)? as usize;
    if count > 1024 {
        return Err(SerError::InvalidTag(format!("Operand count {} exceeds limit 1024", count)));
    }
    let mut operands = Vec::with_capacity(count);
    for _ in 0..count {
        operands.push(read_operand(r)?);
    }
    Ok(Instruction { opcode, operands })
}

pub fn write_function<W: Write>(w: &mut W, f: &Function) -> Result<(), SerError> {
    write_string(w, &f.name)?;
    write_u64(w, f.instructions.len() as u64)?;
    for inst in &f.instructions {
        write_instruction(w, inst)?;
    }
    write_u64(w, f.register_count as u64)?;
    write_u64(w, f.argument_count as u64)?;
    let fn_type_str = match f.fn_type {
        FunctionType::Regular => "Regular",
        FunctionType::Native => "Native",
        FunctionType::Ffi(_) => "Ffi",
        FunctionType::Closure => "Closure",
    };
    write_string(w, fn_type_str)?;
    write_string(w, "Regular")?; // data tag
    write_u64(w, 1)?; // ref_count
    Ok(())
}

pub fn read_function<R: Read>(r: &mut R) -> Result<Function, SerError> {
    let name = read_string(r)?;
    let inst_count = read_u64(r)? as usize;
    if inst_count > 1_000_000 {
        return Err(SerError::InvalidTag(format!("Instruction count {} exceeds limit 1,000,000", inst_count)));
    }
    let mut instructions = Vec::with_capacity(inst_count);
    for _ in 0..inst_count {
        instructions.push(read_instruction(r)?);
    }
    let register_count = read_u64(r)? as usize;
    let argument_count = read_u64(r)? as usize;
    let fn_type_tag = read_string(r)?;
    let fn_type = match fn_type_tag.as_str() {
        "Regular" => FunctionType::Regular,
        "Native" => FunctionType::Native,
        "Ffi" => FunctionType::Ffi(name.clone()),
        "Closure" => FunctionType::Closure,
        _ => FunctionType::Regular,
    };
    let _data_tag = read_string(r)?;
    let _ref_count = read_u64(r)?;
    Ok(Function {
        name,
        instructions,
        register_count,
        argument_count,
        return_type: None,
        bound_this: None,
        fn_type,
        call_counter: 0,
        jit_compiled_fn: None,
        closure_env: None,
    })
}

pub fn write_bytecode<W: Write>(w: &mut W, bc: &Bytecode) -> Result<(), SerError> {
    write_u64(w, bc.functions.len() as u64)?;
    for f in &bc.functions {
        write_function(w, f)?;
    }
    write_string(w, &bc.entry_point)?;
    write_u64(w, bc.native_declarations.len() as u64)?;
    for decl in &bc.native_declarations {
        write_string(w, &decl.lib_name)?;
        write_string(w, &decl.namespace)?;
    }
    let object_names: HashMap<usize, String> = bc.object_inits.iter()
        .map(|(name, object)| (Arc::as_ptr(object) as usize, name.clone()))
        .collect();
    let mut ordered_objects = Vec::with_capacity(bc.object_inits.len());
    let mut remaining: Vec<usize> = (0..bc.object_inits.len()).collect();
    let mut written = HashSet::new();
    while !remaining.is_empty() {
        let ready_idx = remaining.iter().position(|&idx| {
            let object = bc.object_inits[idx].1.read().unwrap();
            match &object.prototype {
                None => true,
                Some(proto) => {
                    let proto_ptr = Arc::as_ptr(proto) as usize;
                    object_names
                        .get(&proto_ptr)
                        .and_then(|proto_name| bc.object_inits.iter().position(|(name, _)| name == proto_name))
                        .is_none_or(|dependency_idx| written.contains(&dependency_idx))
                }
            }
        });
        let idx = ready_idx.map(|pos| remaining.remove(pos)).unwrap_or_else(|| remaining.remove(0));
        written.insert(idx);
        ordered_objects.push(idx);
    }

    write_u64(w, ordered_objects.len() as u64)?;
    for idx in ordered_objects {
        let (name, object) = &bc.object_inits[idx];
        write_string(w, name)?;
        write_load_time_object(w, &object.read().unwrap(), &object_names)?;
    }
    Ok(())
}

pub fn read_bytecode<R: Read>(r: &mut R) -> Result<Bytecode, SerError> {
    let count = read_u64(r)? as usize;
    if count > 100_000 {
        return Err(SerError::InvalidTag(format!("Function count {} exceeds limit 100,000", count)));
    }
    let mut functions = Vec::with_capacity(count);
    for _ in 0..count {
        functions.push(read_function(r)?);
    }
    let entry_point = read_string(r)?;
    let native_declarations = match read_u64(r) {
        Ok(decl_count) => {
            let mut decls = Vec::with_capacity(decl_count as usize);
            for _ in 0..decl_count {
                let lib_name = read_string(r)?;
                let namespace = read_string(r)?;
                decls.push(crate::bytecode::instruction::NativeDeclaration { lib_name, namespace });
            }
            decls
        }
        Err(_) => Vec::new(),
    };

    let object_inits = match read_u64(r) {
        Ok(init_count) => {
            let mut inits = Vec::with_capacity(init_count as usize);
            let mut objects = HashMap::new();
            for _ in 0..init_count {
                let name = read_string(r)?;
                let object = Arc::new(RwLock::new(read_load_time_object(r, &objects)?));
                objects.insert(name.clone(), Arc::clone(&object));
                inits.push((name, object));
            }
            inits
        }
        Err(_) => Vec::new(),
    };

    Ok(Bytecode {
        functions,
        entry_point,
        native_declarations,
        object_inits,
    })
}
