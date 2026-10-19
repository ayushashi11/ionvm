pub mod cuda;
pub mod llvm_translator;
pub mod profiler;
pub mod translator;

use crate::lang::types::Function;

pub fn maybe_jit_compile(func: &mut Function) {
    if profiler::record_call_and_check_hot(func) {
        if let Some(native_fn) = translator::compile_function(func) {
            func.jit_compiled_fn = Some(native_fn);
        }
    }
}
