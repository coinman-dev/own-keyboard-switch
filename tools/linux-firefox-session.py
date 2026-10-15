#!/usr/bin/env python3
"""Start Firefox's official local automation session for an isolated test profile."""
import json
import socket
import sys

with socket.create_connection(("127.0.0.1", int(sys.argv[1])), 10) as connection:
    connection.settimeout(10)
    def receive():
        prefix = b""
        while not prefix.endswith(b":"):
            block=connection.recv(1)
            if not block: raise RuntimeError("Firefox closed its automation socket")
            prefix+=block
        remaining=int(prefix[:-1]); data=b""
        while remaining:
            block=connection.recv(remaining)
            if not block: raise RuntimeError("Firefox returned a partial automation reply")
            data+=block;remaining-=len(block)
        return json.loads(data)
    hello=receive()
    if hello.get("applicationType")!="gecko": raise RuntimeError("Unexpected automation peer")
    request=json.dumps([0,1,"WebDriver:NewSession",{"capabilities":{"alwaysMatch":{}}}]).encode()
    connection.sendall(str(len(request)).encode()+b":"+request)
    response=receive()
    if response[2]: raise RuntimeError(response[2])
    print("Isolated Firefox WebDriver session started")
