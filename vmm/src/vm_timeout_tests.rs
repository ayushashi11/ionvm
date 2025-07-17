use crate::value::{Function, Primitive, Value};
use crate::vm::{Instruction, IonVM};
use std::rc::Rc;
use std::thread;
use std::time::Duration;

#[test]
fn test_receive_with_timeout_sets_result_false() {
    let mut vm = IonVM::new();
    vm.set_debug(true);
    // Function: r0 = receive_with_timeout(r1, r2); return r2
    // r1 = timeout (ms), r2 = result
    let bytecode = vec![
        Instruction::LoadConst(1, Value::Primitive(Primitive::Number(50.0))), // r1 = 50ms
        Instruction::ReceiveWithTimeout(0, 1, 2), // r0 = msg, r1 = timeout, r2 = result
        Instruction::Return(2),                   // return r2 (should be false)
    ];
    let func = Rc::new(Function::new_bytecode(
        Some("timeout_test".to_string()),
        0,
        3,
        bytecode,
    ));
    let pid = vm.spawn_process(func, vec![]);
    // Run the VM (should block, then timeout)
    vm.run();
    // Wait a bit to ensure timeout expires
    thread::sleep(Duration::from_millis(60));
    // Now the process should have timed out
    println!("Running VM after timeout...");
    // Run again to unblock process
    vm.run();
    // Check result
    let proc = vm.processes.get(&pid).unwrap().borrow();
    let result = proc.last_result.as_ref().unwrap_or(&Value::Primitive(Primitive::Undefined));
    assert!(result==&Value::Primitive(Primitive::Boolean(false)));
}

#[test]
fn test_message_recieved_before_timeout() {
    let mut vm = IonVM::new();
    vm.set_debug(true);

    // Function: r0 = receive_with_timeout(r1, r2); return r2
    // r1 = timeout (ms), r2 = result
    let bytecode = vec![
        Instruction::LoadConst(1, Value::Primitive(Primitive::Number(100.0))), // r1 = 100ms
        Instruction::ReceiveWithTimeout(0, 1, 2), // r0 = msg, r1 = timeout, r2 = result
        Instruction::Return(2),                   // return r2 (should be true)
    ];
    let func = Rc::new(Function::new_bytecode(
        Some("timeout_test".to_string()),
        0,
        3,
        bytecode,
    ));
    let pid = vm.spawn_process(func, vec![]);

    // Simulate sending a message before the timeout
    // Run the VM to process the message (should block on receive_with_timeout)
    vm.run();
    // Instead of pushing directly to the mailbox, use the SEND instruction to properly wake the process
    // But for this test, we mimic a scheduler tick: after pushing, manually wake the process
    {
        let mut proc = vm.processes.get(&pid).unwrap().borrow_mut();
        proc.mailbox.push(Value::Primitive(Primitive::Number(42.0)));
        if proc.status == crate::value::ProcessStatus::WaitingForMessage {
            proc.status = crate::value::ProcessStatus::Runnable;
            // Also push the pid to the run queue if not already present
            if !vm.run_queue.contains(&pid) {
                vm.run_queue.push_back(pid);
            }
        }
    }
    // Now run the VM again, the process should consume the message and set result to true
    vm.run();
    // Wait till timeout expires, it should have no effect
    thread::sleep(Duration::from_millis(110));
    vm.run();
    // Check result
    let proc = vm.processes.get(&pid).unwrap().borrow();
    let result = proc.last_result.as_ref().unwrap_or(&Value::Primitive(Primitive::Undefined));
    println!("Result: {:?}", result);
    assert!(result == &Value::Primitive(Primitive::Boolean(true)));
}
