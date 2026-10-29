//! Conservative, semantics-preserving IonVM bytecode optimizer.
//!
//! The implementation is split into analysis and transformation passes.
//! Unsupported or unproven transformations leave the original bytecode intact.

use std::collections::{HashMap, HashSet};

use crate::bytecode::instruction::{Bytecode, Instruction, OperandType};
use crate::bytecode::opcode::Opcode;
use crate::lang::types::{Function, ValType};
#[cfg(test)]
use crate::lang::types::Value;

pub(super) const SIMD_WIDTH: i64 = 4;

mod analysis;
mod loop_transforms;
mod array_analysis;
mod predicated_simd;
mod integer_reduction;
#[cfg(test)]
mod tests;

// Import only within this optimizer module tree; the public API remains the four
// entry points below. Helpers are pub(super), never publicly exposed by the crate.
use analysis::*;
use array_analysis::*;
use integer_reduction::*;
use loop_transforms::*;
use predicated_simd::*;

pub(super) fn vectorize_loops_with_types(
    func: &mut Function,
    returns: &HashMap<String, ValType>,
    argument_types: &[ValType],
    safe_readonly_array_args: &HashSet<usize>,
) -> bool {
    // Scalar typed arithmetic may panic on overflow in debug builds while the
    // current vector helpers wrap. Keep every loop scalar until both execution
    // paths share the same overflow contract.
    if cfg!(debug_assertions) { return false; }
    let mut changed = false;
    let mut scan_from = 0usize;

    loop {
        if scan_from >= func.instructions.len() { break; }
        let analysis = analyze_types(func, returns, argument_types);
        let mut transformed = false;
        let mut pc = scan_from;
        while pc < func.instructions.len() {
            let inst = &func.instructions[pc];
            if inst.opcode == Opcode::Jump {
                if let Some(OperandType::Offset(off)) = inst.operands.first() {
                    if *off < 0 {
                        if let Some(head) = target_from_offset(pc, *off, func.instructions.len()) {
                            if head < pc {
                                let candidate = build_predicated_simd_prefix(
                                    func, head, pc, &analysis, argument_types, safe_readonly_array_args,
                                ).or_else(|| build_int_simd_prefix(func, head, pc, &analysis));
                                if let Some((prefix, new_count)) = candidate {
                                    let prefix_len = prefix.len();
                                    if insert_prefix_with_relocation(func, head, prefix, new_count) {
                                        changed = true;
                                        // Skip the transformed scalar loop so it is
                                        // not vectorized a second time on this run.
                                        scan_from = pc.saturating_add(prefix_len).saturating_add(1);
                                        transformed = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            pc += 1;
        }
        if !transformed { break; }
    }
    changed
}

/// Optimize one function. LLVM/native compilation is intentionally outside this
/// module; this function only rewrites bytecode before the VM's later JIT stage.
pub fn optimize_function(func: &mut Function) {
    optimize_function_with_types_and_args(func, &HashMap::new(), &unknown_argument_types(func), &HashSet::new());
}

pub(super) fn direct_callee_name(bytecode: &Bytecode, inst: &Instruction) -> Option<String> {
    match inst.opcode {
        Opcode::Call | Opcode::Spawn => match inst.operands.first()? {
            OperandType::Function(name) => Some(name.clone()),
            _ => None,
        },
        Opcode::CallJit => {
            let OperandType::Offset(index) = inst.operands.first()? else { return None };
            if *index < 0 { return None; }
            bytecode.functions.get(*index as usize).map(|f| f.name.clone())
        }
        _ => None,
    }
}

pub(super) fn direct_call_argument_count(inst: &Instruction) -> Option<usize> {
    if !matches!(inst.opcode, Opcode::Call | Opcode::CallJit | Opcode::Spawn) {
        return None;
    }
    match inst.operands.get(1)? {
        OperandType::Offset(count) if *count >= 0 => Some(*count as usize),
        _ => None,
    }
}

pub(super) fn infer_argument_types(
    bytecode: &Bytecode,
    returns: &HashMap<String, ValType>,
    current: &HashMap<String, Vec<ValType>>,
) -> HashMap<String, Vec<ValType>> {
    let mut next: HashMap<String, Vec<ValType>> = bytecode.functions.iter()
        .map(|f| (f.name.clone(), vec![ValType::Unknown; f.argument_count]))
        .collect();
    let mut observed: HashMap<String, Vec<Option<ValType>>> = bytecode.functions.iter()
        .map(|f| (f.name.clone(), vec![None; f.argument_count]))
        .collect();
    let mut conflicted: HashMap<String, Vec<bool>> = bytecode.functions.iter()
        .map(|f| (f.name.clone(), vec![false; f.argument_count]))
        .collect();
    let mut escaped = HashSet::<String>::new();

    // A Function operand used as a value/closure can be invoked without a
    // statically visible argument list. Do not infer that target's parameters.
    for func in &bytecode.functions {
        for inst in &func.instructions {
            for (idx, op) in inst.operands.iter().enumerate() {
                if let OperandType::Function(name) = op {
                    let is_direct_target = idx == 0 && matches!(inst.opcode, Opcode::Call | Opcode::Spawn);
                    if !is_direct_target {
                        escaped.insert(name.clone());
                    }
                }
            }
        }
    }
    // The module entry point may be invoked by the host with unknown arguments.
    if let Some(entry) = bytecode.functions.first() {
        escaped.insert(entry.name.clone());
    }
    if let Some(main) = bytecode.functions.iter().find(|f| f.name == "main") {
        escaped.insert(main.name.clone());
    }

    for caller in &bytecode.functions {
        let caller_args = current.get(&caller.name).cloned().unwrap_or_else(|| unknown_argument_types(caller));
        let analysis = analyze_types(caller, returns, &caller_args);
        for (pc, inst) in caller.instructions.iter().enumerate() {
            let Some(callee) = direct_callee_name(bytecode, inst) else { continue };
            let Some(argc) = direct_call_argument_count(inst) else {
                escaped.insert(callee);
                continue;
            };
            let Some(arg_slots) = observed.get_mut(&callee) else { continue };
            let Some(conflict_slots) = conflicted.get_mut(&callee) else { continue };
            let state = analysis.before.get(pc).and_then(|s| s.as_ref());
            for arg_idx in 0..arg_slots.len() {
                if arg_idx >= argc {
                    conflict_slots[arg_idx] = true;
                    continue;
                }
                let ty = match state {
                    Some(state) => inst.operands.get(2 + arg_idx)
                        .map(|op| operand_type_with_args(op, state, &caller_args))
                        .unwrap_or(ValType::Unknown),
                    None => ValType::Unknown,
                };
                if ty == ValType::Unknown {
                    conflict_slots[arg_idx] = true;
                    continue;
                }
                match arg_slots[arg_idx] {
                    None => arg_slots[arg_idx] = Some(ty),
                    Some(existing) if existing == ty => {},
                    Some(_) => conflict_slots[arg_idx] = true,
                }
            }
            // A target with fewer arguments than its declared arity cannot have
            // a single proven type signature for this call graph.
            if argc < arg_slots.len() {
                for conflict in conflict_slots.iter_mut() { *conflict = true; }
            }
        }
    }

    for func in &bytecode.functions {
        let Some(dst) = next.get_mut(&func.name) else { continue };
        let Some(obs) = observed.get(&func.name) else { continue };
        let Some(conflicts) = conflicted.get(&func.name) else { continue };
        if escaped.contains(&func.name) { continue; }
        for idx in 0..dst.len() {
            if !conflicts[idx] {
                if let Some(ty) = obs[idx] { dst[idx] = ty; }
            }
        }
    }
    next
}

pub(super) fn optimize_function_with_types_and_args(
    func: &mut Function,
    func_return_types: &HashMap<String, ValType>,
    argument_types: &[ValType],
    safe_readonly_array_args: &HashSet<usize>,
) {
    let _ = remove_redundant_copy_roundtrips(func);
    let _ = specialize_types(func, func_return_types, argument_types);
    let _ = unroll_small_constant_loops(func);
    let _ = specialize_types(func, func_return_types, argument_types);
    // Handles canonical pure integer reductions even when the loop has no array
    // access. Unlike the more general SIMD passes, this pass is also enabled in
    // debug builds, but only after a static range proof rules out overflow.
    let _ = vectorize_integer_reduction_loops(func, func_return_types, argument_types);
    let _ = vectorize_loops_with_types(func, func_return_types, argument_types, safe_readonly_array_args);
    let final_analysis = analyze_types(func, func_return_types, argument_types);
    if func.return_type.is_none() {
        if let Some(ret) = final_analysis.inferred_return {
            func.return_type = Some(ret);
        }
    }
}

/// Infer internal argument and return types to a fixed point, then optimize
/// each function once. Argument types only become facts when every statically
/// visible call site agrees and the function does not escape as a first-class
/// value; otherwise they remain Unknown.
pub fn optimize_bytecode(bytecode: &mut Bytecode) {
    let mut returns: HashMap<String, ValType> = bytecode.functions.iter()
        .filter_map(|f| f.return_type.map(|ty| (f.name.clone(), ty)))
        .collect();
    let mut argument_types: HashMap<String, Vec<ValType>> = bytecode.functions.iter()
        .map(|f| (f.name.clone(), unknown_argument_types(f)))
        .collect();

    for _ in 0..=bytecode.functions.len().saturating_mul(2).saturating_add(2) {
        let next_args = infer_argument_types(bytecode, &returns, &argument_types);
        let mut changed = next_args != argument_types;
        argument_types = next_args;

        for func in &bytecode.functions {
            if returns.contains_key(&func.name) { continue; }
            let args = argument_types.get(&func.name).cloned().unwrap_or_else(|| unknown_argument_types(func));
            let analysis = analyze_types(func, &returns, &args);
            if let Some(ret) = analysis.inferred_return {
                returns.insert(func.name.clone(), ret);
                changed = true;
            }
        }
        if !changed { break; }
    }

    let local_readonly = local_readonly_array_parameters(bytecode);
    let safe_array_args = array_argument_origins_are_safe(bytecode, &returns, &argument_types, &local_readonly);

    for func in &mut bytecode.functions {
        let args = argument_types.get(&func.name).cloned().unwrap_or_else(|| unknown_argument_types(func));
        let safe_arrays = safe_array_args.get(&func.name).cloned().unwrap_or_default();
        optimize_function_with_types_and_args(func, &returns, &args, &safe_arrays);
    }
}

/// Public compatibility entry point retained for callers that already know
/// callee return types but cannot provide whole-bytecode argument facts.
pub fn optimize_function_with_types(func: &mut Function, func_return_types: &HashMap<String, ValType>) {
    let args = unknown_argument_types(func);
    optimize_function_with_types_and_args(func, func_return_types, &args, &HashSet::new());
}

/// Attempt conservative four-lane vectorization of supported integer-array loops.
/// This includes a predicated CFG path for pure nested loops when the argument
/// provenance and every operation's legality can be proved. Unsupported patterns
/// remain unchanged and continue to execute using the original scalar bytecode.
pub fn vectorize_loops(func: &mut Function) -> bool {
    vectorize_loops_with_types(func, &HashMap::new(), &unknown_argument_types(func), &HashSet::new())
}

