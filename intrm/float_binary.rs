fn main(){
	let v=(f32::INFINITY).to_ne_bytes();
	println!("{:?},{}",v,f32::from_ne_bytes(v));
}
