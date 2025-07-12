use std::rc::Rc;
use std::thread;
use std::time::Duration;
use crate::value::{Function, Primitive, Value};
use crate::vm::{IonVM, Instruction};

#[test]
fn test_receive_with_timeout_sets_result_false() {
    let mut vm = IonVM::new();
    vm.set_debug(true);
    // Function: r0 = receive_with_timeout(r1, r2); return r2
    // r1 = timeout (ms), r2 = result
    let bytecode = vec![
        Instruction::LoadConst(1, Value::Primitive(Primitive::Number(50.0))), // r1 = 50ms
        Instruction::ReceiveWithTimeout(0, 1, 2), // r0 = msg, r1 = timeout, r2 = result
        Instruction::Return(2), // return r2 (should be false)
    ];
    let func = Rc::new(Function::new_bytecode(Some("timeout_test".to_string()), 0, 3, bytecode));
    let pid = vm.spawn_process(func, vec![]);
    // Run the VM (should block, then timeout)
    vm.run();
    // Wait a bit to ensure timeout expires
    thread::sleep(Duration::from_millis(60));
    // Manually trigger timeout handling
    vm.handle_expired_timeouts();
    // Run again to unblock process
    vm.run();
    // Check result
    let proc = vm.processes.get(&pid).unwrap().borrow();
    println!("Process {:?}", proc);
    let result = proc.last_result.as_ref().unwrap();
    assert_eq!(result, &Value::Primitive(Primitive::Boolean(false)));
}
