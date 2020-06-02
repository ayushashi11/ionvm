use std::num::ParseIntError;
use ionvm::vm::VM;
use ionvm::env::IonEnv;
use std::env;
use rustyline::Editor;
pub struct Repl{
    vm:VM,
    cmd_buf:Vec<String>,
    rl:Editor<()>,
    loc:String
}
impl Repl{
    pub fn new() -> Self{
	let loc=env::var("HOME").unwrap_or(home.to_string())+"/.ion.history.txt";
	let mut rl=Editor::<()>::new();
	if rl.load_history(&loc).is_err(){
	    println!("No history");
	}
	Repl{
	    vm:VM::new(),
	    cmd_buf:vec![],
	    rl,
	    loc:loc.to_string()
	}
    }
    fn parse_hex(&mut self, i: &str) -> Result<Vec<u16>, ParseIntError>{
	let split:Vec<&str> = i.split(' ').collect();
	let mut res:Vec<u16> = vec![];
	for s in split{
	    match u16::from_str_radix(&s, 16){
		Ok(r) => res.push(r),
		Err(e) => return Err(e),
	    };
	}
	Ok(res)
    }
    pub fn run (&mut self){
	println!("Ionvm 0.2.0");
	unsafe{IonEnv.is_shell = true;}
	loop{
	    let buf=self.rl.readline(prompt).unwrap_or(String::from(".quit"));
	    match buf.as_str(){
		".quit" => {
		    println!("See ya!");
		    self.rl.save_history(&self.loc).unwrap();
		    std::process::exit(0);
		},
		".history" => {
		    for cmd in &self.cmd_buf{
			println!("{}",cmd);
		    }
		},
		".program"|".prog" => {
		    for inst in &self.vm.program{
			println!("{}",inst);
		    }
		},
		".registers"|".regs" => {
		    println!("normal registers:{:?}",self.vm.regs);
		    println!("remainder_flag:{}",self.vm.rem);
		    println!("program counter:{}",self.vm.pc);
		},
		_ => {
		    let res=self.parse_hex(buf.as_str());
		    match res{
			Ok(bytes) => {
			    for b in bytes{
				self.vm.program.push(b);
			    }
			},
			Err(e) => println!("Invalid input\n{}",e)
		    };
		    self.vm.run_once();
		}
	    }
	    self.rl.add_history_entry(buf.clone());
	    self.cmd_buf.push(buf);
	}
    }
}
fn main (){
    let mut repl=Repl::new();
    repl.run();
}
#[cfg(windows)]
const home:&'static str="%APPDATA%";
#[cfg(windows)]
const prompt:&'static str=">>> ";
#[cfg(not(windows))]
const home:&'static str="~";
#[cfg(not(windows))]
const prompt:&'static str="\x1b[38;5;85m>>> \x1b[0m";
