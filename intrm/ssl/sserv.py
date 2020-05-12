import ssl
import subprocess as sb
from socket import *
context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain('localhost.crt', 'localhost.key')
s=socket()
s.bind(("localhost",5000))
s.listen()
s=context.wrap_socket(s,server_side=True)
conn,addr=s.accept()
stdin=conn.makefile()
stdout=conn.makefile(mode="w")
sb.run(["bash"],stdin=stdin,stdout=stdout,stderr=stdout)
