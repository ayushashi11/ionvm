//! Regression tests for optimizer passes and bytecode relocation.

    use super::*;

    #[test]
    fn simple_integer_reduction_loop_is_vectorized() {
        let loop_instrs = vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(1)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(10)), OperandType::Register(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(1), OperandType::Register(2), OperandType::Register(3)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(3), OperandType::Offset(5)]),
            Instruction::new(Opcode::Mul, vec![OperandType::Register(1), OperandType::Register(1), OperandType::Register(4)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(0), OperandType::Register(4), OperandType::Register(0)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(1), OperandType::Constant(Value::Int(1)), OperandType::Register(1)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-5)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        ];
        let mut func = Function::new(
            "sum_squares".to_string(), loop_instrs, 5, 0,
            crate::lang::types::FunctionType::Regular,
        );

        optimize_function(&mut func);

        assert!(func.instructions.len() > 10, "loop should gain a SIMD prefix");
        assert!(func.instructions.iter().any(|inst| inst.opcode == Opcode::VIotaI));
        assert!(func.instructions.iter().any(|inst| inst.opcode == Opcode::VMulI));
        assert!(func.instructions.iter().any(|inst| inst.opcode == Opcode::VSumI));
    }

    #[test]
    fn call_argument_count_is_not_treated_as_branch_target() {
        let call = Instruction::new(Opcode::Call, vec![
            OperandType::Function("callee".to_string()),
            OperandType::Offset(3),
            OperandType::Register(0),
            OperandType::Register(1),
            OperandType::Register(2),
            OperandType::Register(3),
        ]);
        assert_eq!(jump_offset_index(&call), None);
        assert!(all_offsets_are_relocatable(&Function::new(
            "f".to_string(), vec![call], 4, 0, crate::lang::types::FunctionType::Regular,
        )));
    }

    #[test]
    fn insertion_relocates_branches_from_before_the_insertion_point() {
        let mut f = Function::new("branch_over_prefix".to_string(), vec![
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(0), OperandType::Offset(2)]),
            Instruction::new(Opcode::Nop, vec![]),
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(0))]),
        ], 1, 0, crate::lang::types::FunctionType::Regular);

        assert!(insert_prefix_with_relocation(
            &mut f, 1, vec![Instruction::new(Opcode::Nop, vec![]), Instruction::new(Opcode::Nop, vec![])], 1,
        ));
        // Old target pc 2 is now pc 4, so the branch at pc 0 needs offset 4.
        assert_eq!(f.instructions[0].operands[1], OperandType::Offset(4));
    }

    #[test]
    fn unknown_call_result_invalidates_destination_type() {
        let mut state = vec![ValType::Int; 4];
        let call = Instruction::new(Opcode::Call, vec![
            OperandType::Function("dynamic".to_string()), OperandType::Offset(0), OperandType::Register(0),
        ]);
        transfer_types(&call, &mut state, &HashMap::new(), &[]);
        assert_eq!(state[0], ValType::Unknown);
        assert_eq!(state[1], ValType::Int);
    }

    #[test]
    fn receive_invalidates_register_type_facts() {
        // Receive-style operations may write a destination register, and this
        // module intentionally does not assume their operand schema.
        for opcode in [Opcode::Receive, Opcode::ReceiveTimeout, Opcode::Spawn] {
            let mut state = vec![ValType::Int, ValType::Float, ValType::String];
            let inst = Instruction::new(opcode, vec![]);
            transfer_types(&inst, &mut state, &HashMap::new(), &[]);
            assert!(state.iter().all(|ty| *ty == ValType::Unknown));
        }
    }

    #[test]
    fn small_constant_loop_is_unrolled_sequentially() {
        // r2 = 0; for r0 in 0..3 { r2 += r0; r0 += 1 }; return r2
        let mut f = Function::new("sum3".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(0), OperandType::Constant(Value::Int(3)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(4)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(2), OperandType::Register(0), OperandType::Register(2)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
        ], 3, 0, crate::lang::types::FunctionType::Regular);

        assert!(unroll_small_constant_loops(&mut f));
        assert_eq!(f.instructions.len(), 9);
        assert_eq!(f.instructions.iter().filter(|i| i.opcode == Opcode::Add).count(), 6);
        assert!(f.instructions.iter().all(|i| i.opcode != Opcode::Jump && i.opcode != Opcode::JumpFalse));
        assert!(matches!(f.instructions.last().unwrap().opcode, Opcode::Ret));
    }

    #[test]
    fn unrolled_early_return_branches_stay_within_each_iteration_copy() {
        // for r0 in 0..3 { if r0 == 1 { return r0 }; r0 += 1 }
        let mut f = Function::new("early_return".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(0), OperandType::Constant(Value::Int(3)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(7)]),
            Instruction::new(Opcode::IEqRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(2)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(2), OperandType::Offset(2)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
            Instruction::new(Opcode::Nop, vec![]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-7)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
        ], 3, 0, crate::lang::types::FunctionType::Regular);

        assert!(unroll_small_constant_loops(&mut f));
        let branch_pcs = [2usize, 7, 12];
        for pc in branch_pcs {
            assert_eq!(f.instructions[pc].opcode, Opcode::JumpFalse);
            assert_eq!(direct_branch_target(&f.instructions[pc], pc, f.instructions.len()), Some(pc + 2));
        }
        assert_eq!(f.instructions.len(), 17);
    }

    #[test]
    fn unroll_relocates_branches_before_the_loop() {
        // Branch at pc 0 targets pc 7 (the return), skipping the loop. Once the
        // loop shrinks, the branch displacement must be adjusted too.
        let mut f = Function::new("branch_over_loop".to_string(), vec![
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(3), OperandType::Offset(7)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(0), OperandType::Constant(Value::Int(2)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(4)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(2), OperandType::Constant(Value::Int(1)), OperandType::Register(2)]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(2)]),
        ], 4, 0, crate::lang::types::FunctionType::Regular);

        assert!(unroll_small_constant_loops(&mut f));
        assert_eq!(f.instructions[0].operands[1], OperandType::Offset(6));
        assert_eq!(f.instructions.len(), 7);
    }

    #[test]
    fn unroll_preserves_loop_condition_register_live_out_by_bailing_out() {
        let mut f = Function::new("condition_live_out".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(0), OperandType::Constant(Value::Int(2)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(4)]),
            Instruction::new(Opcode::Nop, vec![]),
            Instruction::new(Opcode::Add, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-4)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(1)]),
        ], 3, 0, crate::lang::types::FunctionType::Regular);

        assert!(!unroll_small_constant_loops(&mut f));
        assert!(f.instructions.iter().any(|inst| inst.opcode == Opcode::Jump));
    }

    #[test]
    fn simd_prefix_falls_back_to_scalar_loop_for_tail_iterations() {
        // The vector prefix must leave the original scalar loop in place. At
        // runtime a limit of 5, 6, or 7 executes the last 1, 2, or 3 iterations
        // through that original loop rather than performing an out-of-range gather.
        let mut f = Function::new("simd_tail".to_string(), vec![
            Instruction::new(Opcode::NewIntArray, vec![OperandType::Constant(Value::Int(8)), OperandType::Register(1)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::Lt, vec![OperandType::Register(0), OperandType::Constant(Value::Int(8)), OperandType::Register(3)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(3), OperandType::Offset(5)]),
            Instruction::new(Opcode::IArrayGet, vec![OperandType::Register(1), OperandType::Register(0), OperandType::Register(4)]),
            Instruction::new(Opcode::IArraySet, vec![OperandType::Register(1), OperandType::Register(0), OperandType::Register(4)]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-5)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        ], 5, 0, crate::lang::types::FunctionType::Regular);

        let analysis = analyze_types(&f, &HashMap::new(), &unknown_argument_types(&f));
        let (prefix, new_register_count) = build_int_simd_prefix(&f, 2, 7, &analysis)
            .expect("canonical local integer-array loop should be eligible");
        let prefix_len = prefix.len();
        let exit_idx = prefix.iter().position(|inst| inst.opcode == Opcode::IJumpFalseR)
            .expect("vector-width guard should have a scalar fallback");
        let exit_off = match prefix[exit_idx].operands.get(1) {
            Some(OperandType::Offset(off)) => *off,
            other => panic!("missing vector exit offset: {other:?}"),
        };
        assert_eq!(target_from_offset(exit_idx, exit_off, prefix_len), Some(prefix_len));

        assert!(insert_prefix_with_relocation(&mut f, 2, prefix, new_register_count));
        assert_eq!(f.instructions[2 + prefix_len].opcode, Opcode::Lt);
        assert_eq!(direct_branch_target(&f.instructions[3 + prefix_len], 3 + prefix_len, f.instructions.len()), Some(8 + prefix_len));
    }

    #[test]
    fn predicated_region_rejects_an_escape_back_into_the_scalar_loop() {
        // A post-backedge target is not automatically a valid lane-local exit.
        // If it jumps back to the scalar loop, vectorization must conservatively
        // decline instead of accidentally executing a second scalar iteration.
        let f = Function::new("unsafe_escape".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::ILtRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(4)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(6)]),
            Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(0), OperandType::Register(0), OperandType::Register(2)]),
            Instruction::new(Opcode::IGtRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(3), OperandType::Offset(4)]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-6)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),
        ], 4, 1, crate::lang::types::FunctionType::Regular);

        assert!(predicated_loop_region(&f, 1, 7).is_none());
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_rejects_explicit_branch_to_outer_latch() {
        // A jump straight to the latch executes the scalar increment and then
        // re-enters the loop. It is not equivalent to marking the SIMD lane done.
        let f = Function::new("explicit_latch_branch".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::ILtRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(4)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(8)]),
            Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(0), OperandType::Register(0), OperandType::Register(2)]),
            Instruction::new(Opcode::IGtRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(3), OperandType::Offset(6)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(2)]), // Explicit branch to latch at pc 8.
            Instruction::new(Opcode::Nop, vec![]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-8)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        ], 4, 1, crate::lang::types::FunctionType::Regular);

        let args = [ValType::IntArray];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([0usize]);
        assert!(build_predicated_simd_prefix(&f, 1, 9, &analysis, &args, &safe_array_args).is_none());
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_accepts_return_epilogue_liveins_written_only_after_backedge() {
        // The post-backedge epilogue writes r4, but the scalar loop-exhaustion
        // return also reads r4. Those paths are mutually exclusive: writes in
        // the return-only epilogue must not be mistaken for loop-carried writes.
        let mut f = Function::new("return_epilogue_livein".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(-1)), OperandType::Register(4)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Argument(1), OperandType::Register(1)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::ILtRRR, vec![OperandType::Register(0), OperandType::Register(1), OperandType::Register(2)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(2), OperandType::Offset(6)]),
            Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(0), OperandType::Register(0), OperandType::Register(3)]),
            Instruction::new(Opcode::IGtRCR, vec![OperandType::Register(3), OperandType::Constant(Value::Int(100)), OperandType::Register(5)]),
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(5), OperandType::Offset(5)]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-6)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(4)]),
            Instruction::new(Opcode::Nop, vec![]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Register(4), OperandType::Register(6)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(1)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Register(6), OperandType::Register(4)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
        ], 7, 2, crate::lang::types::FunctionType::Regular);

        let args = [ValType::IntArray, ValType::Unknown];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([0usize]);
        let region = predicated_loop_region(&f, 3, 9)
            .expect("return-only epilogue after the loop should be accepted");
        assert!(region.contains(&12));
        assert!(region.contains(&13));
        assert!(region.contains(&14));
        assert!(region.contains(&15));
        assert!(!region.contains(&10), "normal loop exhaustion must remain scalar");

        let (prefix, new_register_count) = build_predicated_simd_prefix(
            &f, 3, 9, &analysis, &args, &safe_array_args,
        ).expect("tail-only register writes must not be treated as loop-carried writes");
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VSelectI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VGatherI));
        let prefix_len = prefix.len();
        assert!(insert_prefix_with_relocation(&mut f, 3, prefix, new_register_count));
        assert_eq!(f.instructions[3 + prefix_len].opcode, Opcode::ILtRRR);
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn predicated_vectorizer_accepts_a_nested_integer_array_search_loop() {
        // General nested-loop shape (not a Munchausen intrinsic): candidates
        // run from 0..8; each candidate repeatedly divides by ten, gathers from
        // a read-only IntArray, accumulates a sum, and can break its inner loop
        // early when the sum exceeds the candidate. Matching candidates return
        // from the scalar loop in ascending order.
        let mut f = Function::new("nested_search".to_string(), vec![
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(0)]),
            Instruction::new(Opcode::ILtRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(8)), OperandType::Register(1)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(1), OperandType::Offset(21)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Register(0), OperandType::Register(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(0)), OperandType::Register(3)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(1)]),
            Instruction::new(Opcode::IGtRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(0)), OperandType::Register(4)]),
            Instruction::new(Opcode::JumpFalse, vec![OperandType::Register(4), OperandType::Offset(11)]),
            Instruction::new(Opcode::IRemRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(10)), OperandType::Register(5)]),
            Instruction::new(Opcode::IArrayGet, vec![OperandType::Argument(2), OperandType::Register(5), OperandType::Register(6)]),
            Instruction::new(Opcode::IAddRRR, vec![OperandType::Register(3), OperandType::Register(6), OperandType::Register(7)]),
            Instruction::new(Opcode::IGtRRR, vec![OperandType::Register(7), OperandType::Register(0), OperandType::Register(8)]),
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(8), OperandType::Offset(3)]),
            Instruction::new(Opcode::IDivRCR, vec![OperandType::Register(2), OperandType::Constant(Value::Int(10)), OperandType::Register(2)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Constant(Value::Int(-1)), OperandType::Register(2)]),
            Instruction::new(Opcode::LoadConst, vec![OperandType::Register(7), OperandType::Register(3)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-11)]),
            Instruction::new(Opcode::IEqRRR, vec![OperandType::Register(3), OperandType::Register(0), OperandType::Register(9)]),
            Instruction::new(Opcode::JumpTrue, vec![OperandType::Register(9), OperandType::Offset(3)]),
            Instruction::new(Opcode::IAddRCR, vec![OperandType::Register(0), OperandType::Constant(Value::Int(1)), OperandType::Register(0)]),
            Instruction::new(Opcode::Jump, vec![OperandType::Offset(-20)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Register(0)]),
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(-1))]),
        ], 10, 3, crate::lang::types::FunctionType::Regular);

        let args = [ValType::Int, ValType::Unknown, ValType::IntArray];
        let analysis = analyze_types(&f, &HashMap::new(), &args);
        let safe_array_args = HashSet::from([2usize]);
        let (prefix, new_register_count) = build_predicated_simd_prefix(
            &f, 1, 21, &analysis, &args, &safe_array_args,
        ).expect("nested pure integer-array loop should be eligible in release mode");
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VSelectI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VGatherI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VRemI));
        assert!(prefix.iter().any(|inst| inst.opcode == Opcode::VDivI));
        let prefix_len = prefix.len();
        assert!(insert_prefix_with_relocation(&mut f, 1, prefix, new_register_count));
        // The unmodified scalar loop remains as the fallback/tail after the
        // generated SIMD prefix, with all original branches relocated.
        assert_eq!(f.instructions[1 + prefix_len].opcode, Opcode::ILtRCR);
        for (pc, inst) in f.instructions.iter().enumerate() {
            for (operand_idx, op) in inst.operands.iter().enumerate() {
                if let OperandType::Offset(off) = op {
                    if offset_is_non_branch_metadata(inst.opcode, operand_idx) { continue; }
                    assert_eq!(jump_offset_index(inst), Some(operand_idx), "unexpected Offset at pc {pc}");
                    assert!(target_from_offset(pc, *off, f.instructions.len()).is_some(), "invalid target at pc {pc}");
                }
            }
        }
    }

    #[test]
    fn call_offsets_do_not_change_during_insert_relocation() {
        let call = Instruction::new(Opcode::Call, vec![
            OperandType::Function("callee".to_string()), OperandType::Offset(2),
            OperandType::Register(0), OperandType::Register(1), OperandType::Register(2),
        ]);
        let mut f = Function::new("f".to_string(), vec![
            Instruction::new(Opcode::Nop, vec![]), call,
            Instruction::new(Opcode::Ret, vec![OperandType::Constant(Value::Int(0))]),
        ], 3, 0, crate::lang::types::FunctionType::Regular);
        let prefix = vec![Instruction::new(Opcode::Nop, vec![])];
        assert!(insert_prefix_with_relocation(&mut f, 1, prefix, 3));
        match f.instructions[2].operands.get(1) {
            Some(OperandType::Offset(2)) => {}
            other => panic!("call arity changed: {other:?}"),
        }
    }
