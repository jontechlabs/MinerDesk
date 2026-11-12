#!/usr/bin/env python3
"""Exercise real isolated headless backends without running a mining engine.

All listeners are loopback, every configuration directory is disposable, and
every profile is disabled with an executable that does not exist. --serve keeps
the fixture dashboards available for visual QA until interrupted.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def call(base, path, body=None, status=200, headers=None, context=None):
    data = None if body is None else json.dumps(body).encode()
    values = {"Content-Type": "application/json", **(headers or {})}
    request = urllib.request.Request(base + path, data=data, headers=values)
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context))
        response = opener.open(request, timeout=18)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        value = json.loads(response.read())
        assert response.code == status, (path, response.code, value)
        return value


def eventually(check, seconds=20):
    deadline = time.monotonic() + seconds
    error = None
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, AssertionError, urllib.error.URLError) as failure:
            error = failure
        time.sleep(0.15)
    raise AssertionError(f"Fixture condition did not complete: {error}")


def free_port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class Rig:
    def __init__(self, binary, base, name):
        self.binary, self.base, self.name = binary, base / name, name
        self.base.mkdir(mode=0o700)
        self.port = free_port()
        self.url = f"http://127.0.0.1:{self.port}"
        config = self.base / "MinerDesk"
        config.mkdir(mode=0o700)
        self.config_file = config / "config.json"
        self.private_file = config / "fleet-private" / "state.json"
        self.config_file.write_text(json.dumps({
            "miners": [{"id": "fixture", "name": "Disabled fixture", "enabled": False,
                        "engine": "srbminer", "executable_path": "/never-exists/fleet-fixture-miner",
                        "algorithm": "pearlhash", "pool": "pool.example.invalid:3360",
                        "wallet": "public-fixture-address", "password": "private-fixture-pool-password"}],
            "schedules": [], "web": {"desktop_api_port": self.port, "expose_lan": False,
                                      "token": "private-fixture-web-token"},
            "ui": {"language": "en"}
        }))
        self.process = None
        self.log = None

    def start(self):
        environment = dict(os.environ, XDG_CONFIG_HOME=str(self.base))
        self.log = (self.base / "backend-fixture.log").open("ab")
        self.process = subprocess.Popen([str(self.binary), "--config", str(self.config_file), "--listen", "127.0.0.1", "--port", str(self.port)],
                                        env=environment, stdout=self.log, stderr=self.log)
        eventually(lambda: call(self.url, "/api/health").get("scheduler"))

    def stop(self):
        if self.process and self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=3)
        if self.log:
            self.log.close()
        self.process = self.log = None

    def view(self):
        return call(self.url, "/api/fleet")

    def private(self):
        return json.loads(self.private_file.read_text())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--backend", required=True, type=Path)
    parser.add_argument("--serve", action="store_true")
    arguments = parser.parse_args()
    source_binary = arguments.backend.resolve()
    assert source_binary.is_file()
    rigs = []
    checks = []
    with tempfile.TemporaryDirectory(prefix="minerdesk-fleet-e2e-") as directory:
        try:
            base = Path(directory)
            # The fixture owns its executable copy as well as its settings, so
            # visual QA never locks a maintainer's build output on Windows.
            binary = base / source_binary.name
            shutil.copy2(source_binary, binary)
            for name in ["fleet-hub-fixture", "fleet-linux-fixture", "fleet-third-fixture"]:
                rigs.append(Rig(binary, base, name))
                rigs[-1].start()
            a, b, c = rigs
            initial = [rig.view() for rig in rigs]
            identities = [view["setup"]["rig_id"] for view in initial]
            assert len(set(identities)) == 3
            assert all(view["setup"]["mode"] == "standalone" for view in initial)
            checks.append("Independent persistent rig identities; Fleet disabled by default")

            call(a.url, "/api/fleet/create", {"name": "Denied", "advertise_url": "https://127.0.0.1:0", "listen": "127.0.0.1:0"},
                 status=403, headers={"Origin": "https://attacker.example"})
            assert a.view()["setup"]["mode"] == "standalone"
            checks.append("Cross-origin enrollment is rejected before creating a listener")

            created = call(a.url, "/api/fleet/create", {"name": "Fleet test coordinator", "advertise_url": "https://127.0.0.1:0", "listen": "127.0.0.1:0"})
            address = created["setup"]["hub_url"]
            assert address.startswith("https://127.0.0.1:")
            assert call(a.url, "/api/health")["fleet_enabled"]

            def invite():
                return call(a.url, "/api/fleet/invite", {"expires_in_seconds": 300})

            first = invite()
            call(b.url, "/api/fleet/join", {"name": "Linux test rig", "address": address, "key": first["key"]})
            call(c.url, "/api/fleet/join", {"name": "Replay test", "address": address, "key": first["key"]}, status=400)
            assert c.view()["setup"]["mode"] == "standalone"
            second = invite()
            call(c.url, "/api/fleet/join", {"name": "Third test rig", "address": address, "key": second["key"]})
            checks.append("Pinned HTTPS pairing; one-use invitation rejects replay")

            def all_online():
                view = a.view()
                return view if len(view["rigs"]) == 3 and all(r["online"] for r in view["rigs"]) else None

            view = eventually(all_online)
            assert len(b.view()["rigs"]) == len(c.view()["rigs"]) == 3
            encoded = json.dumps(view)
            for secret in ["private-fixture-pool-password", "private-fixture-web-token", "/never-exists/", "agent_token", "controller_token", "private_key_pem"]:
                assert secret not in encoded, secret
            checks.append("Same three-rig dashboard on every instance; private fields excluded")

            membership = b.private()["member"]
            context = ssl.create_default_context(cadata=membership["certificate_pem"])
            call(address, "/fleet/v1/view", headers={"Authorization": "Bearer " + membership["agent_token"]}, context=context, status=401)
            call(address, "/fleet/v1/view", headers={"Authorization": "Bearer " + membership["controller_token"]}, context=context)
            try:
                call(address, "/fleet/v1/view")
                raise AssertionError("Untrusted TLS certificate was accepted")
            except urllib.error.URLError:
                pass
            checks.append("TLS trust required; an agent credential cannot control/read the fleet")

            target = identities[1]
            before = call(a.url, f"/api/fleet/rigs/{target}/config")
            revision = before["profiles"][0]["revision"]

            def operation(operation):
                receipts = call(a.url, "/api/fleet/commands", {"rig_ids": [target], "operation": operation})
                receipt_id = receipts[0]["id"]
                def terminal():
                    result = call(a.url, f"/api/fleet/commands/{receipt_id}")
                    return result if result["state"] in ["succeeded", "failed", "expired"] else None
                return eventually(terminal)

            result = operation({"action": "profile_patch", "profile_id": "fixture", "expected_revision": revision,
                                "patch": {"name": "Edited remotely — fixture"}})
            assert result["state"] == "succeeded", result
            actual = json.loads(b.config_file.read_text())
            assert actual["miners"][0]["name"] == "Edited remotely — fixture"
            assert actual["miners"][0]["password"] == "private-fixture-pool-password"
            checks.append("A typed remote edit reaches the other real process and preserves hidden settings")

            result = operation({"action": "profile_patch", "profile_id": "fixture", "expected_revision": revision,
                                "patch": {"name": "Stale overwrite"}})
            assert result["state"] == "failed" and "conflict" in result["error"].lower(), result
            latest = eventually(lambda: (lambda config: config if config["profiles"][0]["name"] == "Edited remotely — fixture" else None)(call(a.url, f"/api/fleet/rigs/{target}/config")))
            result = operation({"action": "profile_patch", "profile_id": "fixture", "expected_revision": latest["profiles"][0]["revision"],
                                "patch": {"executable_path": "/bin/sh", "extra_args": "unapproved"}})
            assert result["state"] == "failed", result
            assert json.loads(b.config_file.read_text())["miners"][0]["executable_path"] == "/never-exists/fleet-fixture-miner"
            checks.append("Conflicting revisions and remote executable/argument changes are rejected")

            result = operation({"action": "start", "profile_id": "fixture"})
            assert result["state"] == "failed", result
            assert all(not item["runtime"]["running"] and item["runtime"]["pid"] is None for item in call(b.url, "/api/status"))
            checks.append("Disabled profile never launches a mining process")

            a.stop()
            outage = eventually(lambda: (lambda v: v if v["setup"]["error"] and all(not r["online"] for r in v["rigs"]) else None)(b.view()))
            assert call(b.url, "/api/health")["scheduler"]
            assert outage["setup"]["mode"] == "member"
            checks.append("Coordinator outage marks cached data offline; local scheduler stays alive")
            a.start()
            eventually(all_online)
            assert a.view()["setup"]["rig_id"] == identities[0]
            checks.append("Coordinator restart preserves identities and members reconnect")

            call(a.url, f"/api/fleet/rigs/{target}/revoke", {})
            call(address, "/fleet/v1/view", headers={"Authorization": "Bearer " + membership["controller_token"]}, context=context, status=401)
            assert call(b.url, "/api/health")["scheduler"]
            checks.append("Revocation removes the controller credential without shutting down its local rig")

            # Restore the fixture membership for optional UI inspection.
            call(b.url, "/api/fleet/leave", {})
            invitation = invite()
            call(b.url, "/api/fleet/join", {"name": "Linux test rig", "address": address, "key": invitation["key"]})
            eventually(all_online)
            third_membership = c.private()["member"]
            third_context = ssl.create_default_context(cadata=third_membership["certificate_pem"])
            left = call(c.url, "/api/fleet/leave", {})
            assert left["setup"]["mode"] == "standalone" and left["setup"]["error"] is None
            assert call(c.url, "/api/health")["fleet_enabled"], "Leaving must not remove current backend autonomy"
            call(address, "/fleet/v1/view", headers={"Authorization": "Bearer " + third_membership["controller_token"]}, context=third_context, status=401)
            checks.append("Leaving revokes the rig's old server credentials and preserves local backend autonomy")
            invitation = invite()
            call(c.url, "/api/fleet/join", {"name": "Third test rig", "address": address, "key": invitation["key"]})
            eventually(all_online)
            print(json.dumps({"result": "passed", "checks": checks, "hub_ui": a.url,
                              "member_ui": b.url, "third_ui": c.url, "mining_processes_started": 0}, indent=2), flush=True)
            if arguments.serve:
                print("Isolated fixture dashboards ready; interrupt this process to clean up.", flush=True)
                while True:
                    time.sleep(1)
        finally:
            for rig in reversed(rigs):
                rig.stop()


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        pass
