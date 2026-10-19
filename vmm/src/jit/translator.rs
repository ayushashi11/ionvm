use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, NativeJitFn};
use super::llvm_translator;

pub fn compile_function(func: &Function) -> Option<NativeJitFn> {
    for inst in &func.instructions {
        match inst.opcode {
            Opcode::IAddRRR
            | Opcode::IAddRCR
            | Opcode::ISubRRR
            | Opcode::IMulRRR
            | Opcode::IDivRCR
            | Opcode::IRemRCR
            | Opcode::IEqRRR
            | Opcode::ILtRRR
            | Opcode::IGtRRR
            | Opcode::IGtRCR
            | Opcode::IGtJmpRR
            | Opcode::IGtJmpRC
            | Opcode::ILtJmpRR
            | Opcode::ILtJmpRC
            | Opcode::IEqJmpRR
            | Opcode::IEqJmpRC
            | Opcode::IJumpTrueR
            | Opcode::IJumpFalseR
            | Opcode::AGetIndexAR
            | Opcode::FAddRRR
            | Opcode::FSubRRR
            | Opcode::FMulRRR
            | Opcode::FDivRRR
            | Opcode::FEqRRR
            | Opcode::FLtRRR
            | Opcode::FGtRRR
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::LoadConst
            | Opcode::Ret
            | Opcode::Eq
            | Opcode::JumpTrue
            | Opcode::JumpFalse
            | Opcode::Jump
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Rem
            | Opcode::GetIndex
            | Opcode::ArrayGet
            | Opcode::ArraySet
            | Opcode::IArrayGet
            | Opcode::IArraySet
            | Opcode::FArrayGet
            | Opcode::FArraySet
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Not
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::IShlRRR
            | Opcode::IShlRCR
            | Opcode::IShrRRR
            | Opcode::IShrRCR
            | Opcode::IAndRRR
            | Opcode::IAndRCR
            | Opcode::IOrRRR
            | Opcode::IOrRCR
            | Opcode::IXorRRR
            | Opcode::IXorRCR
            | Opcode::IBitGet
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0 => {}
            _ => {
                return None;
            }
        }
    }

    llvm_translator::compile_with_llvm(func)
}

pub fn compile_function_with_ir(func: &Function) -> Option<(NativeJitFn, String)> {
    for inst in &func.instructions {
        match inst.opcode {
            Opcode::IAddRRR
            | Opcode::IAddRCR
            | Opcode::ISubRRR
            | Opcode::IMulRRR
            | Opcode::IDivRCR
            | Opcode::IRemRCR
            | Opcode::IEqRRR
            | Opcode::ILtRRR
            | Opcode::IGtRRR
            | Opcode::IGtRCR
            | Opcode::IGtJmpRR
            | Opcode::IGtJmpRC
            | Opcode::ILtJmpRR
            | Opcode::ILtJmpRC
            | Opcode::IEqJmpRR
            | Opcode::IEqJmpRC
            | Opcode::IJumpTrueR
            | Opcode::IJumpFalseR
            | Opcode::AGetIndexAR
            | Opcode::FAddRRR
            | Opcode::FSubRRR
            | Opcode::FMulRRR
            | Opcode::FDivRRR
            | Opcode::FEqRRR
            | Opcode::FLtRRR
            | Opcode::FGtRRR
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::LoadConst
            | Opcode::Ret
            | Opcode::Eq
            | Opcode::JumpTrue
            | Opcode::JumpFalse
            | Opcode::Jump
            | Opcode::Add
            | Opcode::Sub
            | Opcode::Mul
            | Opcode::Div
            | Opcode::Rem
            | Opcode::GetIndex
            | Opcode::ArrayGet
            | Opcode::ArraySet
            | Opcode::IArrayGet
            | Opcode::IArraySet
            | Opcode::FArrayGet
            | Opcode::FArraySet
            | Opcode::And
            | Opcode::Or
            | Opcode::Xor
            | Opcode::Not
            | Opcode::Shl
            | Opcode::Shr
            | Opcode::IShlRRR
            | Opcode::IShlRCR
            | Opcode::IShrRRR
            | Opcode::IShrRCR
            | Opcode::IAndRRR
            | Opcode::IAndRCR
            | Opcode::IOrRRR
            | Opcode::IOrRCR
            | Opcode::IXorRRR
            | Opcode::IXorRCR
            | Opcode::IBitGet
            | Opcode::IBitSet
            | Opcode::IBitSet1
            | Opcode::IBitSet0 => {}
            _ => {
                return None;
            }
        }
    }

    llvm_translator::compile_with_llvm_ir(func)
}

pub fn get_function_llvm_ir(func: &Function) -> Option<String> {
    compile_function_with_ir(func).map(|(_, ir)| ir)
}
