use crate::lang::types::Function;

pub const HOT_THRESHOLD: usize = 20;

pub fn record_call_and_check_hot(func: &mut Function) -> bool {
    if func.jit_compiled_fn.is_some() {
        return false;
    }
    func.call_counter += 1;
    func.call_counter >= HOT_THRESHOLD
}
