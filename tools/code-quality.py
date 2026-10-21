"""Repository-owned structural gate; no source contents or secrets in reports."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
EXTENSIONS = {".rs", ".ts", ".tsx", ".js", ".mjs", ".css", ".py", ".ps1", ".sh"}
ROOTS = ("crates/", "frontend/", "tools/")
ALLOWED = {
    "protocol": set(), "store": {"protocol"}, "config": {"protocol"},
    "models": {"protocol"}, "policy": {"protocol"},
    "approval": {"protocol", "policy"},
    "execution": {"protocol", "policy"},
    "capabilities": {"protocol", "policy"},
    "context": {"protocol", "store", "memory"}, "eval": {"protocol", "store"},
    "memory": {"protocol", "store"},
    "loop": {"protocol", "capabilities", "context", "policy", "models"},
    "harness": {"protocol", "store", "policy", "approval", "loop", "coordination",
                "cognition", "context", "models", "capabilities", "execution", "eval"},
    "cognition": {"protocol", "memory", "store"},
    "coordination": {"protocol", "cognition", "capabilities"},
    "communication": {"protocol", "policy", "harness"},
    "gateway": {"protocol", "harness", "communication"},
    "cli": {"protocol", "gateway"},
}


def source_paths(root: Path) -> list[str]:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=root, check=True, stdout=subprocess.PIPE,
    )
    return sorted({
        p for p in result.stdout.decode("utf-8").split("\0")
        if p.startswith(ROOTS) and Path(p).suffix in EXTENSIONS
        and not p.startswith("frontend/dist/") and "/node_modules/" not in p
        and (root / p).is_file()
    })


def limit_for(path: str) -> int:
    stem = Path(path).stem
    return 1200 if "/tests/" in path or stem == "tests" or stem.endswith("_tests") else 600


def file_metrics(root: Path, paths: list[str], baseline: dict) -> tuple[list, list]:
    violations, debt = [], []
    for path in paths:
        count = len((root / path).read_text(encoding="utf-8-sig").splitlines())
        default = limit_for(path)
        entry = baseline.get(path)
        cap = default
        if entry is not None:
            if (not isinstance(entry, dict) or not isinstance(entry.get("max_lines"), int)
                    or isinstance(entry["max_lines"], bool) or entry["max_lines"] <= default
                    or not isinstance(entry.get("reason"), str) or not entry["reason"].strip()):
                violations.append(f"{path}: invalid reviewed debt entry")
                continue
            cap = entry["max_lines"]
        if count > cap:
            violations.append(f"{path}: {count} lines exceeds cap {cap}")
        if count > default:
            debt.append({"path": path, "lines": count, "target": default, "cap": cap})
    return violations, sorted(debt, key=lambda entry: (-entry["lines"], entry["path"]))


def rust_entrypoints(crate: Path) -> list[Path]:
    """Return Rust crate roots, including nested bin/main.rs entrypoints."""
    source = crate / "src"
    roots = [path for path in (source / "lib.rs", source / "main.rs") if path.exists()]
    bin_root = source / "bin"
    if bin_root.exists():
        roots.extend(
            path for path in bin_root.rglob("*.rs")
            if path.parent == bin_root or path.name == "main.rs"
        )
    return sorted(roots)


def dependency_errors(root: Path) -> list[str]:
    errors = []
    manifests = sorted((root / "crates").glob("*/Cargo.toml"))
    if {m.parent.name for m in manifests} != set(ALLOWED):
        errors.append("crate set differs from the frozen 18-crate graph")
    for manifest in manifests:
        name = manifest.parent.name
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        sections = [data]
        sections.extend(data.get("target", {}).values())
        for section in sections:
            for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
                for alias, specification in section.get(kind, {}).items():
                    package = specification.get("package", alias) if isinstance(specification, dict) else alias
                    if package.startswith("forme-"):
                        target = package.removeprefix("forme-")
                        if target not in ALLOWED.get(name, set()):
                            errors.append(f"{name}: forbidden internal dependency {package}")
                    elif isinstance(specification, dict) and "path" in specification:
                        errors.append(f"{name}: unrecognized local dependency {alias}")
        for source in rust_entrypoints(manifest.parent):
            if "#![forbid(unsafe_code)]" not in source.read_text(encoding="utf-8"):
                errors.append(f"{source.relative_to(root).as_posix()}: missing unsafe prohibition")
    return errors


def check(root: Path, baseline: dict) -> dict:
    if baseline.get("schema_version") != 1 or not isinstance(baseline.get("files"), dict):
        raise ValueError("unsupported quality baseline schema")
    paths = source_paths(root)
    errors, debt = file_metrics(root, paths, baseline["files"])
    errors.extend(dependency_errors(root))
    return {"schema_version": 1, "status": "FAIL" if errors else "PASS",
            "checked_files": len(paths), "known_size_debt": debt, "violations": errors}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="emit bounded path/count metadata")
    args = parser.parse_args()
    try:
        baseline = json.loads((ROOT / "tools/quality-baseline.json").read_text(encoding="utf-8"))
        report = check(ROOT, baseline)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"code-quality: cannot establish baseline ({type(error).__name__})", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(report, ensure_ascii=False, indent=2))
    else:
        print(f"code-quality: {report['status']} ({report['checked_files']} files, "
              f"{len(report['known_size_debt'])} existing size debts)")
        for error in report["violations"]:
            print(error)
    return 0 if report["status"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
