use openssl::ssl::{SslMethod, SslConnector};
use std::io::{Read, Write};
use std::net::TcpStream;
fn main(){
let connector = SslConnector::builder(SslMethod::tls()).unwrap().build();

let stream = TcpStream::connect("localhost:5000").unwrap();
let mut stream = connector.connect("localhost", stream).unwrap();

stream.write_all(b"id\n").unwrap();
let mut res = vec![];
stream.read_to_end(&mut res).unwrap();
println!("{}", String::from_utf8_lossy(&res));

}
