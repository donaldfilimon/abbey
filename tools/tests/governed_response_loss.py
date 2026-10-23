"""Bounded loopback proxy: discard replies only after verified upstream acceptance."""
import hashlib
import http.client
import http.server
import json
import socket
import threading


class ResponseLossProxy(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, backend_port):
        super().__init__(("127.0.0.1", 0), _Forward)
        self.backend_port = backend_port
        self.lock = threading.Lock()
        self.evidence = {}
        self.thread = threading.Thread(target=self.serve_forever, daemon=True)
        self.thread.start()

    def close(self):
        self.shutdown()
        self.server_close()
        self.thread.join(timeout=3)


class _Forward(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.forward()

    def do_POST(self):
        self.forward()

    def forward(self):
        length = int(self.headers.get("Content-Length", 0))
        if not 0 <= length <= 1024 * 1024:
            self.send_error(413)
            return
        body = self.rfile.read(length)
        headers = {key: value for key, value in self.headers.items()
                   if key.lower() not in ("connection", "transfer-encoding")}
        connection = http.client.HTTPConnection("127.0.0.1", self.server.backend_port, timeout=10)
        try:
            connection.request(self.command, self.path, body=body or None, headers=headers)
            response = connection.getresponse()
            data = response.read(1024 * 1024 + 1)
            if len(data) > 1024 * 1024:
                raise ValueError("upstream response exceeds fixture bound")
            kind = None
            if self.command == "POST" and response.status == 200:
                request = json.loads(body)
                result = json.loads(data)
                action = request.get("action", {})
                recovery = action.get("request", {}).get("action", {})
                if (self.path == "/v1/relay/node" and recovery.get("operation") == "publish"
                        and result.get("recovery", {}).get("status") == "recorded"):
                    kind = "publication"
                if self.path.endswith("/delivery/ack") and result is True:
                    kind = "consumer_ack"
            with self.server.lock:
                drop = kind is not None and kind not in self.server.evidence
                if drop:
                    # Only non-secret acceptance facts leave this fixture.
                    self.server.evidence[kind] = {
                        "upstream_status": response.status,
                        "accepted": True,
                        "response_sha256": hashlib.sha256(data).hexdigest(),
                        "response_dropped": True,
                    }
                    if kind == "publication":
                        encoded = json.dumps(recovery["packet"], sort_keys=True,
                                             separators=(",", ":")).encode()
                        self.server.evidence[kind]["packet_sha256"] = hashlib.sha256(encoded).hexdigest()
            if drop:
                self.close_connection = True
                self.connection.shutdown(socket.SHUT_RDWR)
                return
            self.send_response(response.status)
            for key, value in response.getheaders():
                if key.lower() not in ("connection", "transfer-encoding", "content-length"):
                    self.send_header(key, value)
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except OSError:
            self.close_connection = True
        finally:
            connection.close()

    def log_message(self, *_):
        pass
