use std::io::Read;
use std::env::args;
use std::fs::File;
use ionvm::repl::Repl;
use ionvm::vm::VM;
fn main(){
    match args().nth(1){
	Some(fil) => {
	    let mut fil = File::open(fil).expect("Unable to open file");
	    let mut dat=String::new();
	    fil.read_to_string(&mut dat).expect("Unable to read file");
	    let prog:Vec<u16>=dat.encode_utf16().collect();
	    let mut vm=VM::from(prog);
	    vm.run();
	    println!("{:?}\nremainder_flag:{}\nprogram counter:{}",vm.regs,vm.rem,vm.pc);
	},
	None => {
	    let mut repl=Repl::new();
	    repl.run();
	}
    }
}
