#!/usr/bin/env python3
"""Require successful managed CodeQL analysis for every configured language.

Only read APIs are used. Missing, neutral, skipped and stale results never pass.
The managed workflow ID is repository-specific and intentionally explicit.
"""

import argparse
from datetime import datetime
import json
import subprocess
import time


LANGUAGES = ("actions", "c-cpp", "javascript-typescript", "python", "rust")


def api(path):
    result = subprocess.run(
        ["gh", "api", "--paginate", "--slurp", path],
        check=True, capture_output=True, text=True, timeout=60,
    )
    return json.loads(result.stdout)


def coverage_complete(run, jobs, checks, sha):
    if not run or run.get("head_sha") != sha:
        return False
    if run.get("status") != "completed" or run.get("conclusion") != "success":
        return False
    for language in LANGUAGES:
        matches = [job for job in jobs if job["name"] == f"Analyze ({language})"]
        if not matches or any(job.get("conclusion") != "success" for job in matches):
            return False
    codeql = [check for check in checks if check["name"] == "CodeQL"
              and check.get("app", {}).get("id") == 57789
              and check.get("head_sha") == sha]
    latest = max(codeql, key=lambda check: check["id"], default=None)
    if not latest or latest.get("conclusion") != "success":
        return False
    # A result from an earlier attempt at the same SHA is not qualification
    # for the latest attempt. CodeQL may update an existing check in place.
    try:
        completed = datetime.fromisoformat(latest["completed_at"].replace("Z", "+00:00"))
        started = datetime.fromisoformat(run["run_started_at"].replace("Z", "+00:00"))
        return completed >= started
    except (KeyError, TypeError, ValueError):
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--workflow-id", required=True, type=int)
    parser.add_argument("--timeout", default=1200, type=int)
    args = parser.parse_args()
    deadline = time.monotonic() + args.timeout
    while True:
        pages = api(f"repos/{args.repo}/actions/workflows/{args.workflow_id}/runs"
                    f"?head_sha={args.sha}&per_page=100")
        runs = [run for page in pages for run in page["workflow_runs"]]
        run = max(runs, key=lambda item: item["id"], default=None)
        jobs = []
        if run:
            # The attempt-specific endpoint prevents a successful old attempt
            # from hiding a failed or incomplete rerun.
            pages = api(f"repos/{args.repo}/actions/runs/{run['id']}/attempts/"
                        f"{run['run_attempt']}/jobs?per_page=100")
            jobs = [job for page in pages for job in page["jobs"]]
        pages = api(f"repos/{args.repo}/commits/{args.sha}/check-runs?per_page=100")
        checks = [check for page in pages for check in page["check_runs"]]
        if coverage_complete(run, jobs, checks, args.sha):
            print(f"CodeQL: all five language analyses and result passed for {args.sha}")
            return
        if time.monotonic() >= deadline:
            raise SystemExit("CodeQL coverage incomplete or unsuccessful; refusing qualification")
        print("Waiting for successful CodeQL coverage on the exact PR head", flush=True)
        time.sleep(min(20, max(0, deadline - time.monotonic())))


if __name__ == "__main__":
    main()
