"""Bounded, explicitly invoked live provider smoke adapted from vercel_endpoint_smoke.py."""

import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import ctypes
import json
import os
import queue
import re
import shutil
import stat
import subprocess
import tempfile
import threading
import time
import tomllib
from pathlib import Path

BASE = "https://ai-gateway.vercel.sh/v1"
PROTOCOLS = ("responses", "chat", "messages")
DEFAULT_MODELS = {protocol: "inclusionai/ling-3.1-flash-1019" for protocol in PROTOCOLS}
MAX_LINE = 2 * 1024 * 1024
MAX_BYTES = 24 * 1024 * 1024
MAX_SCAN_BYTES = 192 * 1024 * 1024
MAX_SCAN_FILES = 20000
SCAN_TIMEOUT = 90
SCAN_WORKERS = 4
SCAN_PENDING = 16
MAX_EVENTS = 10000
RPC_TIMEOUT = 45
TURN_TIMEOUT = 180
TOTAL_TIMEOUT = 480


def secret_patterns(secret):
    return (
        secret.encode("utf-8"),
        secret.encode("utf-16-le"),
        json.dumps(secret, ensure_ascii=True)[1:-1].encode("ascii"),
    )


class Failure(Exception):
    pass


class Server:
    def __init__(self, binary, home, workspace, secret, deadline):
        self.secret = secret.encode("utf-8")
        self.patterns = secret_patterns(secret)
        self.deadline = deadline
        self.events = queue.Queue(maxsize=MAX_EVENTS)
        self.bytes = 0
        self.leaked = False
        self.overflow = False
        self.next_id = 0
        self.configure_reference = None
        self.configure_outcome = None
        self.turn_error = None
        self.shutdown_result = None
        excluded = {
            "VERCELAPI",
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "LENTOR_API_KEY",
            "LENTOR_ACCESS_TOKEN",
            "CODEX_API_KEY",
            "CODEX_ACCESS_TOKEN",
            "LENTOR_SQLITE_HOME",
            "CODEX_SQLITE_HOME",
        }
        env = {
            key: value
            for key, value in os.environ.items()
            if key.upper() not in excluded
        }
        env["LENTOR_HOME"] = str(home)
        env["CODEX_HOME"] = str(home.parent / "unused-codex-home")
        # Preserve non-credential variables, including sandbox restrictions.
        self.p = subprocess.Popen(
            [str(binary), "app-server", "--listen", "stdio://"],
            cwd=workspace,
            env=env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            creationflags=subprocess.CREATE_NO_WINDOW,
            bufsize=0,
        )
        self.threads = [
            threading.Thread(target=self.read_stdout, daemon=True),
            threading.Thread(target=self.read_stderr, daemon=True),
        ]
        for thread in self.threads:
            thread.start()

    def scan(self, data):
        if any(pattern in data for pattern in self.patterns):
            self.leaked = True

    def read_stdout(self):
        try:
            while True:
                line = self.p.stdout.readline(MAX_LINE + 1)
                if not line:
                    break
                self.scan(line)
                self.bytes += len(line)
                if len(line) > MAX_LINE or self.bytes > MAX_BYTES:
                    self.overflow = True
                    break
                try:
                    obj = json.loads(line)
                    self.scan(json.dumps(obj, ensure_ascii=False).encode("utf-8"))
                    self.events.put_nowait(obj)
                except (ValueError, queue.Full):
                    self.overflow = True
                    break
        finally:
            try:
                self.events.put_nowait(None)
            except queue.Full:
                pass

    def read_stderr(self):
        tail = b""
        count = 0
        while True:
            chunk = self.p.stderr.read(8192)
            if not chunk:
                return
            count += len(chunk)
            if count > MAX_BYTES:
                self.overflow = True
            self.scan(tail + chunk)
            tail = (tail + chunk)[-(max(map(len, self.patterns)) - 1) :]

    def send(self, obj):
        self.p.stdin.write(
            json.dumps(obj, ensure_ascii=True, separators=(",", ":")).encode() + b"\n"
        )
        self.p.stdin.flush()

    def receive(self, end):
        if self.overflow:
            raise Failure("output_limit")
        timeout = min(end, self.deadline) - time.monotonic()
        if timeout <= 0:
            raise Failure("timeout")
        try:
            obj = self.events.get(timeout=timeout)
        except queue.Empty:
            raise Failure("timeout") from None
        if obj is None:
            raise Failure("server_eof")
        if not isinstance(obj, dict):
            raise Failure("invalid_message")
        if "method" in obj and "id" in obj:
            # Refuse unsolicited approvals or other server requests, never execute them.
            self.send(
                {"id": obj["id"], "error": {"code": -32600, "message": "unsupported"}}
            )
        return obj

    def request(self, method, params):
        self.next_id += 1
        request_id = self.next_id
        self.send({"id": request_id, "method": method, "params": params})
        end = time.monotonic() + RPC_TIMEOUT
        while True:
            obj = self.receive(end)
            if obj.get("id") == request_id and "method" not in obj:
                if "error" in obj:
                    if method == "modelProvider/configure":
                        error = obj.get("error")
                        data = error.get("data") if isinstance(error, dict) else None
                        if isinstance(data, dict):
                            ref = data.get("providerCredentialReference")
                            if isinstance(ref, str) and re.fullmatch(
                                r"[0-9a-f]{64}", ref
                            ):
                                self.configure_reference = ref
                            if (
                                data.get("providerConfigurationOutcome") == "unchanged"
                                and ref is None
                            ):
                                self.configure_outcome = "unchanged"
                    raise Failure("rpc_" + method.replace("/", "_") + "_failed")
                if not isinstance(obj.get("result"), dict):
                    raise Failure("invalid_response")
                return obj["result"]

    def turn(self, thread_id, prompt, require_plan, effort=None, require_verify=False):
        # TurnStartParams uses effort, not reasoningEffort. Omit on continuation
        # so the resumed thread must retain the first turn's selected effort.
        params = {
            "threadId": thread_id,
            "input": [{"type": "text", "text": prompt, "text_elements": []}],
        }
        if effort is not None:
            params["effort"] = effort
        result = self.request("turn/start", params)
        turn_id = result["turn"]["id"]
        plan = False
        tool_output = False
        streamed = False
        after_plan = False
        text = ""
        end = time.monotonic() + TURN_TIMEOUT
        while True:
            obj = self.receive(end)
            method = obj.get("method")
            params = obj.get("params") or {}
            if params.get("threadId") != thread_id:
                continue
            if method == "turn/plan/updated" and params.get("turnId") == turn_id:
                plan = bool(params.get("plan"))
            if method == "item/completed" and params.get("turnId") == turn_id:
                item = params.get("item") or {}
                if (
                    item.get("type") == "functionCallOutput"
                    and item.get("name") == "update_plan"
                ):
                    tool_output = True
            if (
                method == "item/agentMessage/delta"
                and params.get("turnId") == turn_id
                and params.get("delta")
            ):
                streamed = True
                text = (text + params["delta"])[-8192:]
                if plan:
                    after_plan = True
            if (
                method == "turn/completed"
                and (params.get("turn") or {}).get("id") == turn_id
            ):
                if params["turn"].get("status") != "completed":
                    error = params["turn"].get("error") or {}
                    message = error.get("message") if isinstance(error, dict) else None
                    if isinstance(message, str):
                        # Redact before truncating so a boundary cannot expose part of the key.
                        self.turn_error = message.replace(
                            self.secret.decode("utf-8"), "[redacted]"
                        )[:512]
                    raise Failure("turn_failed")
                if not streamed or (require_plan and not (plan and after_plan)):
                    raise Failure("turn_evidence_missing")
                if require_verify and "verify" not in text.lower():
                    raise Failure("resume_context_not_confirmed")
                return {
                    "streamed": streamed,
                    "plan_updated": plan,
                    "tool_output_item_seen": tool_output,
                    "streamed_after_plan": after_plan,
                }

    def shutdown(self):
        if self.shutdown_result is not None:
            return self.shutdown_result
        try:
            self.p.stdin.close()
        except OSError:
            pass
        try:
            code = self.p.wait(timeout=20)
        except subprocess.TimeoutExpired:
            self.p.terminate()
            try:
                self.p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.p.kill()
                self.p.wait(timeout=5)
            code = -1
        for thread in self.threads:
            thread.join(timeout=2)
        self.shutdown_result = code == 0 and all(not t.is_alive() for t in self.threads)
        return self.shutdown_result


def scan_files(root, secret, *, stats=None):
    patterns = secret_patterns(secret)
    deadline = time.monotonic() + SCAN_TIMEOUT
    stop = threading.Event()
    total = 0
    count = 0
    directories = [root]
    pending = set()

    def scan_one(path):
        tail = b""
        with open(path, "rb") as source:
            while not stop.is_set():
                if time.monotonic() > deadline:
                    raise Failure("file_scan_timeout")
                chunk = source.read(64 * 1024)
                if time.monotonic() > deadline:
                    raise Failure("file_scan_timeout")
                if not chunk:
                    return False
                data = tail + chunk
                if any(pattern in data for pattern in patterns):
                    return True
                tail = data[-(max(map(len, patterns)) - 1) :]
        return False

    def collect():
        nonlocal pending
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise Failure("file_scan_timeout")
        done, pending = wait(pending, timeout=remaining, return_when=FIRST_COMPLETED)
        if not done:
            raise Failure("file_scan_timeout")
        results = [future.result() for future in done]
        return any(results)

    with ThreadPoolExecutor(max_workers=SCAN_WORKERS) as executor:
        try:
            while directories:
                with os.scandir(directories.pop()) as entries:
                    for entry in entries:
                        if time.monotonic() > deadline:
                            raise Failure("file_scan_timeout")
                        metadata = entry.stat(follow_symlinks=False)
                        reparse = getattr(metadata, "st_file_attributes", 0) & getattr(
                            stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0x400
                        )
                        if reparse or stat.S_ISLNK(metadata.st_mode):
                            continue
                        if stat.S_ISDIR(metadata.st_mode):
                            directories.append(entry.path)
                            continue
                        if not stat.S_ISREG(metadata.st_mode):
                            continue
                        total += metadata.st_size
                        count += 1
                        if stats is not None:
                            stats.update(files=count, bytes=total)
                        if total > MAX_SCAN_BYTES or count > MAX_SCAN_FILES:
                            raise Failure("file_scan_limit")
                        pending.add(executor.submit(scan_one, entry.path))
                        if len(pending) >= SCAN_PENDING and collect():
                            return True
            while pending:
                if collect():
                    return True
            if time.monotonic() > deadline:
                raise Failure("file_scan_timeout")
        finally:
            stop.set()
            for future in pending:
                future.cancel()
    return False


def credential_reference(home):
    config = home / "config.toml"
    if not config.exists():
        return None
    if config.stat().st_size > MAX_LINE:
        raise Failure("config_limit")
    value = tomllib.loads(config.read_text(encoding="utf-8"))
    ref = value.get("model_providers", {}).get("smoke", {}).get("keyring_key")
    if ref is None:
        return None
    if not isinstance(ref, str) or not re.fullmatch(r"[0-9a-f]{64}", ref):
        raise Failure("invalid_credential_reference")
    return ref


def delete_credential(reference):
    # keyring 3.6.3 Windows generic credential target is {user}.{service}.
    advapi = ctypes.WinDLL("Advapi32", use_last_error=True)
    delete = advapi.CredDeleteW
    delete.argtypes = [ctypes.c_wchar_p, ctypes.c_uint32, ctypes.c_uint32]
    delete.restype = ctypes.c_int
    if delete(reference + ".lentor-api-providers", 1, 0):
        return True
    return ctypes.get_last_error() == 1168  # ERROR_NOT_FOUND


def verify_plan_output(home, thread, minimum=1):
    if not re.fullmatch(r"[0-9a-fA-F-]{36}", thread):
        raise Failure("invalid_thread_id")
    paths = list(home.glob("sessions/**/*" + thread + "*.jsonl"))
    if len(paths) != 1 or paths[0].stat().st_size > MAX_BYTES:
        raise Failure("rollout_missing_or_too_large")
    calls = set()
    completed = 0
    for line in paths[0].read_bytes().splitlines():
        item = json.loads(line)
        if item.get("type") != "response_item":
            continue
        payload = item.get("payload") or {}
        if (
            payload.get("type") == "function_call"
            and payload.get("name") == "update_plan"
        ):
            calls.add(payload.get("call_id"))
        if (
            payload.get("type") == "function_call_output"
            and payload.get("call_id") in calls
            and payload.get("output") == "Plan updated"
        ):
            calls.remove(payload["call_id"])
            completed += 1
    if completed < minimum:
        raise Failure("plan_output_missing")
    return True


def remove_created_root(root, worktrees):
    # Re-resolve immediately before recursive removal; reject redirected roots.
    resolved_parent = worktrees.resolve(strict=True)
    resolved_root = root.resolve(strict=True)
    if (
        not resolved_parent.is_dir()
        or resolved_root == resolved_parent
        or not resolved_root.is_relative_to(resolved_parent)
        or resolved_root.parent != resolved_parent
        or resolved_root != root
        or root.is_symlink()
        or root.is_junction()
        or not resolved_root.name.startswith("lentor-live-smoke-")
    ):
        raise Failure("unsafe_cleanup_root")

    def remove_readonly(function, path, error):
        failed = Path(path)
        if (
            not isinstance(error, PermissionError)
            or failed.is_symlink()
            or failed.is_junction()
            or not failed.resolve(strict=True).is_relative_to(resolved_root)
        ):
            raise error
        # Git object packs in this newly created test home are read-only on Windows.
        os.chmod(failed, stat.S_IWRITE)
        function(path)

    shutil.rmtree(resolved_root, onexc=remove_readonly)


def run(binary, secret, protocol, model, base):
    summary = {
        "ok": False,
        "stage": "setup",
        "secret_in_server_output": False,
        "secret_in_temp_files": None,
        "credential_cleanup": "not_attempted",
        "graceful_shutdown": True,
        "temporary_directory_removed": False,
    }
    deadline = time.monotonic() + TOTAL_TIMEOUT
    worktrees = Path(tempfile.gettempdir()).resolve(strict=True)
    root = Path(tempfile.mkdtemp(prefix="lentor-live-smoke-", dir=worktrees)).resolve(
        strict=True
    )
    home, workspace = root / "home", root / "workspace"
    home.mkdir()
    workspace.mkdir()
    (home / "config.toml").write_text(
        "[features]\nshell_tool = false\n[tools.update_plan]\nenabled = true\n",
        encoding="utf-8",
    )
    servers = []
    alive = False
    configure_attempted = False
    try:

        def start():
            server = Server(binary, home, workspace, secret, deadline)
            servers.append(server)
            server.request(
                "initialize",
                {
                    "clientInfo": {
                        "name": "endpoint_smoke",
                        "title": "Endpoint Smoke",
                        "version": "1",
                    },
                    "capabilities": {"experimentalApi": True},
                },
            )
            server.send({"method": "initialized", "params": {}})
            return server

        summary["stage"] = "initialize"
        server = start()
        summary["stage"] = "configure"
        configure_attempted = True
        result = server.request(
            "modelProvider/configure",
            {
                "providerId": "smoke",
                "name": "Smoke",
                "baseUrl": base,
                "apiKey": secret,
                "model": model,
                "wireApi": protocol,
            },
        )
        if (
            result.get("providerId") != "smoke"
            or result.get("baseUrl") != base
            or result.get("model") != model
        ):
            raise Failure("configure_response_mismatch")
        if credential_reference(home) is None:
            raise Failure("credential_reference_missing")
        summary["stage"] = "protocol_persistence"
        config = tomllib.loads((home / "config.toml").read_text(encoding="utf-8"))
        if (
            config.get("model_providers", {}).get("smoke", {}).get("wire_api")
            != protocol
        ):
            raise Failure("configured_protocol_mismatch")
        summary["stage"] = "model_list"
        cursor = None
        found = None
        for _ in range(20):
            page = server.request(
                "model/list", {"limit": 200, "cursor": cursor, "includeHidden": True}
            )
            found = next(
                (
                    row
                    for row in page.get("data", [])
                    if row.get("id") == model or row.get("model") == model
                ),
                found,
            )
            cursor = page.get("nextCursor")
            if not cursor:
                break
        else:
            raise Failure("model_page_limit")
        if found is None:
            raise Failure("model_not_found")
        efforts = found.get("supportedReasoningEfforts")
        if not isinstance(efforts, list):
            raise Failure("reasoning_metadata_missing")
        summary["model_found"] = True
        summary["reasoning_efforts_advertised"] = len(efforts)
        known_efforts = (
            "none",
            "minimal",
            "low",
            "medium",
            "high",
            "xhigh",
            "max",
            "ultra",
            "persistent",
        )
        summary["reasoning_efforts"] = [
            item.get("reasoningEffort")
            for item in efforts
            if isinstance(item, dict) and item.get("reasoningEffort") in known_efforts
        ][:16]
        selected_effort = next(
            (
                effort
                for effort in known_efforts
                if effort in summary["reasoning_efforts"]
            ),
            None,
        )
        # Sparse catalogs may advertise no reasoning control; do not invent one.
        summary["selected_reasoning_effort"] = selected_effort
        summary["stage"] = "thread_start"
        started = server.request(
            "thread/start",
            {
                "model": model,
                "modelProvider": "smoke",
                "cwd": str(workspace),
                "approvalPolicy": "never",
                "sandbox": "read-only",
                "ephemeral": False,
            },
        )
        if started.get("model") != model or started.get("modelProvider") != "smoke":
            raise Failure("thread_start_metadata_mismatch")
        expected_effort = (
            selected_effort
            if selected_effort is not None
            else started.get("reasoningEffort")
        )
        thread = started["thread"]["id"]
        summary["stage"] = "first_turn"
        summary["first_turn"] = server.turn(
            thread,
            "Call update_plan with one completed step named Verify, then respond briefly that the plan is complete. Do not use any other tool.",
            True,
            effort=selected_effort,
        )
        summary["stage"] = "restart"
        if not server.shutdown():
            raise Failure("shutdown_failed")
        summary["plan_result_in_rollout"] = verify_plan_output(home, thread)
        summary["stage"] = "resume"
        server = start()
        resumed = server.request("thread/resume", {"threadId": thread})
        if resumed["thread"]["id"] != thread:
            raise Failure("resume_mismatch")
        # ThreadResumeResponse exposes all three effective persisted settings.
        if (
            resumed.get("model") != model
            or resumed.get("modelProvider") != "smoke"
            or resumed.get("reasoningEffort") != expected_effort
        ):
            raise Failure("resume_metadata_mismatch")
        summary["resumed_model_provider_effort_verified"] = True
        summary["stage"] = "continued_turn"
        summary["continued_turn"] = server.turn(
            thread,
            "Name the completed step from the previous turn, then call update_plan with that same step completed. Do not use any other tool. Respond briefly.",
            True,
            require_verify=True,
        )
        if not server.shutdown():
            raise Failure("resumed_shutdown_failed")
        summary["plan_result_after_resume"] = verify_plan_output(
            home, thread, minimum=2
        )
        summary["stage"] = "isolation"
        unused = root / "unused-codex-home"
        if unused.exists() and any(unused.iterdir()):
            raise Failure("codex_home_used")
        summary["stage"] = "complete"
        summary["ok"] = True
    except Failure as error:
        summary["error"] = str(error)
    except Exception:
        summary["error"] = "internal_error"
    finally:
        for server in servers:
            try:
                if server.p.poll() is None and not server.shutdown():
                    summary["graceful_shutdown"] = False
            except Exception:
                summary["graceful_shutdown"] = False
            if server.p.poll() is None:
                alive = True
            summary["secret_in_server_output"] |= server.leaked
            if getattr(server, "turn_error", None):
                summary["turn_error_message"] = server.turn_error
        scan_stats = {}
        scan_started = time.monotonic()
        try:
            summary["secret_in_temp_files"] = scan_files(root, secret, stats=scan_stats)
        except Failure as error:
            summary["secret_in_temp_files"] = None
            summary["ok"] = False
            summary.setdefault("error", "file_scan_failed")
            summary["file_scan_error"] = str(error)
        except Exception as error:
            summary["secret_in_temp_files"] = None
            summary["ok"] = False
            summary.setdefault("error", "file_scan_failed")
            summary["file_scan_error"] = type(error).__name__
        scan_stats["seconds"] = round(time.monotonic() - scan_started, 2)
        summary["file_scan"] = scan_stats
        try:
            ref = next(
                (
                    getattr(server, "configure_reference", None)
                    for server in servers
                    if getattr(server, "configure_reference", None)
                ),
                None,
            )
            ref = ref or credential_reference(home)
            if ref and not alive:
                summary["credential_cleanup"] = (
                    "deleted" if delete_credential(ref) else "failed"
                )
            elif ref:
                summary["credential_cleanup"] = "process_still_running"
            elif not configure_attempted or any(
                getattr(server, "configure_outcome", None) == "unchanged"
                for server in servers
            ):
                summary["credential_cleanup"] = "no_credential_created"
            else:
                summary["credential_cleanup"] = "reference_unavailable"
        except Exception:
            summary["credential_cleanup"] = "failed"
        if alive or summary["credential_cleanup"] in (
            "reference_unavailable",
            "failed",
            "process_still_running",
        ):
            summary["temporary_directory_retained"] = True
        else:
            try:
                remove_created_root(root, worktrees)
                summary["temporary_directory_removed"] = True
            except (OSError, Failure):
                summary["temporary_directory_retained"] = True
                summary["ok"] = False
                summary["error"] = "temporary_directory_cleanup_failed"
        if (
            summary["secret_in_server_output"]
            or summary["secret_in_temp_files"] is not False
            or summary["credential_cleanup"] not in ("deleted", "no_credential_created")
            or not summary["graceful_shutdown"]
        ):
            summary["ok"] = False
    return summary


def normalize_selection(protocol, model, base):
    base = base.rstrip("/")
    if protocol == "messages" and base == BASE + "/messages":
        base = BASE
    if base != BASE or model != DEFAULT_MODELS[protocol]:
        raise Failure("unsupported_gateway_or_model")
    return base


def main():
    parser = argparse.ArgumentParser(
        description="Explicit Windows Vercel provider integration smoke; never builds."
    )
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--protocol", required=True, choices=PROTOCOLS)
    parser.add_argument(
        "--model", required=True, help="Exact provider model ID; no fallback"
    )
    parser.add_argument(
        "--base", required=True, help="Gateway API base, or /v1/messages for Anthropic"
    )
    args = parser.parse_args()
    # This live harness is authorized only for the same Vercel gateway.
    try:
        base = normalize_selection(args.protocol, args.model, args.base)
    except Failure:
        parser.exit(2, "Unsupported explicit gateway/model selection\n")
    secret = os.environ.pop("VercelAPI", "")
    if os.name != "nt" or not secret or not args.binary.is_file():
        result = {"ok": False, "error": "windows_binary_or_environment_missing"}
    else:
        try:
            result = run(args.binary.resolve(), secret, args.protocol, args.model, base)
        except Exception:
            result = {"ok": False, "error": "setup_failed"}
    result.update(protocol=args.protocol, model=args.model, base=base)
    # Only bounded summaries and a redacted turn error reach stdout.
    output = json.dumps(result, separators=(",", ":"))
    if (secret and secret in output) or len(output) > 8192:
        output = '{"ok":false,"error":"unsafe_summary"}'
        result["ok"] = False
    print(output)
    return 0 if result["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
