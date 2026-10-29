//! Range-proven SIMD lowering for simple integer reductions.

use std::collections::{HashMap, HashSet};

use crate::bytecode::instruction::{Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType, Value};

use super::analysis::*;
use super::loop_transforms::*;
use super::SIMD_WIDTH;

#[derive(Clone, Copy, Debug)]
pub(super) struct IntRange {
    lo: i128,
    hi: i128,
}

impl IntRange {
    fn point(value: i64) -> Self {
        Self { lo: value as i128, hi: value as i128 }
    }

    fn fits_i64(self) -> bool {
        self.lo >= i64::MIN as i128 && self.hi <= i64::MAX as i128
    }
}

pub(super) fn simple_integer_preheader_constants(func: &Function, head: usize) -> Option<Vec<Option<i64>>> {
    // Intentionally restrictive first implementation: it only optimizes loops
    // preceded by straight-line integer LoadConst instructions. This proves the
    // initial IV, bound, and accumulator values without speculative assumptions.
    let mut constants = vec![None; register_count_needed(func)];
    for inst in func.instructions.get(..head)? {
        if inst.opcode != Opcode::LoadConst {
            return None;
        }
        let value = constant_int(inst.operands.first()?)?;
        let dst = is_register(inst.operands.get(1))?;
        *constants.get_mut(dst)? = Some(value);
    }
    Some(constants)
}

pub(super) fn range_for_reduction_operand(
    op: &OperandType,
    ranges: &[Option<IntRange>],
    induction: usize,
    accumulator: usize,
    body_defs: &HashSet<usize>,
    defined_in_body: &HashSet<usize>,
) -> Option<IntRange> {
    match op {
        OperandType::Constant(Value::Int(value)) => Some(IntRange::point(*value)),
        OperandType::Register(reg) if *reg == accumulator => None,
        OperandType::Register(reg) if body_defs.contains(reg) && !defined_in_body.contains(reg) => None,
        OperandType::Register(reg) => ranges.get(*reg).copied().flatten().or_else(|| {
            if *reg == induction { ranges.get(induction).copied().flatten() } else { None }
        }),
        _ => None,
    }
}

pub(super) fn integer_binary_range(opcode: Opcode, a: IntRange, b: IntRange) -> Option<IntRange> {
    let (lo, hi) = match opcode {
        Opcode::IAddRRR | Opcode::IAddRCR => (
            a.lo.checked_add(b.lo)?,
            a.hi.checked_add(b.hi)?,
        ),
        Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR => (
            a.lo.checked_sub(b.hi)?,
            a.hi.checked_sub(b.lo)?,
        ),
        Opcode::IMulRRR | Opcode::IMulRCR => {
            let products = [
                a.lo.checked_mul(b.lo)?,
                a.lo.checked_mul(b.hi)?,
                a.hi.checked_mul(b.lo)?,
                a.hi.checked_mul(b.hi)?,
            ];
            (*products.iter().min()?, *products.iter().max()?)
        }
        _ => return None,
    };
    let result = IntRange { lo, hi };
    result.fits_i64().then_some(result)
}

pub(super) fn reduction_vector_opcode(opcode: Opcode) -> Option<Opcode> {
    match opcode {
        Opcode::IAddRRR | Opcode::IAddRCR => Some(Opcode::VAddI),
        Opcode::ISubRRR | Opcode::ISubRCR | Opcode::ISubCRR => Some(Opcode::VSubI),
        Opcode::IMulRRR | Opcode::IMulRCR => Some(Opcode::VMulI),
        _ => None,
    }
}

pub(super) fn build_integer_reduction_simd_prefix(
    func: &Function,
    head: usize,
    back: usize,
    types: &TypeAnalysis,
) -> Option<(Vec<Instruction>, usize)> {
    if head == 0 || back <= head + 3 || back >= func.instructions.len() {
        return None;
    }
    if !all_offsets_are_relocatable(func) || has_external_entry_into_loop(func, head, back) {
        return None;
    }

    let instrs = &func.instructions;
    let constants = simple_integer_preheader_constants(func, head)?;
    let header = instrs.get(head)?;
    if !matches!(header.opcode, Opcode::Lt | Opcode::ILtRRR | Opcode::ILtRCR) {
        return None;
    }
    let induction = is_register(header.operands.first())?;
    let condition = is_register(header.operands.get(2))?;
    let limit_op = header.operands.get(1)?.clone();
    let header_state = types.before.get(head)?.as_ref()?;
    if operand_type(&OperandType::Register(induction), header_state) != ValType::Int
        || operand_type(&limit_op, header_state) != ValType::Int
    {
        return None;
    }
    let start = constants.get(induction).copied().flatten()?;
    let limit = match &limit_op {
        OperandType::Register(reg) => constants.get(*reg).copied().flatten()?,
        OperandType::Constant(Value::Int(value)) => *value,
        _ => return None,
    };
    if start < 0 || limit < 0 || limit <= start {
        return None;
    }
    let trips = i128::from(limit).checked_sub(i128::from(start))?;
    if trips < i128::from(SIMD_WIDTH) {
        return None;
    }
    if condition == induction || matches!(&limit_op, OperandType::Register(reg) if *reg == condition) {
        return None;
    }

    let branch = instrs.get(head + 1)?;
    if !matches!(branch.opcode, Opcode::JumpFalse | Opcode::IJumpFalseR)
        || is_register(branch.operands.first()) != Some(condition)
        || direct_branch_target(branch, head + 1, instrs.len())? != back + 1
    {
        return None;
    }
    let back_inst = instrs.get(back)?;
    if back_inst.opcode != Opcode::Jump
        || direct_branch_target(back_inst, back, instrs.len())? != head
    {
        return None;
    }
    let increment = instrs.get(back - 1)?;
    if increment.opcode != Opcode::IAddRCR
        || is_register(increment.operands.first()) != Some(induction)
        || constant_int(increment.operands.get(1)?)? != 1
        || is_register(increment.operands.get(2)) != Some(induction)
    {
        return None;
    }
    let body = instrs.get(head + 2..back - 1)?;
    if body.is_empty() {
        return None;
    }

    // Find one final scalar integer addition of an independent term into the
    // reduction accumulator: acc = acc + term (or term + acc).
    let reductions: Vec<(usize, usize, OperandType)> = body.iter().enumerate().filter_map(|(idx, inst)| {
        if inst.opcode != Opcode::IAddRRR || inst.operands.len() < 3 {
            return None;
        }
        let dst = is_register(inst.operands.get(2))?;
        let lhs = is_register(inst.operands.first());
        let rhs = is_register(inst.operands.get(1));
        match (lhs == Some(dst), rhs == Some(dst)) {
            (true, false) => Some((idx, dst, inst.operands.get(1)?.clone())),
            (false, true) => Some((idx, dst, inst.operands.first()?.clone())),
            _ => None,
        }
    }).collect();
    if reductions.len() != 1 {
        return None;
    }
    let (reduction_idx, accumulator, term_op) = reductions.into_iter().next()?;
    if reduction_idx + 1 != body.len()
        || accumulator == induction
        || accumulator == condition
        || matches!(&limit_op, OperandType::Register(reg) if *reg == accumulator)
        || matches!(&term_op, OperandType::Register(reg) if *reg == accumulator)
    {
        return None;
    }
    let accumulator_start = constants.get(accumulator).copied().flatten()?;
    if operand_type(&OperandType::Register(accumulator), header_state) != ValType::Int {
        return None;
    }

    // Check that the condition register and per-iteration temporaries are not
    // observed after the loop. We only update the IV and reduction scalar in
    // the SIMD prefix; all other scalar body registers would otherwise be stale
    // when the vector batch consumes the whole trip count.
    let mut body_defs = HashSet::<usize>::new();
    for inst in body {
        for dst in instruction_written_registers(inst) {
            if dst == induction || dst == condition
                || matches!(&limit_op, OperandType::Register(reg) if *reg == dst)
            {
                return None;
            }
            body_defs.insert(dst);
        }
    }
    if !body_defs.contains(&accumulator) {
        return None;
    }
    for reg in body_defs.iter().copied().filter(|reg| *reg != accumulator).chain([condition]) {
        if instrs.iter().enumerate().any(|(pc, inst)| pc > back && instruction_mentions_register(inst, reg)) {
            return None;
        }
    }
    if body.iter().enumerate().any(|(idx, inst)| {
        idx != reduction_idx && instruction_reads(inst).contains(&accumulator)
    }) {
        return None;
    }
    if body.iter().enumerate().any(|(idx, inst)| {
        idx != reduction_idx && instruction_written_registers(inst).contains(&accumulator)
    }) {
        return None;
    }

    // The type/interval proof is deliberately limited to straight-line integer
    // expressions. It proves no scalar op, per-batch reduction, or accumulator
    // addition can overflow in debug builds; otherwise this pass declines.
    let mut ranges: Vec<Option<IntRange>> = constants.iter().map(|v| v.map(IntRange::point)).collect();
    if induction >= ranges.len() { return None; }
    ranges[induction] = Some(IntRange { lo: i128::from(start), hi: i128::from(limit - 1) });
    let mut defined_in_body = HashSet::<usize>::new();
    for inst in &body[..reduction_idx] {
        let vector_opcode = reduction_vector_opcode(inst.opcode)?;
        let a = inst.operands.first()?;
        let b = inst.operands.get(1)?;
        let dst = is_register(inst.operands.get(2))?;
        if dst == accumulator || dst == induction || dst == condition
            || matches!(&limit_op, OperandType::Register(reg) if *reg == dst)
        {
            return None;
        }
        let ar = range_for_reduction_operand(a, &ranges, induction, accumulator, &body_defs, &defined_in_body)?;
        let br = range_for_reduction_operand(b, &ranges, induction, accumulator, &body_defs, &defined_in_body)?;
        let dst_range = integer_binary_range(inst.opcode, ar, br)?;
        ranges[dst] = Some(dst_range);
        defined_in_body.insert(dst);
    }
    // The reduction term must be available from this iteration's IV, a prior
    // temporary definition, or a compile-time-proven loop invariant.
    if let OperandType::Register(reg) = &term_op {
        if body_defs.contains(reg) && !defined_in_body.contains(reg) {
            return None;
        }
    }
    let term_range = range_for_reduction_operand(
        &term_op, &ranges, induction, accumulator, &body_defs, &defined_in_body,
    )?;
    if !term_range.fits_i64() {
        return None;
    }
    let max_abs_term = term_range.lo.abs().max(term_range.hi.abs());
    let worst_total_delta = max_abs_term.checked_mul(trips)?;
    let acc_min = i128::from(accumulator_start).checked_sub(worst_total_delta)?;
    let acc_max = i128::from(accumulator_start).checked_add(worst_total_delta)?;
    let worst_batch_sum = max_abs_term.checked_mul(i128::from(SIMD_WIDTH))?;
    if acc_min < i64::MIN as i128 || acc_max > i64::MAX as i128
        || worst_batch_sum > i64::MAX as i128
    {
        return None;
    }

    // Allocate one vector register per scalar temporary. The accumulator stays
    // scalar: each batch is reduced with VSumI and added once to the accumulator.
    let mut next_reg = func.register_count.max(register_count_needed(func));
    let vector_induction = alloc_temp(&mut next_reg);
    let remaining = alloc_temp(&mut next_reg);
    let vector_condition = alloc_temp(&mut next_reg);
    let batch_sum = alloc_temp(&mut next_reg);
    let mut vector_registers = HashMap::<usize, usize>::new();
    for reg in body_defs.iter().copied().filter(|reg| *reg != accumulator) {
        vector_registers.insert(reg, alloc_temp(&mut next_reg));
    }

    let mut prefix = Vec::<Instruction>::new();
    let mut constant_splats = HashMap::<i64, usize>::new();
    fn vector_operand(
        op: &OperandType,
        induction: usize,
        vector_induction: usize,
        accumulator: usize,
        body_defs: &HashSet<usize>,
        defined_in_body: &HashSet<usize>,
        constants: &[Option<i64>],
        vector_registers: &HashMap<usize, usize>,
        constant_splats: &mut HashMap<i64, usize>,
        next_reg: &mut usize,
        setup: &mut Vec<Instruction>,
    ) -> Option<OperandType> {
        let constant = match op {
            OperandType::Constant(Value::Int(value)) => Some(*value),
            OperandType::Register(reg) if *reg == induction => {
                return Some(OperandType::Register(vector_induction));
            }
            OperandType::Register(reg) if *reg == accumulator => return None,
            OperandType::Register(reg) if body_defs.contains(reg) => {
                if !defined_in_body.contains(reg) { return None; }
                return Some(OperandType::Register(*vector_registers.get(reg)?));
            }
            OperandType::Register(reg) => constants.get(*reg).copied().flatten(),
            _ => None,
        }?;
        let vector = if let Some(existing) = constant_splats.get(&constant) {
            *existing
        } else {
            let vector = alloc_temp(next_reg);
            setup.push(Instruction::new(Opcode::VSplatI, vec![
                OperandType::Constant(Value::Int(constant)),
                OperandType::Constant(Value::Int(SIMD_WIDTH)),
                OperandType::Register(vector),
            ]));
            constant_splats.insert(constant, vector);
            vector
        };
        Some(OperandType::Register(vector))
    }

    // Prepare the loop body first so any needed invariant splats are emitted
    // before the vector loop header and are not recomputed on each batch.
    let mut vector_body = Vec::<Instruction>::new();
    let mut vector_defined = HashSet::<usize>::new();
    for inst in &body[..reduction_idx] {
        let vector_opcode = reduction_vector_opcode(inst.opcode)?;
        let dst = is_register(inst.operands.get(2))?;
        let a = vector_operand(
            inst.operands.first()?, induction, vector_induction, accumulator,
            &body_defs, &vector_defined, &constants, &vector_registers,
            &mut constant_splats, &mut next_reg, &mut prefix,
        )?;
        let b = vector_operand(
            inst.operands.get(1)?, induction, vector_induction, accumulator,
            &body_defs, &vector_defined, &constants, &vector_registers,
            &mut constant_splats, &mut next_reg, &mut prefix,
        )?;
        let vector_dst = *vector_registers.get(&dst)?;
        vector_body.push(Instruction::new(vector_opcode, vec![
            a, b, OperandType::Register(vector_dst),
        ]));
        vector_defined.insert(dst);
    }
    let vector_term = vector_operand(
        &term_op, induction, vector_induction, accumulator,
        &body_defs, &vector_defined, &constants, &vector_registers,
        &mut constant_splats, &mut next_reg, &mut prefix,
    )?;

    let vector_head = prefix.len();
    match &limit_op {
        OperandType::Register(limit_reg) => prefix.push(Instruction::new(Opcode::ISubRRR, vec![
            OperandType::Register(*limit_reg), OperandType::Register(induction), OperandType::Register(remaining),
        ])),
        OperandType::Constant(Value::Int(limit_value)) => prefix.push(Instruction::new(Opcode::ISubCRR, vec![
            OperandType::Constant(Value::Int(*limit_value)), OperandType::Register(induction), OperandType::Register(remaining),
        ])),
        _ => return None,
    }
    prefix.push(Instruction::new(Opcode::IGtRCR, vec![
        OperandType::Register(remaining), OperandType::Constant(Value::Int(SIMD_WIDTH - 1)), OperandType::Register(vector_condition),
    ]));
    let exit_branch = prefix.len();
    prefix.push(Instruction::new(Opcode::IJumpFalseR, vec![
        OperandType::Register(vector_condition), OperandType::Offset(0),
    ]));
    prefix.push(Instruction::new(Opcode::VIotaI, vec![
        OperandType::Register(induction), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(vector_induction),
    ]));
    prefix.extend(vector_body);
    prefix.push(Instruction::new(Opcode::VSumI, vec![
        vector_term, OperandType::Register(batch_sum),
    ]));
    prefix.push(Instruction::new(Opcode::IAddRRR, vec![
        OperandType::Register(accumulator), OperandType::Register(batch_sum), OperandType::Register(accumulator),
    ]));
    prefix.push(Instruction::new(Opcode::IAddRCR, vec![
        OperandType::Register(induction), OperandType::Constant(Value::Int(SIMD_WIDTH)), OperandType::Register(induction),
    ]));
    let batch_back = prefix.len();
    prefix.push(Instruction::new(Opcode::Jump, vec![
        OperandType::Offset(vector_head as i64 - batch_back as i64),
    ]));
    let scalar_loop_head = prefix.len();
    prefix[exit_branch].operands[1] = OperandType::Offset(scalar_loop_head as i64 - exit_branch as i64);
    Some((prefix, next_reg))
}

pub(super) fn vectorize_integer_reduction_loops(
    func: &mut Function,
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
) -> bool {
    let mut changed = false;
    let mut scan_from = 0usize;
    loop {
        let analysis = analyze_types(func, returns, argument_types);
        let mut selected: Option<(usize, usize, Vec<Instruction>, usize)> = None;
        for back in scan_from..func.instructions.len() {
            let inst = &func.instructions[back];
            if inst.opcode != Opcode::Jump { continue; }
            let Some(OperandType::Offset(off)) = inst.operands.first() else { continue };
            if *off >= 0 { continue; }
            let Some(head) = target_from_offset(back, *off, func.instructions.len()) else { continue };
            if head >= back { continue; }
            if let Some((prefix, new_count)) = build_integer_reduction_simd_prefix(func, head, back, &analysis) {
                selected = Some((head, back, prefix, new_count));
                break;
            }
        }
        let Some((head, back, prefix, new_count)) = selected else { break };
        let prefix_len = prefix.len();
        if !insert_prefix_with_relocation(func, head, prefix, new_count) { break; }
        changed = true;
        // The original scalar loop remains after the prefix for the tail. Skip
        // its back edge on the next search so we don't insert the same prefix again.
        scan_from = back.saturating_add(prefix_len).saturating_add(1);
    }
    changed
}

