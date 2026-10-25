"""Bounded build-space checks and explicit, idle-only Cargo dev-cache maintenance."""

from __future__ import annotations

import argparse
import ctypes
import functools
import hashlib
import json
import math
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GIB = 1024 ** 3
BUILD_NAMES = {"cargo", "rustc", "rustdoc", "clippy-driver", "gcc", "ld", "link", "lld-link", "rust-lld"}


def run(arguments: list[str], root: Path) -> str:
    result = subprocess.run(arguments, cwd=root, text=True, encoding="utf-8",
                            errors="replace", stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"{Path(arguments[0]).name} failed ({result.returncode})")
    return result.stdout


def policy() -> dict:
    value = json.loads((ROOT / "tools/build-resource-policy.json").read_text(encoding="utf-8"))
    if value.get("schema_version") != 1:
        raise ValueError("unsupported build resource policy")
    for key in ("minimum_free_gib", "cache_budget_gib"):
        number = value.get(key)
        if isinstance(number, bool) or not isinstance(number, (int, float)) or not math.isfinite(number) or number <= 0:
            raise ValueError(f"invalid resource threshold: {key}")
    return value


def worktrees(root: Path) -> list[Path]:
    output = run(["git", "worktree", "list", "--porcelain", "-z"], root)
    return [Path(part[9:]).absolute() for part in output.split("\0") if part.startswith("worktree ")]


def linked(path: Path) -> bool:
    state = path.lstat()
    return path.is_symlink() or bool(getattr(state, "st_file_attributes", 0) & 0x400)


def no_links(path: Path) -> None:
    for parent in [path, *path.parents]:
        if parent.exists() or parent.is_symlink():
            if linked(parent):
                raise ValueError(f"linked path refused: {parent}")


def files_below(root: Path):
    no_links(root)
    if not root.exists():
        return
    if root.is_file():
        yield root
        return
    stack = [root]
    while stack:
        directory = stack.pop()
        with os.scandir(directory) as entries:
            for entry in entries:
                path = Path(entry.path)
                if linked(path):
                    raise ValueError(f"linked path refused: {path}")
                if entry.is_dir(follow_symlinks=False):
                    stack.append(path)
                elif entry.is_file(follow_symlinks=False):
                    yield path


@functools.cache
def compressed_size_function():
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    function = kernel.GetCompressedFileSizeW
    function.argtypes = [ctypes.c_wchar_p, ctypes.POINTER(ctypes.c_ulong)]
    function.restype = ctypes.c_ulong
    return function


def allocated_size(path: Path, state) -> int:
    if os.name != "nt":
        return getattr(state, "st_blocks", math.ceil(state.st_size / 512)) * 512
    high = ctypes.c_ulong()
    function = compressed_size_function()
    ctypes.set_last_error(0)
    low = function(str(path), ctypes.byref(high))
    if low == 0xffffffff and ctypes.get_last_error():
        raise OSError(ctypes.get_last_error(), "cannot measure allocated size")
    return (high.value << 32) | low


def size_of(path: Path, seen: set | None = None) -> dict:
    logical, allocated, count, duplicates = 0, 0, 0, 0
    seen = set() if seen is None else seen
    for item in files_below(path):
        state = item.stat()
        logical += state.st_size
        count += 1
        identity = (state.st_dev, state.st_ino) if state.st_ino else str(item)
        if identity in seen:
            duplicates += 1
        else:
            seen.add(identity)
            allocated += allocated_size(item, state)
    return {"logical_bytes": logical, "allocated_bytes": allocated,
            "files": count, "duplicate_hardlinks": duplicates}


def cargo_cache(target: Path) -> bool:
    return ((target / "CACHEDIR.TAG").is_file() or (target / ".rustc_info.json").is_file()
            or ((target / "debug/deps").is_dir() and (target / "debug/.fingerprint").is_dir()))


def targets(root: Path) -> list[Path]:
    base = root / "target"
    no_links(base)
    if not base.exists():
        return []
    result = [base] if cargo_cache(base) else []
    for child in base.iterdir():
        if child.is_dir() and not linked(child) and (child / "debug").is_dir() and cargo_cache(child):
            result.append(child)
    return sorted(result)


def check_space(root: Path, minimum_gib: float) -> dict:
    target = Path(os.environ.get("CARGO_TARGET_DIR") or root / "target")
    if not target.is_absolute():
        target = root / target
    existing = target
    while not existing.exists():
        existing = existing.parent
    usage = shutil.disk_usage(existing)
    return {"schema_version": 1, "status": "PASS" if usage.free >= minimum_gib * GIB else "LOW_SPACE",
            "target": str(target), "free_bytes": usage.free, "minimum_free_bytes": int(minimum_gib * GIB),
            "overrides": sorted(name for name in os.environ if name in {
                "CARGO_INCREMENTAL", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "CARGO_TARGET_DIR"
            } or name.startswith(("CARGO_PROFILE_DEV_", "CARGO_PROFILE_TEST_")))}


def report(root: Path, all_worktrees: bool, budget_gib: float) -> dict:
    rows, errors = [], []
    seen = set()
    for checkout in worktrees(root) if all_worktrees else [root]:
        try:
            caches = targets(checkout)
            for target in caches:
                metrics = size_of(target / "debug", seen)
                rows.append({"worktree": str(checkout), "target": str(target), **metrics})
        except (OSError, ValueError) as error:
            errors.append({"worktree": str(checkout), "error": str(error)})
    totals = {}
    for row in rows:
        totals[row["worktree"]] = totals.get(row["worktree"], 0) + row["allocated_bytes"]
    return {"schema_version": 1, "status": "INCOMPLETE" if errors else "PASS", "caches": rows,
            "budget_bytes": int(budget_gib * GIB), "over_budget": [key for key, size in totals.items()
                                                                      if size > budget_gib * GIB], "errors": errors}


def processes() -> list[dict]:
    if os.name == "nt":
        command = ("Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,"
                   "ExecutablePath,CommandLine | ConvertTo-Json -Compress")
        output = run(["powershell.exe", "-NoProfile", "-NonInteractive", "-Command", command], ROOT)
        data = json.loads(output or "[]")
        return [data] if isinstance(data, dict) else data
    output = run(["ps", "-eo", "pid=,ppid=,comm=,args="], ROOT)
    result = []
    for line in output.splitlines():
        parts = line.strip().split(None, 3)
        if len(parts) == 4:
            result.append({"ProcessId": int(parts[0]), "ParentProcessId": int(parts[1]),
                           "Name": parts[2], "ExecutablePath": parts[2], "CommandLine": parts[3]})
    return result


def normalized(value: str) -> str:
    return value.replace("\\", "/").casefold()


def mentions(value: str, target: Path) -> bool:
    needle = normalized(str(target)).rstrip("/")
    haystack = normalized(value)
    return bool(re.search(re.escape(needle) + r'(?:[/\s"\']|$)', haystack))


def active_users(target: Path, current: list[dict], known_targets: list[Path]) -> list[dict]:
    busy = []
    for process in current:
        name = Path(process.get("Name") or "").stem.casefold()
        executable = process.get("ExecutablePath") or ""
        command = process.get("CommandLine") or ""
        reason = None
        if mentions(executable, target) or (name in BUILD_NAMES and mentions(command, target)):
            reason = "target in use"
        elif name in BUILD_NAMES - {"cargo"} and not command:
            reason = "build process target cannot be established"
        elif name == "cargo":
            explicit = re.search(r'--target-dir(?:=|\s+)(?:"([^"]+)"|(\S+))', command)
            if explicit and Path(explicit.group(1) or explicit.group(2)).is_absolute():
                if mentions(explicit.group(1) or explicit.group(2), target):
                    reason = "Cargo target in use"
            else:
                descendants = {process["ProcessId"]}
                for _ in range(len(current)):
                    expanded = descendants | {p["ProcessId"] for p in current
                                               if p.get("ParentProcessId") in descendants}
                    if expanded == descendants:
                        break
                    descendants = expanded
                observed = {str(candidate) for p in current if p["ProcessId"] in descendants
                            and p["ProcessId"] != process["ProcessId"] for candidate in known_targets
                            if mentions(p.get("ExecutablePath") or "", candidate)
                            or (Path(p.get("Name") or "").stem.casefold() in BUILD_NAMES
                                and mentions(p.get("CommandLine") or "", candidate))}
                if not observed or any(mentions(item, target) or mentions(str(target), Path(item)) for item in observed):
                    reason = "Cargo target is active or cannot be established"
        if reason:
            busy.append({"pid": process["ProcessId"], "name": process.get("Name"), "reason": reason})
    return busy


def validate_target(root: Path, target: Path) -> tuple[Path, list[Path]]:
    if not target.is_absolute() or ".." in target.parts:
        raise ValueError("clean requires a canonical absolute target path")
    no_links(target)
    registered = worktrees(root)
    for checkout in registered:
        base = checkout / "target"
        if target == base or target.parent == base:
            if not (checkout / "Cargo.toml").is_file() or not cargo_cache(target):
                raise ValueError("target has no Cargo cache markers")
            if not (target / "debug/deps").is_dir() or not (target / "debug/.fingerprint").is_dir():
                raise ValueError("dev directory is absent or is not a recognized Cargo cache")
            # Complete traversal also rejects junctions inside the deletion scope.
            for _ in files_below(target / "debug"):
                pass
            known = [candidate / "target" for candidate in registered]
            return checkout, known
    raise ValueError("target is outside this repository's registered checkout-local caches")


def digest_files(paths, base: Path) -> dict:
    digest, count, size = hashlib.sha256(), 0, 0
    for path in sorted(set(paths)):
        no_links(path)
        with path.open("rb") as stream:
            file_hash = hashlib.file_digest(stream, "sha256").hexdigest()
        digest.update(path.relative_to(base).as_posix().encode("utf-8") + b"\0" + file_hash.encode("ascii"))
        count += 1
        size += path.stat().st_size
    return {"files": count, "bytes": size, "sha256": digest.hexdigest()}


def protected_snapshot(checkout: Path, target: Path) -> dict:
    source_names = run(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], checkout)
    source_paths = [checkout / name for name in source_names.split("\0") if name and (checkout / name).is_file()]
    extra = [item for name in (".forme", "research/upstreams") for item in files_below(checkout / name)]
    preserved = []
    for child in target.iterdir():
        if child.name not in {"debug", ".rustc_info.json"}:
            preserved.extend(files_below(child))
    return {"source": digest_files(source_paths, checkout), "runtime_and_research": digest_files(extra, checkout),
            "preserved_target": digest_files(preserved, target),
            "head": run(["git", "rev-parse", "HEAD"], checkout).strip(),
            "git_status_sha256": hashlib.sha256(run(["git", "status", "--porcelain=v1", "-z"], checkout).encode()).hexdigest()}


def clean(root: Path, target: Path, apply: bool) -> dict:
    checkout, known = validate_target(root, target)
    busy = active_users(target, processes(), known)
    if busy:
        return {"schema_version": 1, "status": "BUSY", "target": str(target), "processes": busy}
    arguments = ["cargo", "clean", "--profile", "dev", "--frozen", "--manifest-path",
                 str(checkout / "Cargo.toml"), "--target-dir", str(target)]
    preview = run(arguments + ["--dry-run"], checkout).strip()
    result = {"schema_version": 1, "status": "DRY_RUN", "target": str(target), "preview": preview}
    if not apply:
        return result
    before = protected_snapshot(checkout, target)
    validate_target(root, target)
    busy = active_users(target, processes(), known)
    if busy:
        return {**result, "status": "BUSY", "processes": busy}
    volume = shutil.disk_usage(target).free
    output = run(arguments, checkout).strip()
    after = protected_snapshot(checkout, target)
    remaining = size_of(target / "debug")
    return {**result, "status": "CLEANED" if before == after and not remaining["files"] else "PRESERVATION_FAILED",
            "cargo_result": output, "free_space_delta_bytes": shutil.disk_usage(target).free - volume,
            "preserved_before": before, "preserved_after": after, "remaining_dev_files": remaining["files"]}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "report", "clean"))
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--all-worktrees", action="store_true")
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        settings = policy()
        root = args.root.absolute()
        if root not in worktrees(ROOT):
            raise ValueError("root must be a registered worktree of this repository")
        if args.command == "clean":
            if args.target_dir is None:
                raise ValueError("clean requires --target-dir")
            result = clean(root, args.target_dir, args.apply)
        elif args.apply or args.target_dir:
            raise ValueError("--apply and --target-dir are only valid with clean")
        elif args.command == "check":
            result = check_space(root, settings["minimum_free_gib"])
        else:
            result = report(root, args.all_worktrees, settings["cache_budget_gib"])
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result = {"schema_version": 1, "status": "REFUSED", "error": str(error)}
    if args.json or args.command == "clean":
        print(json.dumps(result, ensure_ascii=False, indent=2))
    elif args.command == "check":
        print(f"build-resources: {result['status']}" + (f" ({result['free_bytes'] / GIB:.1f} GiB free)" if "free_bytes" in result else ""))
        if result.get("overrides"):
            print("build-resources: override variable names: " + ", ".join(result["overrides"]))
        if result.get("error"):
            print(result["error"])
    else:
        print(f"build-resources: {result['status']}")
        for row in result.get("caches", []):
            print(f"  {row['target']}: {row['logical_bytes'] / GIB:.2f} GiB logical, {row['allocated_bytes'] / GIB:.2f} GiB allocated")
        for checkout in result.get("over_budget", []):
            print(f"build-resources: cache budget exceeded: {checkout}")
        for error in result.get("errors", []):
            print(error)
    return 0 if result["status"] in {"PASS", "DRY_RUN", "CLEANED"} else 1


if __name__ == "__main__":
    sys.exit(main())
