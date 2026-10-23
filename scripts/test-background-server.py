#!/usr/bin/env python3
"""Exercise the actual app-server, disconnect/reconnect, and request-free waiting.

Only a local fake inference endpoint is used. No account credentials or paid model.
"""
import argparse
import base64
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import threading
import time


class Client:
    """Small test-only WebSocket client for the Unix control socket."""

    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(40)
        self.sock.connect(str(path))
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall((f"GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
        header = b""
        while not header.endswith(b"\r\n\r\n"):
            header += self.exact(1)
        accept = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
        assert b"101 Switching Protocols" in header and accept in header, header
        self.seq = 0
        self.events = []
        self.rpc("initialize", {"clientInfo": {"name": "background_acceptance", "version": "1"}, "capabilities": {"experimentalApi": True}})
        self.send({"method": "initialized"})

    def exact(self, size):
        data = b""
        while len(data) < size:
            chunk = self.sock.recv(size - len(data))
            if not chunk:
                raise EOFError("app-server disconnected")
            data += chunk
        return data

    def frame(self, payload, opcode=1):
        mask = os.urandom(4)
        length = len(payload)
        header = bytes([0x80 | opcode])
        if length < 126:
            header += bytes([0x80 | length])
        elif length <= 65535:
            header += bytes([0x80 | 126]) + struct.pack("!H", length)
        else:
            header += bytes([0x80 | 127]) + struct.pack("!Q", length)
        self.sock.sendall(header + mask + bytes(b ^ mask[i % 4] for i, b in enumerate(payload)))

    def send(self, value):
        self.frame(json.dumps(value).encode())

    def receive(self):
        fragments = b""
        while True:
            first, second = self.exact(2)
            length = second & 127
            if length == 126:
                length = struct.unpack("!H", self.exact(2))[0]
            elif length == 127:
                length = struct.unpack("!Q", self.exact(8))[0]
            assert not second & 128 and length < 32 * 1024 * 1024
            payload = self.exact(length)
            opcode = first & 15
            if opcode == 9:
                self.frame(payload, 10)
                continue
            if opcode == 8:
                raise EOFError(f"WebSocket closed: {payload!r}")
            if opcode == 10:
                continue
            fragments += payload
            if first & 128:
                return json.loads(fragments)

    def rpc(self, method, params):
        self.seq += 1
        self.send({"id": self.seq, "method": method, "params": params})
        while True:
            message = self.receive()
            if message.get("id") == self.seq:
                assert "error" not in message, message
                return message["result"]
            self.events.append(message)

    def until(self, method):
        while True:
            for index, event in enumerate(self.events):
                if event.get("method") == method:
                    return self.events.pop(index)
            self.events.append(self.receive())

    def close(self):
        self.sock.close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--seconds", type=int, default=300)
    args = parser.parse_args()
    requests = []
    changed = threading.Condition()

    class Inference(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            with changed:
                requests.append(body)
                count = len(requests)
                changed.notify_all()
            if count == 1:
                item = {"type": "function_call", "call_id": "background-start", "name": "exec_command", "arguments": json.dumps({"cmd": "while [ ! -f release-background ]; do sleep 0.1; done; echo SERVER_BACKGROUND_DONE", "login": False, "background": {"label": "Disconnected five-minute test"}})}
            else:
                item = {"type": "message", "role": "assistant", "id": f"message-{count}", "content": [{"type": "output_text", "text": "Waiting." if count == 2 else "Completion received."}]}
            events = [
                {"type": "response.created", "response": {"id": f"response-{count}"}},
                {"type": "response.output_item.done", "item": item},
                {"type": "response.completed", "response": {"id": f"response-{count}", "usage": {"input_tokens": 0, "output_tokens": 0, "total_tokens": 0}}},
            ]
            payload = "".join(f"data: {json.dumps(event)}\n\n" for event in events).encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Inference)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    with tempfile.TemporaryDirectory(prefix="codex-background-acceptance-") as temporary:
        root = Path(temporary)
        home = root / "home"
        home.mkdir()
        (home / "config.toml").write_text(f'''model = "gpt-5.4"
model_provider = "acceptance"
approval_policy = "never"
sandbox_mode = "workspace-write"
[features]
unified_exec = true
[model_providers.acceptance]
name = "Local acceptance fixture"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
''')
        sock = root / "server.sock"
        environment = dict(os.environ, CODEX_HOME=str(home))
        for key in ("OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"):
            environment.pop(key, None)
        log_path = Path(__file__).resolve().parents[1] / "acceptance-server.log"
        with log_path.open("w") as log:
            process = subprocess.Popen([str(args.binary.resolve()), "app-server", "--listen", f"unix://{sock}"], env=environment, cwd=root, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 30
                while not sock.exists():
                    assert process.poll() is None, log_path.read_text()[-6000:]
                    assert time.monotonic() < deadline, "server socket startup timeout"
                    time.sleep(0.05)
                client = Client(sock)
                thread_id = client.rpc("thread/start", {"cwd": str(root), "model": "gpt-5.4", "approvalPolicy": "never", "sandbox": "workspace-write"})["thread"]["id"]
                client.rpc("turn/start", {"threadId": thread_id, "input": [{"type": "text", "text": "Start a background task and finish your turn while it waits."}]})
                client.until("turn/completed")
                assert len(requests) == 2, f"Expected two initial requests, got {len(requests)}"
                tasks = client.rpc("thread/backgroundTerminals/list", {"threadId": thread_id})["tasks"]
                assert tasks[0]["status"] == "running", tasks
                client.close()
                print(f"Client disconnected; task running. Waiting {args.seconds}s with no inference.", flush=True)
                with changed:
                    unexpected = changed.wait_for(lambda: len(requests) != 2, timeout=args.seconds)
                assert not unexpected, "Unexpected inference while waiting"
                client = Client(sock)
                client.rpc("thread/resume", {"threadId": thread_id})
                tasks = client.rpc("thread/backgroundTerminals/list", {"threadId": thread_id})["tasks"]
                assert tasks[0]["status"] == "running", tasks
                client.close()
                (root / "release-background").write_text("go")
                with changed:
                    assert changed.wait_for(lambda: len(requests) >= 3, timeout=30), "No automatic completion wake"
                assert b"SERVER_BACKGROUND_DONE" in requests[2], requests[2][-4000:]
                with changed:
                    assert not changed.wait_for(lambda: len(requests) > 3, timeout=1), "Duplicate completion wake"
                client = Client(sock)
                client.rpc("thread/resume", {"threadId": thread_id})
                tasks = client.rpc("thread/backgroundTerminals/list", {"threadId": thread_id})["tasks"]
                assert tasks[0]["status"] == "succeeded" and tasks[0]["exitCode"] == 0, tasks
                client.close()
                print(json.dumps({"result": "passed", "idle_seconds": args.seconds, "requests_before_wait": 2, "requests_during_wait": 0, "completion_requests": 1, "disconnected_completion": True, "paid_tokens": 0}), flush=True)
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                server.shutdown()


if __name__ == "__main__":
    main()
