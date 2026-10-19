#!/usr/bin/env python3
"""Closed, portable wave inventories; runtime receipts do not prove live usefulness."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path


VERSION = 1
SOURCES = ("crates", "tools", "Cargo.toml", "Cargo.lock")
DIGEST = re.compile(r"sha256:[0-9a-f]{64}\Z")
COMMIT = re.compile(r"(?:[0-9a-f]{40}|[0-9a-f]{64})\Z")
BASE = "docs/acceptance"
FILES = {
    "A": {"runtime.json", "entrypoints.json"},
    "B": {"runtime.json", "tick.json", "relevance-comparison.json"},
    "C": {"runtime.json", *[name + ".json" for name in
                              ("patterns", "conflict", "recovery", "isolation", "supervision", "learning")]},
    "D": {"runtime.json", "product-golden.json", "score.json", "report.txt"},
}
ROW_KEYS = {"wave", "id", "package", "target", "test", "include_ignored"}
FORBIDDEN = re.compile(
    r"(?:[A-Za-z]:[\\/]Users[\\/]|/(?:Users|home)/|https?://|"
    r"credential:|secret:|bearer\s+|sk-[A-Za-z0-9_-]{12,}|api[_-]?key\s*[=:])", re.I
)
SECRET_KEYS = {"api_key", "apikey", "authorization", "password", "secret", "credential_env", "base_url"}


def canonical(value) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True, allow_nan=False).encode()


def digest(value: bytes) -> str:
    return "sha256:" + hashlib.sha256(value).hexdigest()


def closed(value, keys):
    if not isinstance(value, dict) or set(value) != set(keys):
        raise ValueError("evidence fields are incomplete or unknown")


def regular(path: Path):
    if not path.is_file() or path.is_symlink() or any(
        parent.is_symlink() or getattr(parent, "is_junction", lambda: False)()
        for parent in (path, *path.parents)
    ):
        raise ValueError("evidence must be a regular file without link indirection")
    if path.stat().st_size > 33_554_432:
        raise ValueError("evidence exceeds size bound")


def load(path: Path):
    regular(path)
    # Reject duplicate object keys and NaN rather than silently interpreting them.
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError("duplicate evidence field")
            result[key] = value
        return result
    return json.loads(path.read_text(encoding="utf-8-sig"), object_pairs_hook=pairs,
                      parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite value")))


def run(root: Path, *args: str) -> str:
    result = subprocess.run(args, cwd=root, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if result.returncode:
        raise ValueError("evidence source or scenario inventory could not be verified")
    return result.stdout


def inventory(root: Path) -> list[dict]:
    value = json.loads(run(root, "powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                           "tools/verify-core-brain-closure.ps1", "-ListScenarios"))
    closed(value, {"schema_version", "scenarios"})
    if type(value["schema_version"]) is not int or value["schema_version"] != VERSION:
        raise ValueError("unsupported scenario inventory")
    rows = value["scenarios"]
    if not isinstance(rows, list) or not 1 <= len(rows) <= 1000:
        raise ValueError("empty or oversized inventory")
    for row in rows:
        closed(row, ROW_KEYS)
        if row["wave"] not in FILES or type(row["include_ignored"]) is not bool:
            raise ValueError("invalid scenario identity")
    return rows


def source_digest(root: Path, source: str) -> str:
    if not isinstance(source, str) or not COMMIT.fullmatch(source):
        raise ValueError("invalid evidence source identity")
    run(root, "git", "merge-base", "--is-ancestor", source, "HEAD")
    run(root, "git", "diff", "--quiet", source, "--", *SOURCES)
    if run(root, "git", "ls-files", "--others", "--exclude-standard", "--", *SOURCES).strip():
        raise ValueError("untracked source prevents evidence attestation")
    return digest(run(root, "git", "ls-tree", "-r", source, "--", *SOURCES).encode())


def scan(path: Path):
    regular(path)
    text = path.read_text(encoding="utf-8-sig")
    if FORBIDDEN.search(text):
        raise ValueError("private path, endpoint or secret marker in portable evidence")
    if path.suffix == ".json":
        def walk(value):
            if isinstance(value, dict):
                if SECRET_KEYS.intersection(str(key).lower() for key in value):
                    raise ValueError("secret-bearing field in portable evidence")
                for child in value.values():
                    walk(child)
            elif isinstance(value, list):
                for child in value:
                    walk(child)
            elif isinstance(value, str) and FORBIDDEN.search(value):
                raise ValueError("private path, endpoint or secret marker in decoded evidence")
        walk(load(path))


def runtime_report(root: Path, path: Path, wave: str, rows: list[dict], seal=False):
    value = load(path)
    keys = {"schema_version", "kind", "wave", "source_commit", "scenarios"}
    closed(value, keys if seal else keys | {"input_digest", "inventory_digest"})
    expected = [row for row in rows if row["wave"] == wave]
    if (type(value["schema_version"]) is not int or value["schema_version"] != VERSION
            or value["kind"] != "forme-core-brain-runtime" or value["wave"] != wave):
        raise ValueError("runtime report identity is invalid")
    observed = value["scenarios"]
    if not isinstance(observed, list) or len(observed) != len(expected) or not expected:
        raise ValueError("runtime report scenario set is incomplete")
    for actual, wanted in zip(observed, expected):
        closed(actual, ROW_KEYS | {"passed", "failed", "ignored"})
        if any(actual[key] != wanted[key] or type(actual[key]) is not type(wanted[key]) for key in ROW_KEYS):
            raise ValueError("runtime report differs from the exact current inventory")
        for key, number in (("passed", 1), ("failed", 0), ("ignored", 0)):
            if type(actual[key]) is not int or actual[key] != number:
                raise ValueError("each runtime scenario must actually pass exactly once")
    source = source_digest(root, value["source_commit"])
    selected = digest(canonical(expected))
    if seal:
        value.update(input_digest=source, inventory_digest=selected)
        path.write_bytes(canonical(value) + b"\n")
    elif value["input_digest"] != source or value["inventory_digest"] != selected:
        raise ValueError("runtime source or inventory digest is stale")
    return value


def entrypoint_report(value):
    closed(value, {"schema_version", "kind", "grade", "source_commit", "composition_digest",
                   "taxonomy_digest", "model_calls", "streams"})
    if (type(value["schema_version"]) is not int or value["schema_version"] != VERSION
            or value["kind"] != "forme-core-brain-entrypoints" or value["grade"] != "ControlledRuntime"
            or not COMMIT.fullmatch(value["source_commit"])
            or not DIGEST.fullmatch(value["composition_digest"])
            or not DIGEST.fullmatch(value["taxonomy_digest"])
            or type(value["model_calls"]) is not int or value["model_calls"] < 1
            or not isinstance(value["streams"], list) or len(value["streams"]) != 2):
        raise ValueError("entrypoint evidence identity is invalid")
    bindings, sources, calls, ranges = [], [], 0, []
    for stream in value["streams"]:
        closed(stream, {"run", "source", "session_bound", "positions"})
        positions = stream["positions"]
        if not isinstance(stream["run"], str) or not stream["run"] or not isinstance(positions, list) or not positions:
            raise ValueError("entrypoint event range is absent")
        kinds = []
        for sequence, event in enumerate(positions, 1):
            closed(event, {"seq", "kind", "digest"})
            if (type(event["seq"]) is not int or event["seq"] != sequence
                    or type(event["kind"]) is not str or not DIGEST.fullmatch(event["digest"])):
                raise ValueError("entrypoint positions are discontinuous or invalid")
            kinds.append(event["kind"])
        required = ("RunAccepted", "SessionBound", "ContextBuildStarted", "ContextBuildFinished",
                    "ModelCallStarted", "ModelCallFinished", "RunComplete")
        cursor = -1
        for kind in required:
            cursor = kinds.index(kind, cursor + 1)
        if kinds.count("RunAccepted") != 1 or kinds.count("SessionBound") != 1:
            raise ValueError("duplicate entrypoint lifecycle")
        bound = stream["session_bound"]
        if not isinstance(bound, dict) or bound.get("composition_manifest") != value["composition_digest"]:
            raise ValueError("entrypoint composition snapshot mismatch")
        fields = ("composition_manifest", "policy_profile", "model_profile", "toolset_ref", "workspace")
        if any(not bound.get(key) for key in fields):
            raise ValueError("entrypoint governance snapshot missing")
        bindings.append({key: bound[key] for key in fields})
        sources.append(stream["source"])
        calls += kinds.count("ModelCallStarted")
        ranges.append({"stream": stream["run"], "first": 1, "last": len(positions)})
    if bindings[0] != bindings[1] or sources[0] != "UserTurn" or sources[1] == "UserTurn" or calls != value["model_calls"]:
        raise ValueError("entrypoint equivalence or ground-truth model count mismatch")
    return ranges


def wave_dir(root: Path, wave: str) -> Path:
    return root / BASE / ("core-brain-" + wave.lower() + "-artifacts")


def tick_report(value):
    closed(value, {"schema_version", "kind", "grade", "source_commit", "composition_digest",
                   "cognition_snapshot", "report_digest", "run", "stream_heads", "phases", "positions",
                   "tick_model_calls", "endpoint_calls_before", "endpoint_calls_after"})
    if (type(value["schema_version"]) is not int or value["schema_version"] != VERSION
            or value["kind"] != "forme-core-brain-tick" or value["grade"] != "ControlledRuntime"
            or not COMMIT.fullmatch(value["source_commit"])
            or any(not DIGEST.fullmatch(value[key]) for key in
                   ("composition_digest", "cognition_snapshot", "report_digest"))
            or value["phases"] != ["ObserveAndPropose", "ReflectAndCandidate", "MaintainMemory"]
            or type(value["tick_model_calls"]) is not int or value["tick_model_calls"] != 0
            or type(value["endpoint_calls_before"]) is not int or value["endpoint_calls_before"] < 1
            or value["endpoint_calls_before"] != value["endpoint_calls_after"]
            or type(value["endpoint_calls_after"]) is not int
            or not isinstance(value["stream_heads"], dict) or not value["stream_heads"]
            or not isinstance(value["positions"], list) or not value["positions"]):
        raise ValueError("tick witness identity or phase/model accounting is invalid")
    kinds = []
    for sequence, event in enumerate(value["positions"], 1):
        closed(event, {"seq", "kind", "digest"})
        if (type(event["seq"]) is not int or event["seq"] != sequence
                or not isinstance(event["kind"], str) or not DIGEST.fullmatch(event["digest"])):
            raise ValueError("tick positions are discontinuous")
        kinds.append(event["kind"])
    if (kinds[0] != "ObservationRecorded" or kinds[-1] != "MemoryMaintenanceApplied"
            or not kinds.index("ProactiveProposalEmitted") < kinds.index("ReflectionProduced")
            or value["stream_heads"].get(value["run"]) != len(kinds)):
        raise ValueError("tick phase event ordering or stream head is invalid")


def manifest_body(root: Path, wave: str, rows: list[dict]):
    folder = wave_dir(root, wave)
    if folder.is_symlink() or not folder.is_dir():
        raise ValueError("wave evidence directory missing")
    allowed = FILES[wave]
    found = {path.name for path in folder.iterdir()}
    if found - {"manifest.json", "README.md"} != allowed:
        raise ValueError("wave evidence file set is incomplete or contains extra files")
    receipt = runtime_report(root, folder / "runtime.json", wave, rows)
    files, snapshots, ranges = [], set(), []
    for name in sorted(found - {"manifest.json"}):
        path = folder / name
        scan(path)
        files.append({"path": name, "digest": digest(path.read_bytes())})
        if path.suffix != ".json":
            continue
        value = load(path)
        def refs(item):
            if isinstance(item, dict):
                for key, child in item.items():
                    if isinstance(child, str) and key in {
                        "graph_snapshot", "task_digest", "strategy_snapshot", "composition_digest",
                        "taxonomy_digest", "configuration_digest", "model_profile", "policy_profile", "toolset_ref",
                        "cognition_snapshot", "report_digest"
                    }:
                        snapshots.add((key, child))
                    if key == "stream_heads" and isinstance(child, dict):
                        for stream, last in child.items():
                            if type(last) is not int or last < 1:
                                raise ValueError("invalid typed evidence stream head")
                            ranges.append({"stream": stream, "first": 1, "last": last})
                    refs(child)
            elif isinstance(item, list):
                for child in item:
                    refs(child)
        refs(value)
        if wave == "A" and name == "entrypoints.json":
            source_digest(root, value["source_commit"])
            ranges.extend(entrypoint_report(value))
        if wave == "B" and name == "tick.json":
            source_digest(root, value["source_commit"])
            tick_report(value)
    grade, verdict = "ControlledRuntime", "Pass"
    if wave == "D":
        product = load(folder / "product-golden.json")
        source_digest(root, product["source_commit"])
        grade = product["grade"]
        reported = load(folder / "score.json")["verdict"]
        if reported not in {"Pass", "Fail", "Unverifiable"} or grade not in {"LiveConfigured", "Controlled", "Unavailable"}:
            raise ValueError("invalid product evidence classification")
        verdict = reported if grade == "LiveConfigured" else "Unverifiable"
    return dict(schema_version=VERSION, kind="forme-core-brain-wave-manifest", wave=wave,
                source_commit=receipt["source_commit"], input_digest=receipt["input_digest"],
                inventory_digest=receipt["inventory_digest"], files=files, verdict=verdict, evidence_grade=grade,
                event_ranges=sorted(ranges, key=lambda value: (value["stream"], value["last"])),
                snapshot_refs=[dict(kind=key, ref=value) for key, value in sorted(snapshots)],
                privacy_scan="Pass")


def sealed(body):
    return {**body, "digest": digest(canonical(body))}


def verify_wave(root: Path, wave: str, rows: list[dict]):
    manifest = wave_dir(root, wave) / "manifest.json"
    value = load(manifest)
    expected = sealed(manifest_body(root, wave, rows))
    if canonical(value) != canonical(expected):
        raise ValueError("wave manifest differs from its independently reconstructed evidence")
    scan(manifest)
    return value


def bundle_body(root: Path, rows: list[dict]):
    waves = [verify_wave(root, wave, rows) for wave in FILES]
    if len({value["input_digest"] for value in waves}) != 1:
        raise ValueError("wave reports do not describe the same source inputs")
    verdict = "Fail" if any(wave["verdict"] == "Fail" for wave in waves) else (
        "Pass" if all(wave["verdict"] == "Pass" for wave in waves) else "Unverifiable")
    return dict(schema_version=VERSION, kind="forme-core-brain-closure-manifest",
                input_digest=waves[0]["input_digest"], inventory_digest=digest(canonical(rows)), verdict=verdict,
                waves=[dict(wave=wave["wave"], path=f"{BASE}/core-brain-{wave['wave'].lower()}-artifacts/manifest.json",
                            digest=wave["digest"]) for wave in waves], privacy_scan="Pass")


def verify_bundle(root: Path, rows: list[dict]):
    folder = root / BASE / "core-brain-closure-artifacts"
    if not folder.is_dir() or {path.name for path in folder.iterdir()} != {"manifest.json"}:
        raise ValueError("closure evidence must contain exactly its manifest")
    path = folder / "manifest.json"
    value = load(path)
    if canonical(value) != canonical(sealed(bundle_body(root, rows))):
        raise ValueError("closure evidence references are stale or invalid")
    scan(path)
    return value


def write(path: Path, value):
    if any(parent.is_symlink() or getattr(parent, "is_junction", lambda: False)()
           for parent in path.parents):
        raise ValueError("refusing evidence output through link indirection")
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink() or path.exists() and not path.is_file():
        raise ValueError("refusing evidence output with link or directory indirection")
    path.write_bytes(canonical(value) + b"\n")


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--seal-runtime", choices=FILES)
    mode.add_argument("--capture-wave", choices=FILES)
    mode.add_argument("--assemble", action="store_true")
    mode.add_argument("--verify", action="store_true")
    mode.add_argument("--verify-wave", choices=FILES)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--require-pass", action="store_true")
    args = parser.parse_args(argv)
    try:
        root = args.root.resolve()
        rows = inventory(root)
        if args.seal_runtime:
            if args.runtime is None:
                raise ValueError("runtime output path required")
            runtime_report(root, args.runtime, args.seal_runtime, rows, seal=True)
            print("core-brain runtime receipt: sealed")
            return 0
        if args.capture_wave:
            value = sealed(manifest_body(root, args.capture_wave, rows))
            write(wave_dir(root, args.capture_wave) / "manifest.json", value)
        elif args.assemble:
            value = sealed(bundle_body(root, rows))
            write(root / BASE / "core-brain-closure-artifacts/manifest.json", value)
        elif args.verify_wave:
            value = verify_wave(root, args.verify_wave, rows)
        else:
            value = verify_bundle(root, rows)
        print("core-brain evidence: " + value["verdict"])
        return (2 if value["verdict"] == "Unverifiable" else 1) if args.require_pass and value["verdict"] != "Pass" else 0
    except (OSError, ValueError, KeyError, TypeError, RecursionError):
        print("core-brain evidence: FAIL CLOSED (missing, stale, unsafe or invalid evidence)")
        return 1


if __name__ == "__main__":
    sys.exit(main())
