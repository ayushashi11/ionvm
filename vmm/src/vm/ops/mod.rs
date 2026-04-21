mod actor;
mod arithmetic;
mod comparison;
mod control;
mod misc;
mod object;

use super::{ExecutionResult, IonVM};
use crate::instruction::Instruction;
use crate::value::process::Process;

impl IonVM {
    pub(crate) fn execute_instruction(
        &mut self,
        proc: &mut Process,
        instruction: Instruction,
    ) -> ExecutionResult {
        use Instruction::*;

        // Helper to get the last frame, panicking with a clear message if absent
        macro_rules! frame {
            () => {
                proc.frames
                    .last_mut()
                    .expect("process must have an active frame")
            };
        }

        match instruction {
            // --- Memory ---
            LoadConst(reg, val) => {
                misc::exec_load_const(self, proc, reg, val);
                ExecutionResult::Continue
            }
            Move(dst, src) => {
                misc::exec_move(frame!(), dst, src);
                ExecutionResult::Continue
            }

            // --- Arithmetic ---
            Add(dst, a, b) => {
                arithmetic::exec_add(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            Sub(dst, a, b) => {
                arithmetic::exec_sub(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            Mul(dst, a, b) => {
                arithmetic::exec_mul(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            Div(dst, a, b) => {
                arithmetic::exec_div(frame!(), dst, a, b);
                ExecutionResult::Continue
            }

            // --- Comparison ---
            Equal(dst, a, b) => {
                comparison::exec_equal(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            NotEqual(dst, a, b) => {
                comparison::exec_not_equal(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            LessThan(dst, a, b) => {
                comparison::exec_less_than(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            LessEqual(dst, a, b) => {
                comparison::exec_less_equal(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            GreaterThan(dst, a, b) => {
                comparison::exec_greater_than(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            GreaterEqual(dst, a, b) => {
                comparison::exec_greater_equal(frame!(), dst, a, b);
                ExecutionResult::Continue
            }

            // --- Logical ---
            And(dst, a, b) => {
                comparison::exec_and(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            Or(dst, a, b) => {
                comparison::exec_or(frame!(), dst, a, b);
                ExecutionResult::Continue
            }
            Not(dst, src) => {
                comparison::exec_not(frame!(), dst, src);
                ExecutionResult::Continue
            }

            // --- Object ---
            ObjectInit(dst, kvs) => {
                object::exec_object_init(frame!(), dst, kvs);
                ExecutionResult::Continue
            }
            GetProp(dst, obj, key) => {
                object::exec_get_prop(frame!(), dst, obj, key);
                ExecutionResult::Continue
            }
            SetProp(obj, key, val) => {
                object::exec_set_prop(frame!(), obj, key, val);
                ExecutionResult::Continue
            }

            // --- Control flow ---
            Jump(offset) => {
                control::exec_jump(frame!(), offset);
                ExecutionResult::Continue
            }
            JumpIfTrue(cond, offset) => {
                control::exec_jump_if_true(frame!(), cond, offset);
                ExecutionResult::Continue
            }
            JumpIfFalse(cond, offset) => {
                control::exec_jump_if_false(frame!(), cond, offset);
                ExecutionResult::Continue
            }
            ArrayInit(dst, srcs) => {
                misc::exec_array_init(frame!(), dst, srcs);
                ExecutionResult::Continue
            }
            Return(reg) => control::exec_return(proc, reg),
            Call(dst, func, args) => control::exec_call(self, proc, dst, func, args),
            MakeClosure(dst, func, scope_id, captures) => {
                control::exec_make_closure(proc, dst, func, scope_id, captures);
                ExecutionResult::Continue
            }

            // --- Actor ---
            Spawn(dst, func, args) => actor::exec_spawn(self, proc, dst, func, args),
            Send(proc_reg, msg_reg) => actor::exec_send(self, proc, proc_reg, msg_reg),
            Receive(dst) => actor::exec_receive(proc, dst),
            ReceiveWithTimeout(dst, timeout, result) => {
                actor::exec_receive_with_timeout(self, proc, dst, timeout, result)
            }
            Link(target_pid, ret_reg) => actor::exec_link(self, proc, target_pid, ret_reg),
            Select(dst, pids) => actor::exec_select(self, proc, dst, pids),
            SelectWithKill(dst, pids) => actor::exec_select_with_kill(self, proc, dst, pids),

            // --- Misc ---
            Match(src, patterns) => misc::exec_match(proc, src, patterns),
            Nop => ExecutionResult::Continue,
            Yield => todo!("generator/coroutine yield is not yet implemented"),
        }
    }
}
