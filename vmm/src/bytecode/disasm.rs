use std::io::{self, Write};
use crate::bytecode::instruction::{Bytecode, OperandType};
use crate::lang::types::Value;

pub fn print_disassembly<W: Write>(w: &mut W, bytecode: &Bytecode) -> io::Result<()> {
    print_disassembly_with_options(w, bytecode, false)
}

pub fn print_disassembly_with_options<W: Write>(w: &mut W, bytecode: &Bytecode, show_llvm: bool) -> io::Result<()> {
    writeln!(w, "================================================================================")?;
    writeln!(w, "BYTECODE DISASSEMBLY | Entry Point: '{}'", bytecode.entry_point)?;
    writeln!(w, "================================================================================\n")?;
    for native_decl in &bytecode.native_declarations {
        writeln!(
            w,
            "Native Declaration: Library '{}', Namespace '{}'",
            native_decl.lib_name, native_decl.namespace
        )?;
    }
    for (name, obj) in &bytecode.object_inits {
        let obj = obj.read().unwrap();
        writeln!(w, "Object Initialization: '{}'", name)?;
        writeln!(w, "  Property Count: {}", obj.properties.len())?;
        for (prop_name, prop_value) in &obj.properties {
            writeln!(w, "    Property: '{}' = {:?}", prop_name, prop_value)?;
        }
        writeln!(w)?;
    }
    for func in &bytecode.functions {
        writeln!(
            w,
            "Function '{}' (regs: {}, args: {}):",
            func.name, func.register_count, func.argument_count
        )?;
        let mut opt_func = func.clone();
        crate::vm::optimizer::optimize_function(&mut opt_func);

        for (pc, inst) in func.instructions.iter().enumerate() {
            write!(w, "  [{: >4}] {: <22} ", pc, inst.opcode.as_str())?;
            print_operands(w, &inst.operands)?;
            writeln!(w)?;
        }
        if opt_func.instructions != func.instructions {
            let is_vec = opt_func.instructions.len() > func.instructions.len();
            writeln!(w, "  ----------------------------------------------------------------------")?;
            writeln!(
                w,
                "  Optimized Bytecode (In-VM Loop Vectorization: {}):",
                if is_vec { "4-Lane SIMD Unrolled" } else { "Specialized" }
            )?;
            writeln!(w, "  ----------------------------------------------------------------------")?;
            for (pc, inst) in opt_func.instructions.iter().enumerate() {
                write!(w, "  | [{: >4}] {: <22} ", pc, inst.opcode.as_str())?;
                print_operands(w, &inst.operands)?;
                writeln!(w)?;
            }
        }
        if show_llvm {
            if let Some(ir) = crate::jit::translator::get_function_llvm_ir(&opt_func) {
                let has_simd = ir.contains('<') && (ir.contains("x i") || ir.contains("x double") || ir.contains("x float"));
                writeln!(w, "  ----------------------------------------------------------------------")?;
                writeln!(w, "  LLVM JIT IR for '{}' (SIMD Vectorization: {}):", func.name, if has_simd { "YES" } else { "NO (Scalar)" })?;
                writeln!(w, "  ----------------------------------------------------------------------")?;
                for line in ir.lines() {
                    writeln!(w, "  | {}", line)?;
                }
                writeln!(w)?;
            } else {
                writeln!(w, "  (Function '{}' not eligible for LLVM JIT compilation)\n", func.name)?;
            }
        }
        writeln!(w)?;
    }
    Ok(())
}

fn print_operands<W: Write>(w: &mut W, operands: &[OperandType]) -> io::Result<()> {
    for (i, op) in operands.iter().enumerate() {
        if i > 0 {
            write!(w, ", ")?;
        }
        match op {
            OperandType::Register(r) => write!(w, "r{}", r)?,
            OperandType::Constant(c) => match c {
                Value::Int(v) => write!(w, "{}", v)?,
                Value::Float(v) => write!(w, "{:.4}", v)?,
                Value::Bool(v) => write!(w, "{}", if *v { "true" } else { "false" })?,
                Value::String(v) => write!(w, "\"{}\"", v)?,
                Value::Nil => write!(w, "nil")?,
                _ => write!(w, "const")?,
            },
            OperandType::Function(f) => write!(w, "fn({})", f)?,
            OperandType::Offset(off) => write!(w, "off({})", off)?,
            OperandType::Argument(a) => write!(w, "arg{}", a)?,
            OperandType::Property(p) => write!(w, "prop(\"{}\")", p)?,
            OperandType::This => write!(w, "this")?,
            OperandType::SelfProc => write!(w, "self")?,
            OperandType::Pattern(pattern) => print_pattern(w, pattern)?,
            OperandType::Captured(c) => write!(w, "cap({})", c)?,
            OperandType::RegisterList(regs) => {
                write!(w, "[")?;
                for (idx, r) in regs.iter().enumerate() {
                    if idx > 0 {
                        write!(w, ", ")?;
                    }
                    write!(w, "r{}", r)?;
                }
                write!(w, "]")?;
            }
            OperandType::PropertyList(props) => {
                write!(w, "[")?;
                for (idx, p) in props.iter().enumerate() {
                    if idx > 0 {
                        write!(w, ", ")?;
                    }
                    write!(w, "\"{}\"", p)?;
                }
                write!(w, "]")?;
            }
            OperandType::ConstantList(consts) => {
                write!(w, "[")?;
                for (idx, c) in consts.iter().enumerate() {
                    if idx > 0 {
                        write!(w, ", ")?;
                    }
                    match c {
                        Value::Int(v) => write!(w, "{}", v)?,
                        Value::Float(v) => write!(w, "{:.4}", v)?,
                        Value::Bool(v) => write!(w, "{}", if *v { "true" } else { "false" })?,
                        Value::String(v) => write!(w, "\"{}\"", v)?,
                        _ => write!(w, "const")?,
                    }
                }
                write!(w, "]")?;
            }
            OperandType::PayloadKeyList(keys) => {
                write!(w, "[")?;
                for (idx, k) in keys.iter().enumerate() {
                    if idx > 0 {
                        write!(w, ", ")?;
                    }
                    print_operands(w, std::slice::from_ref(k))?;
                }
                write!(w, "]")?;
            }
            OperandType::JumpTable(cases) => {
                write!(w, "[")?;
                for (idx, c) in cases.iter().enumerate() {
                    if idx > 0 {
                        write!(w, ", ")?;
                    }
                    write!(w, "tag({}) => off({})", c.tag, c.offset)?;
                }
                write!(w, "]")?;
            }
        }
    }
    Ok(())
}

fn print_pattern<W: Write>(w: &mut W, pattern: &crate::lang::matcher::Pattern) -> io::Result<()> {
    use crate::lang::matcher::{ListSegment, Pattern};

    match pattern {
        Pattern::Wildcard => write!(w, "_"),
        Pattern::Literal(value) => print_pattern_value(w, value),
        Pattern::Bind(register) => write!(w, "bind(r{})", register),
        Pattern::Tuple(patterns) => {
            write!(w, "(")?;
            print_pattern_list(w, patterns)?;
            if patterns.len() == 1 {
                write!(w, ",")?;
            }
            write!(w, ")")
        }
        Pattern::Object(properties) => {
            write!(w, "object {{")?;
            for (index, (name, property_pattern)) in properties.iter().enumerate() {
                if index > 0 {
                    write!(w, ", ")?;
                } else {
                    write!(w, " ")?;
                }
                write!(w, "{:?}: ", name)?;
                print_pattern(w, property_pattern)?;
            }
            if !properties.is_empty() {
                write!(w, " ")?;
            }
            write!(w, "}}")
        }
        Pattern::IntRange(start, end, inclusive) => {
            print_range(w, start, end, *inclusive)
        }
        Pattern::FloatRange(start, end, inclusive) => {
            print_range(w, start, end, *inclusive)
        }
        Pattern::Array(array) | Pattern::IntArray(array) | Pattern::FloatArray(array) => {
            match pattern {
                Pattern::IntArray(_) => write!(w, "int")?,
                Pattern::FloatArray(_) => write!(w, "float")?,
                _ => {}
            }
            write!(w, "[")?;
            for (index, segment) in array.segments.iter().enumerate() {
                if index > 0 {
                    write!(w, ", ")?;
                }
                match segment {
                    ListSegment::Element(element) => print_pattern(w, element)?,
                    ListSegment::Elements(elements) => print_pattern_list(w, elements)?,
                    ListSegment::BindList(register) => write!(w, "..r{}", register)?,
                    ListSegment::WildcardList => write!(w, ".._")?,
                }
            }
            write!(w, "]")
        }
    }
}

fn print_pattern_list<W: Write>(
    w: &mut W,
    patterns: &[crate::lang::matcher::Pattern],
) -> io::Result<()> {
    for (index, pattern) in patterns.iter().enumerate() {
        if index > 0 {
            write!(w, ", ")?;
        }
        print_pattern(w, pattern)?;
    }
    Ok(())
}

fn print_range<T: std::fmt::Display, W: Write>(
    w: &mut W,
    start: &Option<T>,
    end: &Option<T>,
    inclusive: bool,
) -> io::Result<()> {
    if let Some(start) = start {
        write!(w, "{}", start)?;
    }
    write!(w, "..")?;
    if inclusive {
        write!(w, "=")?;
    }
    if let Some(end) = end {
        write!(w, "{}", end)?;
    }
    Ok(())
}

fn print_pattern_value<W: Write>(w: &mut W, value: &Value) -> io::Result<()> {
    match value {
        Value::Int(value) => write!(w, "{}", value),
        Value::Float(value) => write!(w, "{:?}", value),
        Value::Bool(value) => write!(w, "{}", value),
        Value::String(value) => write!(w, "{:?}", value),
        Value::Nil => write!(w, "nil"),
        Value::Object(_) => write!(w, "object"),
        Value::Function(function) => write!(w, "fn({})", function.read().unwrap().name),
        Value::Array(values) => write!(w, "array(len={})", values.read().unwrap().len()),
        Value::IntArray(values) => write!(w, "int_array(len={})", values.read().unwrap().len()),
        Value::FloatArray(values) => write!(w, "float_array(len={})", values.read().unwrap().len()),
        Value::V4Int(values) => write!(w, "v4int({:?})", values),
        Value::V4Float(values) => write!(w, "v4float({:?})", values),
        Value::V4Bool(values) => write!(w, "v4bool({:?})", values),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::matcher::{ArrayPattern, ListSegment, Pattern};

    #[test]
    fn prints_nested_match_patterns_with_binding_registers() {
        let pattern = Pattern::Object(vec![
            ("x".to_string(), Pattern::Bind(2)),
            (
                "coords".to_string(),
                Pattern::Tuple(vec![
                    Pattern::Literal(Value::Int(1)),
                    Pattern::Array(ArrayPattern::new(vec![
                        ListSegment::Element(Pattern::Bind(3)),
                        ListSegment::BindList(4),
                    ])),
                ]),
            ),
        ]);
        let mut output = Vec::new();
        print_operands(&mut output, &[OperandType::Pattern(pattern)]).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "object { \"x\": bind(r2), \"coords\": (1, [bind(r3), ..r4]) }"
        );
    }

    #[test]
    fn prints_ranges_and_singleton_tuples() {
        let mut output = Vec::new();
        print_operands(
            &mut output,
            &[OperandType::Pattern(Pattern::Tuple(vec![Pattern::FloatRange(
                Some(1.5),
                None,
                false,
            )]))],
        )
        .unwrap();
        assert_eq!(String::from_utf8(output).unwrap(), "(1.5..,)");
    }
}
