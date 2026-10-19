use std::io;
use crate::lang::types::Value;

pub fn tcp_listen(args: &[Value]) -> io::Result<Value> {
    if args.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "port required"));
    }
    let port = match args[0] {
        Value::Int(p) => p as u16,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "port must be int")),
    };

    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let optval: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &optval as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr.s_addr = 0; // INADDR_ANY

        let bind_res = libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        if bind_res < 0 {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        let listen_res = libc::listen(fd, 128);
        if listen_res < 0 {
            libc::close(fd);
            return Err(io::Error::last_os_error());
        }

        Ok(Value::Int(fd as i64))
    }
}

pub fn tcp_accept(args: &[Value]) -> io::Result<Value> {
    if args.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "listener fd required"));
    }
    let listener_fd = match args[0] {
        Value::Int(fd) => fd as i32,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "listener fd must be int")),
    };

    unsafe {
        let mut client_addr: libc::sockaddr_in = std::mem::zeroed();
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

        let client_fd = libc::accept(
            listener_fd,
            &mut client_addr as *mut _ as *mut libc::sockaddr,
            &mut len,
        );
        if client_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(Value::Int(client_fd as i64))
    }
}

pub fn tcp_recv(args: &[Value]) -> io::Result<Value> {
    if args.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "fd and max_bytes required"));
    }
    let client_fd = match args[0] {
        Value::Int(fd) => fd as i32,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "client fd must be int")),
    };
    let max_bytes = match args[1] {
        Value::Int(n) => (n.max(1)) as usize,
        _ => 4096,
    };

    let mut buf = vec![0u8; max_bytes];
    unsafe {
        let bytes_read = libc::read(client_fd, buf.as_mut_ptr() as *mut libc::c_void, max_bytes);
        if bytes_read < 0 {
            return Err(io::Error::last_os_error());
        }
        buf.truncate(bytes_read as usize);
    }

    let s = String::from_utf8_lossy(&buf).to_string();
    Ok(Value::String(s))
}

pub fn tcp_send(args: &[Value]) -> io::Result<Value> {
    if args.len() < 2 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "fd and data required"));
    }
    let client_fd = match args[0] {
        Value::Int(fd) => fd as i32,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "client fd must be int")),
    };
    let data = match &args[1] {
        Value::String(s) => s.as_bytes(),
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "data must be string")),
    };

    unsafe {
        let bytes_written = libc::write(client_fd, data.as_ptr() as *const libc::c_void, data.len());
        if bytes_written < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Value::Int(bytes_written as i64))
    }
}

pub fn tcp_close(args: &[Value]) -> io::Result<Value> {
    if args.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "fd required"));
    }
    let fd = match args[0] {
        Value::Int(fd) => fd as i32,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "fd must be int")),
    };

    unsafe {
        libc::close(fd);
    }
    Ok(Value::Nil)
}
