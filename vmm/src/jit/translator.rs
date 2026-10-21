use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, NativeJitFn};
use super::llvm_translator;

pub fn compile_function(func: &Function) -> Option<NativeJitFn> {
    for inst in &func.instructions {
        match inst.opcode {
            Opcode::IAddRRR
            | Opcode::IAddRCR
            | Opcode::ISubRRR
            | Opcode::ISubRCR
            | Opcode::ISubCRR
            | Opcode::IMulRRR
            | Opcode::IMulRCR
            | Opcode::IDivRCR
            | Opcode::IDivCRR
            | Opcode::IRemRCR
            | Opcode::IRemCRR
            | Opcode::IEqRRR
            | Opcode::IEqRCR
            | Opcode::ILtRRR
            | Opcode::ILtRCR
            | Opcode::ILtCRR
            | Opcode::IGtRRR
            | Opcode::IGtRCR
            | Opcode::IGtCRR
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
            | Opcode::FAddRCR
            | Opcode::FSubRRR
            | Opcode::FSubRCR
            | Opcode::FSubCRR
            | Opcode::FMulRRR
            | Opcode::FMulRCR
            | Opcode::FDivRRR
            | Opcode::FDivRCR
            | Opcode::FDivCRR
            | Opcode::FEqRRR
            | Opcode::FEqRCR
            | Opcode::FLtRRR
            | Opcode::FLtRCR
            | Opcode::FLtCRR
            | Opcode::FGtRRR
            | Opcode::FGtRCR
            | Opcode::FGtCRR
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::LoadConst
            | Opcode::Ret
            | Opcode::Eq
            | Opcode::Lt
            | Opcode::Gt
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
            | Opcode::IBitSet0
            | Opcode::Match
            | Opcode::ETagSwitch
            | Opcode::ETagJmpRC
            | Opcode::AMatchFast
            | Opcode::Nop => {}
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
            | Opcode::ISubRCR
            | Opcode::ISubCRR
            | Opcode::IMulRRR
            | Opcode::IMulRCR
            | Opcode::IDivRCR
            | Opcode::IDivCRR
            | Opcode::IRemRCR
            | Opcode::IRemCRR
            | Opcode::IEqRRR
            | Opcode::IEqRCR
            | Opcode::ILtRRR
            | Opcode::ILtRCR
            | Opcode::ILtCRR
            | Opcode::IGtRRR
            | Opcode::IGtRCR
            | Opcode::IGtCRR
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
            | Opcode::FAddRCR
            | Opcode::FSubRRR
            | Opcode::FSubRCR
            | Opcode::FSubCRR
            | Opcode::FMulRRR
            | Opcode::FMulRCR
            | Opcode::FDivRRR
            | Opcode::FDivRCR
            | Opcode::FDivCRR
            | Opcode::FEqRRR
            | Opcode::FEqRCR
            | Opcode::FLtRRR
            | Opcode::FLtRCR
            | Opcode::FLtCRR
            | Opcode::FGtRRR
            | Opcode::FGtRCR
            | Opcode::FGtCRR
            | Opcode::FGtJmpRR
            | Opcode::FGtJmpRC
            | Opcode::FLtJmpRR
            | Opcode::FLtJmpRC
            | Opcode::LoadConst
            | Opcode::Ret
            | Opcode::Eq
            | Opcode::Lt
            | Opcode::Gt
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
            | Opcode::IBitSet0
            | Opcode::Match
            | Opcode::ETagSwitch
            | Opcode::ETagJmpRC
            | Opcode::AMatchFast
            | Opcode::Nop => {}
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
