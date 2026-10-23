#!/usr/bin/env python3
"""Watch one GitHub Actions run using ordinary code, never model polling."""
import argparse
import json
import subprocess
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run_id", type=int)
    parser.add_argument("--repo", required=True, help="owner/repository")
    parser.add_argument("--interval", type=float, default=60)
    parser.add_argument("--timeout", type=float, default=24 * 60 * 60)
    parser.add_argument("--progress", action="store_true")
    args = parser.parse_args()
    if args.interval < 1 or args.timeout <= 0:
        parser.error("interval must be >= 1 second and timeout must be positive")
    started = time.monotonic()
    previous = None
    errors = 0
    while time.monotonic() - started < args.timeout:
        try:
            response = subprocess.run(
                ["gh", "run", "view", str(args.run_id), "--repo", args.repo,
                 "--json", "status,conclusion,url"],
                capture_output=True, text=True, timeout=45, check=True,
            )
            run = json.loads(response.stdout)
            if not isinstance(run, dict) or not isinstance(run.get("status"), str):
                raise ValueError("Invalid GitHub run response")
            errors = 0
        except (subprocess.SubprocessError, OSError, ValueError) as error:
            errors += 1
            if errors >= 5:
                print(json.dumps({"event": "monitor_failed", "reason": str(error)}), flush=True)
                return 2
            time.sleep(min(args.interval * errors, 300))
            continue
        if run["status"] == "completed":
            print(json.dumps({"event": "completed", **run}), flush=True)
            return 0 if run.get("conclusion") == "success" else 1
        if args.progress and run["status"] != previous:
            print(json.dumps({"event": "status_changed", **run}), flush=True)
        previous = run["status"]
        time.sleep(args.interval)
    print(json.dumps({"event": "monitor_timeout"}), flush=True)
    return 2


if __name__ == "__main__":
    sys.exit(main())
