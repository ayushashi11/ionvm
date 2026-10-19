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
            OperandType::Pattern(_) => write!(w, "pattern")?,
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
