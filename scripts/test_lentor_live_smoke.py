"""Offline safety checks; these tests never use real provider credentials."""

import io
import json
import os
from pathlib import Path
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

import lentor_live_smoke as smoke


class LiveSmokeTests(unittest.TestCase):
    def test_configure_error_captures_only_valid_reference(self):
        for data, expected_ref, expected_outcome in (
            (
                {
                    "providerCredentialReference": "b" * 64,
                    "providerConfigurationOutcome": "unknown",
                },
                "b" * 64,
                None,
            ),
            ({"providerConfigurationOutcome": "unchanged"}, None, "unchanged"),
            (
                {
                    "providerCredentialReference": "invalid",
                    "providerConfigurationOutcome": "unchanged",
                },
                None,
                None,
            ),
        ):
            server = smoke.Server.__new__(smoke.Server)
            server.next_id = 0
            server.configure_reference = server.configure_outcome = None
            server.send = lambda _: None
            server.receive = lambda _: {"id": 1, "error": {"data": data}}
            with self.assertRaises(smoke.Failure):
                server.request("modelProvider/configure", {})
            self.assertEqual(
                (server.configure_reference, server.configure_outcome),
                (expected_ref, expected_outcome),
            )

    def test_failed_configure_cleans_reference_without_config(self):
        secret = "fake-smoke-credential"
        reference = "b" * 64

        class Process:
            def poll(self):
                return 0

        class FakeServer:
            leaked = False
            p = Process()
            configure_reference = None
            configure_outcome = None

            def __init__(self, binary, home, workspace, key, deadline):
                self.home = home

            def request(self, method, params):
                if method == "modelProvider/configure":
                    self.configure_reference = (
                        reference  # synthetic key stored before config failure
                    )
                    raise smoke.Failure("rpc_modelProvider_configure_failed")
                return {}

            def send(self, message):
                pass

            def shutdown(self):
                return True

        with (
            patch.object(smoke, "Server", FakeServer),
            patch.object(smoke, "delete_credential", return_value=True) as delete,
            patch.object(
                smoke, "scan_files", side_effect=smoke.Failure("file_scan_timeout")
            ),
        ):
            result = smoke.run(
                Path("unused.exe"),
                secret,
                "responses",
                smoke.DEFAULT_MODELS["responses"],
                smoke.BASE,
            )
        delete.assert_called_once_with(reference)
        self.assertEqual(result["credential_cleanup"], "deleted")
        self.assertEqual(result["error"], "rpc_modelProvider_configure_failed")
        self.assertEqual(result["file_scan_error"], "file_scan_timeout")
        self.assertNotIn(secret, json.dumps(result))

    def test_turn_error_redacts_key_before_truncating(self):
        secret = "fake-smoke-credential"
        server = smoke.Server.__new__(smoke.Server)
        server.secret = secret.encode()
        server.turn_error = None
        server.request = lambda method, params: {"turn": {"id": "turn"}}
        server.receive = lambda end: {
            "method": "turn/completed",
            "params": {
                "threadId": "thread",
                "turn": {
                    "id": "turn",
                    "status": "failed",
                    "error": {"message": "x" * 500 + secret + " detail"},
                },
            },
        }
        with self.assertRaises(smoke.Failure):
            server.turn("thread", "prompt", require_plan=False)
        self.assertEqual(server.turn_error, "x" * 500 + "[redacted] d")
        self.assertNotIn(secret, json.dumps(server.turn_error))

    def test_transport_failure_without_reference_retains_evidence(self):
        class Process:
            def poll(self):
                return 0

        class FakeServer:
            leaked = False
            p = Process()

            def __init__(self, binary, home, workspace, key, deadline):
                pass

            def request(self, method, params):
                if method == "modelProvider/configure":
                    raise smoke.Failure("server_eof")
                return {}

            def send(self, message):
                pass

            def shutdown(self):
                return True

        with (
            patch.object(smoke, "Server", FakeServer),
            patch.object(smoke, "remove_created_root") as remove,
        ):
            result = smoke.run(
                Path("unused.exe"),
                "fake-smoke-credential",
                "responses",
                smoke.DEFAULT_MODELS["responses"],
                smoke.BASE,
            )
        self.assertEqual(result["credential_cleanup"], "reference_unavailable")
        self.assertTrue(result["temporary_directory_retained"])
        remove.assert_not_called()

    def test_selection_is_exact_and_messages_url_normalizes(self):
        for protocol, model in smoke.DEFAULT_MODELS.items():
            self.assertEqual(
                smoke.normalize_selection(protocol, model, smoke.BASE), smoke.BASE
            )
            with self.assertRaises(smoke.Failure):
                smoke.normalize_selection(protocol, model + "-fallback", smoke.BASE)
            with self.assertRaises(smoke.Failure):
                smoke.normalize_selection(protocol, model, "https://other.example/v1")
        self.assertEqual(
            smoke.normalize_selection(
                "messages", smoke.DEFAULT_MODELS["messages"], smoke.BASE + "/messages"
            ),
            smoke.BASE,
        )

    def test_scan_finds_secret_across_chunks_and_utf16(self):
        secret = "fake-smoke-credential"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            path = root / "log"
            path.write_bytes(b"x" * (64 * 1024 - 5) + secret.encode())
            self.assertTrue(smoke.scan_files(root, secret))
            path.write_bytes(secret.encode("utf-16-le"))
            self.assertTrue(smoke.scan_files(root, secret))
            path.write_text("sanitized", encoding="utf-8")
            self.assertFalse(smoke.scan_files(root, secret))

    def test_scan_covers_nested_files_and_accounts_for_all_bytes(self):
        secret = "fake-smoke-credential"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            session = root / "home" / "sessions" / "turn.jsonl"
            cache = root / "home" / "plugins" / "nested" / "cache"
            session.parent.mkdir(parents=True)
            cache.parent.mkdir(parents=True)
            session.write_text(secret, encoding="utf-8")
            cache.write_bytes(b"x" * (64 * 1024 + 1))
            self.assertTrue(smoke.scan_files(root, secret))
            session.write_bytes(b"safe")
            stats = {}
            self.assertFalse(smoke.scan_files(root, secret, stats=stats))
            self.assertEqual(stats, {"files": 2, "bytes": 4 + 64 * 1024 + 1})

    def test_scan_drains_pending_and_finds_secret_in_nested_final_work(self):
        secret = "fake-smoke-credential"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for index in range(40):
                (root / f"safe-{index}").write_bytes(b"safe")
            nested = root / "nested" / "deep"
            nested.mkdir(parents=True)
            (nested / "last").write_text(secret, encoding="utf-8")
            stats = {}
            self.assertTrue(smoke.scan_files(root, secret, stats=stats))
            self.assertEqual(stats, {"files": 41, "bytes": 160 + len(secret)})

    def test_scan_limits_fail_closed(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "one").write_bytes(b"1234")
            (root / "two").write_bytes(b"5678")
            for limit, value in (("MAX_SCAN_FILES", 1), ("MAX_SCAN_BYTES", 7)):
                with self.subTest(limit=limit), patch.object(smoke, limit, value):
                    with self.assertRaisesRegex(smoke.Failure, "file_scan_limit"):
                        smoke.scan_files(root, "fake-smoke-credential")

    def test_scan_reads_concurrently_and_propagates_read_errors(self):
        barrier = threading.Barrier(smoke.SCAN_WORKERS, timeout=3)
        real_open = open

        class ConcurrentFile:
            def __init__(self, path, mode):
                self.source = real_open(path, mode)

            def __enter__(self):
                return self

            def __exit__(self, *args):
                self.source.close()

            def read(self, size):
                barrier.wait()
                raise OSError("simulated read failure")

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for index in range(smoke.SCAN_WORKERS):
                (root / str(index)).write_bytes(b"safe")
            with patch.object(smoke, "open", ConcurrentFile, create=True):
                with self.assertRaisesRegex(OSError, "simulated read failure"):
                    smoke.scan_files(root, "fake-smoke-credential")

    def test_scan_timeout_waits_for_active_read_and_closes_file(self):
        active = threading.Event()
        closed = threading.Event()
        real_open = open

        class SlowFile:
            def __init__(self, path, mode):
                self.source = real_open(path, mode)

            def __enter__(self):
                return self

            def __exit__(self, *args):
                self.source.close()
                closed.set()

            def read(self, size):
                active.set()
                time.sleep(0.1)
                return self.source.read(size)

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "slow").write_bytes(b"safe")
            with (
                patch.object(smoke, "open", SlowFile, create=True),
                patch.object(smoke, "SCAN_TIMEOUT", 0.03),
            ):
                with self.assertRaisesRegex(smoke.Failure, "file_scan_timeout"):
                    smoke.scan_files(root, "fake-smoke-credential")
            self.assertTrue(active.is_set())
            self.assertTrue(closed.is_set())

    def test_shutdown_failure_cannot_be_overwritten_by_cleanup_retry(self):
        class Process:
            stdin = io.BytesIO()

            def __init__(self):
                self.waits = self.terminations = self.kills = 0

            def wait(self, timeout):
                self.waits += 1
                if self.waits == 1:
                    raise smoke.subprocess.TimeoutExpired("fake-server", timeout)
                return 0

            def terminate(self):
                self.terminations += 1

            def kill(self):
                self.kills += 1

        server = smoke.Server.__new__(smoke.Server)
        server.p = Process()
        server.threads = []
        server.shutdown_result = None
        self.assertFalse(server.shutdown())
        self.assertFalse(server.shutdown())
        self.assertEqual(
            (server.p.waits, server.p.terminations, server.p.kills), (2, 1, 0)
        )

    def test_spawn_removes_key_and_separates_homes(self):
        class Process:
            stdout = io.BytesIO()
            stderr = io.BytesIO()

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            credential_names = (
                "VercelAPI",
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
                "LENTOR_API_KEY",
                "LENTOR_ACCESS_TOKEN",
                "CODEX_API_KEY",
                "codex_access_token",
                "LENTOR_SQLITE_HOME",
                "CODEX_SQLITE_HOME",
            )
            with (
                patch.dict(
                    os.environ,
                    {name: "fake-smoke-credential" for name in credential_names},
                ),
                patch.object(smoke.subprocess, "CREATE_NO_WINDOW", 0, create=True),
                patch.object(
                    smoke.subprocess, "Popen", return_value=Process()
                ) as spawn,
            ):
                server = smoke.Server(
                    root / "lentor.exe",
                    root / "lentor-home",
                    root,
                    "fake-smoke-credential",
                    time.monotonic() + 10,
                )
            for thread in server.threads:
                thread.join(timeout=2)
            env = spawn.call_args.kwargs["env"]
            self.assertTrue(
                all(
                    name.upper() not in {key.upper() for key in env}
                    for name in credential_names
                )
            )
            self.assertEqual(env["LENTOR_HOME"], str(root / "lentor-home"))
            self.assertNotEqual(env["LENTOR_HOME"], env["CODEX_HOME"])
            self.assertEqual(
                spawn.call_args.args[0][1:], ["app-server", "--listen", "stdio://"]
            )

    def test_resume_requires_a_second_persisted_tool_output(self):
        thread = "00000000-0000-0000-0000-000000000001"
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "sessions").mkdir()
            path = root / "sessions" / (thread + ".jsonl")
            entries = []
            for index in range(2):
                call = str(index)
                entries += [
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "function_call",
                            "name": "update_plan",
                            "call_id": call,
                        },
                    },
                    {
                        "type": "response_item",
                        "payload": {
                            "type": "function_call_output",
                            "call_id": call,
                            "output": "Plan updated",
                        },
                    },
                ]
            path.write_text("\n".join(map(json.dumps, entries[:2])), encoding="utf-8")
            with self.assertRaises(smoke.Failure):
                smoke.verify_plan_output(root, thread, minimum=2)
            path.write_text("\n".join(map(json.dumps, entries)), encoding="utf-8")
            self.assertTrue(smoke.verify_plan_output(root, thread, minimum=2))
            # Call IDs are scoped to a response; a later response may reuse one.
            entries[2]["payload"]["call_id"] = entries[0]["payload"]["call_id"]
            entries[3]["payload"]["call_id"] = entries[0]["payload"]["call_id"]
            path.write_text("\n".join(map(json.dumps, entries)), encoding="utf-8")
            self.assertTrue(smoke.verify_plan_output(root, thread, minimum=2))
            path.write_text(
                "\n".join(map(json.dumps, entries[:2] + entries[1:2])), encoding="utf-8"
            )
            with self.assertRaises(smoke.Failure):
                smoke.verify_plan_output(root, thread, minimum=2)

    def test_driver_restarts_and_cleans_credential_even_on_failure(self):
        secret = "fake-smoke-credential"
        thread_id = "00000000-0000-0000-0000-000000000001"
        for protocol in smoke.PROTOCOLS:
            for fail in (False, True):
                instances = []

                class Process:
                    def poll(self):
                        return 0

                class FakeServer:
                    leaked = False
                    p = Process()

                    def __init__(self, binary, home, workspace, key, deadline):
                        self.home = home
                        self.turns = []
                        self.configure = None
                        instances.append(self)

                    def request(self, method, params):
                        if method == "initialize":
                            return {}
                        if method == "modelProvider/configure":
                            self.configure = params
                            self.home.joinpath("config.toml").write_text(
                                '[model_providers.smoke]\nwire_api = "'
                                + protocol
                                + '"\nkeyring_key = "'
                                + "a" * 64
                                + '"\n',
                                encoding="utf-8",
                            )
                            if fail:
                                raise RuntimeError(secret)
                            return {
                                "providerId": "smoke",
                                "baseUrl": smoke.BASE,
                                "model": smoke.DEFAULT_MODELS[protocol],
                            }
                        if method == "model/list":
                            return {
                                "data": [
                                    {
                                        "model": smoke.DEFAULT_MODELS[protocol],
                                        "supportedReasoningEfforts": [],
                                    }
                                ]
                            }
                        return {
                            "model": smoke.DEFAULT_MODELS[protocol],
                            "modelProvider": "smoke",
                            "reasoningEffort": None,
                            "thread": {"id": thread_id},
                        }

                    def send(self, message):
                        pass

                    def turn(self, *args, **kwargs):
                        self.turns.append((args, kwargs))
                        return {"streamed": True, "plan_updated": True}

                    def shutdown(self):
                        return True

                with (
                    patch.object(smoke, "Server", FakeServer),
                    patch.object(smoke, "verify_plan_output", return_value=True),
                    patch.object(
                        smoke, "delete_credential", return_value=True
                    ) as delete,
                ):
                    result = smoke.run(
                        Path("unused.exe"),
                        secret,
                        protocol,
                        smoke.DEFAULT_MODELS[protocol],
                        smoke.BASE,
                    )
                self.assertEqual(result["ok"], not fail)
                self.assertNotIn(secret, json.dumps(result))
                self.assertEqual(result["credential_cleanup"], "deleted")
                delete.assert_called_once_with("a" * 64)
                self.assertFalse(instances[0].home.exists())
                self.assertEqual(instances[0].configure["wireApi"], protocol)
                if not fail:
                    self.assertEqual(len(instances), 2)
                    self.assertEqual(len(instances[1].turns), 1)
                    self.assertTrue(instances[1].turns[0][1]["require_verify"])


if __name__ == "__main__":
    unittest.main()
