"""Deterministic CI-monitor tests: no GitHub calls or real sleeps."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("monitor", Path(__file__).with_name("background-ci-monitor.py"))
monitor = importlib.util.module_from_spec(spec)
spec.loader.exec_module(monitor)


class MonitorTests(unittest.TestCase):
    def run_monitor(self, responses):
        stdout = io.StringIO()
        with patch("sys.argv", ["monitor", "123", "--repo", "owner/repo"]), \
             patch.object(monitor.subprocess, "run", side_effect=responses), \
             patch.object(monitor.time, "sleep"), contextlib.redirect_stdout(stdout):
            result = monitor.main()
        return result, [json.loads(line) for line in stdout.getvalue().splitlines()]

    @staticmethod
    def response(status, conclusion=None):
        return subprocess.CompletedProcess([], 0, json.dumps({"status": status, "conclusion": conclusion, "url": "https://example.test/run/123"}))

    def test_success_is_quiet_until_completion(self):
        code, events = self.run_monitor([self.response("queued"), self.response("in_progress"), self.response("completed", "success")])
        self.assertEqual(code, 0)
        self.assertEqual([e["event"] for e in events], ["completed"])

    def test_failure_and_cancelled_are_terminal(self):
        for conclusion in ("failure", "cancelled", "timed_out"):
            with self.subTest(conclusion=conclusion):
                code, events = self.run_monitor([self.response("completed", conclusion)])
                self.assertEqual(code, 1)
                self.assertEqual(events[0]["conclusion"], conclusion)

    def test_transient_network_error_recovers_without_alert(self):
        code, events = self.run_monitor([OSError("offline"), self.response("completed", "success")])
        self.assertEqual(code, 0)
        self.assertEqual(len(events), 1)

    def test_persistent_errors_produce_one_terminal_event(self):
        code, events = self.run_monitor([OSError("offline")] * 5)
        self.assertEqual(code, 2)
        self.assertEqual([e["event"] for e in events], ["monitor_failed"])


if __name__ == "__main__":
    unittest.main()
