#!/usr/bin/env python3
"""HTTPS server for Godot web exports with required COOP/COEP headers."""
from http.server import HTTPServer, SimpleHTTPRequestHandler
import ssl, subprocess, os, sys, threading

class GodotHandler(SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        self.send_header("Cache-Control", "no-cache")
        super().end_headers()

    def log_message(self, format, *args):
        sys.stderr.write("%s - - [%s] %s\n" %
                         (self.address_string(), self.log_date_time_string(),
                          format % args))

def ensure_cert():
    base = os.path.dirname(os.path.abspath(__file__))
    cert = os.path.join(base, "cert.pem")
    key = os.path.join(base, "key.pem")
    if not os.path.exists(cert) or not os.path.exists(key):
        print("Generating self-signed certificate...")
        subprocess.run([
            "openssl", "req", "-x509", "-newkey", "rsa:2048",
            "-keyout", key, "-out", cert,
            "-days", "365", "-nodes",
            "-subj", "/CN=localhost"
        ], check=True)
    return cert, key

os.chdir(os.path.dirname(os.path.abspath(__file__)))
cert, key = ensure_cert()
port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080

server = HTTPServer(("0.0.0.0", port), GodotHandler)
ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
ctx.load_cert_chain(cert, key)
server.socket = ctx.wrap_socket(server.socket, server_side=True)

print(f"Serving on https://0.0.0.0:{port}")
server.serve_forever()
