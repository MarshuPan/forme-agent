#!/usr/bin/env python3
"""Blocking originality checks for prd/17. Uses only the Python standard library."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
from collections import defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable


SCHEMA_VERSION = 1
TOOL_VERSION = "forme-compliance-doctor/2"
TEXT_EXTENSIONS = {
    ".c",
    ".cc",
    ".cpp",
    ".go",
    ".h",
    ".hpp",
    ".java",
    ".js",
    ".json",
    ".jsx",
    ".kt",
    ".md",
    ".py",
    ".rs",
    ".sh",
    ".swift",
    ".toml",
    ".ts",
    ".tsx",
    ".txt",
    ".yaml",
    ".yml",
}
SKIP_DIRS = {
    ".git",
    ".next",
    ".venv",
    ".worktrees",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "target",
    "venv",
}
SKIP_FILES = {
    "cargo.lock",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
}
COMMON_STRUCTURE_NAMES = {
    ".gitignore",
    "cargo.toml",
    "lib.rs",
    "main.rs",
    "mod.rs",
    "readme.md",
}
COMMON_IDENTIFIERS = {
    "deserialize",
    "from_iter",
    "hash_map",
    "iterator",
    "serialize",
    "schema_version",
    "to_string",
}
MAX_TEXT_BYTES = 2_000_000
MAX_FINDINGS_PER_SCOPE = 50
TOKEN_WINDOW = 20
COMMON_RUST_DERIVES = {
    "Clone",
    "Copy",
    "Debug",
    "Default",
    "Deserialize",
    "Eq",
    "Hash",
    "Ord",
    "PartialEq",
    "PartialOrd",
    "Serialize",
}


@dataclass(frozen=True)
class Finding:
    scope: str
    code: str
    path: str
    detail: str


@dataclass
class CheckResult:
    scope: str
    outcome: str
    blocking: bool
    findings: list[Finding]


@dataclass
class DoctorReport:
    schema_version: int
    outcome: str
    blocking: bool
    checks: list[CheckResult]


@dataclass(frozen=True)
class BorrowingRecord:
    source: str
    license: str
    copy_risk: str
    implementation_rule: str
    notice_action: str
    allowed_paths: frozenset[str]
    allowed_sha256: frozenset[str]
    allow_copyleft: bool


@dataclass(frozen=True)
class MainText:
    path: Path
    relative: str
    text: str
    raw: bytes
    digest: str
    normalized_digest: str
    normalized_length: int


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _iter_files(root: Path) -> Iterable[Path]:
    if not root.exists():
        return
    for current, dirs, files in os.walk(root):
        dirs[:] = sorted(directory for directory in dirs if directory.lower() not in SKIP_DIRS)
        for filename in sorted(files):
            if filename.lower() in SKIP_FILES:
                continue
            path = Path(current, filename)
            if path.is_file():
                yield path


def _main_files(root: Path) -> list[Path]:
    paths: list[Path] = []
    cargo = root / "Cargo.toml"
    if cargo.is_file():
        paths.append(cargo)
    for directory in (root / "crates", root / "tools", root / ".github"):
        paths.extend(_iter_files(directory) or [])
    excluded = {
        (root / "tools" / "compliance_doctor.py").resolve(),
        (root / "tools" / "compliance-doctor.sh").resolve(),
    }
    return sorted(path for path in paths if path.resolve() not in excluded)


def _build_files(root: Path) -> list[Path]:
    paths = [root / "Cargo.toml"]
    for path in _iter_files(root / "crates") or []:
        if path.suffix.lower() in {".rs", ".toml"} or path.name == "build.rs":
            paths.append(path)
    cargo_config = root / ".cargo" / "config.toml"
    if cargo_config.is_file():
        paths.append(cargo_config)
    return [path for path in paths if path.is_file()]


def _read_bytes(path: Path) -> bytes | None:
    try:
        if path.stat().st_size > MAX_TEXT_BYTES:
            return None
        return path.read_bytes()
    except OSError:
        return None


def _decode(raw: bytes) -> str | None:
    if b"\x00" in raw[:4096]:
        return None
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        try:
            return raw.decode("utf-8-sig")
        except UnicodeDecodeError:
            return None


def _normalize_text(text: str) -> str:
    return "\n".join(line.strip() for line in text.replace("\r\n", "\n").split("\n") if line.strip())


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _hash_text(value: str) -> str:
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _tokens(text: str) -> list[str]:
    return re.findall(r"[A-Za-z_][A-Za-z0-9_]{2,}|[\u4e00-\u9fff]{2,}", text.lower())


def _identifiers(text: str) -> set[str]:
    return {
        value
        for value in re.findall(r"\b[A-Za-z_][A-Za-z0-9_]{7,}\b", text)
        if value.lower() not in COMMON_IDENTIFIERS
    }


def _is_common_rust_derive(line: str) -> bool:
    match = re.fullmatch(r"#\[derive\(([^()]*)\)\]", line.strip())
    if match is None:
        return False
    derives = {value.strip() for value in match.group(1).split(",")}
    return bool(derives) and derives <= COMMON_RUST_DERIVES


def _long_lines(text: str) -> set[str]:
    return {
        re.sub(r"\s+", " ", line.strip())
        for line in text.splitlines()
        if 80 <= len(line.strip()) <= 2_000
        and not line.lstrip().startswith(("http://", "https://"))
        and not _is_common_rust_derive(line)
    }


def _resolve_upstream_root(root: Path) -> Path | None:
    local = root / "research" / "upstreams"
    if local.is_dir():
        return local.resolve()

    git_marker = root / ".git"
    if not git_marker.is_file():
        return None
    try:
        marker = git_marker.read_text(encoding="utf-8").strip()
        if not marker.lower().startswith("gitdir:"):
            return None
        git_dir = Path(marker.split(":", 1)[1].strip())
        if not git_dir.is_absolute():
            git_dir = root / git_dir
        git_dir = git_dir.resolve()
        common_marker = git_dir / "commondir"
        common_dir = git_dir
        if common_marker.is_file():
            common_dir = (git_dir / common_marker.read_text(encoding="utf-8").strip()).resolve()
        shared = common_dir.parent / "research" / "upstreams"
        return shared.resolve() if shared.is_dir() else None
    except OSError:
        return None


def upstream_corpus_digest(root: Path) -> str:
    root = root.resolve()
    upstream_root = _resolve_upstream_root(root)
    digest = hashlib.sha256()
    if upstream_root is None:
        digest.update(b"<missing-upstream-corpus>")
        return f"sha256:{digest.hexdigest()}"
    for path in _iter_files(upstream_root) or []:
        relative = _relative(path, upstream_root)
        digest.update(relative.encode("utf-8"))
        digest.update(b"\0")
        raw = _read_bytes(path)
        if raw is None:
            try:
                size = path.stat().st_size
            except OSError:
                size = -1
            digest.update(f"<unreadable-or-oversize:{size}>".encode("ascii"))
        else:
            digest.update(hashlib.sha256(raw).digest())
        digest.update(b"\0")
    return f"sha256:{digest.hexdigest()}"


def _load_records(root: Path) -> tuple[list[BorrowingRecord], list[Finding]]:
    path = root / "compliance" / "borrowing-records.json"
    if not path.exists():
        return [], []
    findings: list[Finding] = []
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [], [Finding("license", "BORROWING_RECORD_INVALID", _relative(path, root), str(error))]
    if payload.get("schema_version") != SCHEMA_VERSION or not isinstance(payload.get("records"), list):
        return [], [
            Finding(
                "license",
                "BORROWING_RECORD_INVALID",
                _relative(path, root),
                "schema_version=1 and a records array are required",
            )
        ]
    records: list[BorrowingRecord] = []
    for index, item in enumerate(payload["records"]):
        required = ("source", "license", "copy_risk", "implementation_rule", "notice_action")
        missing = [field for field in required if not isinstance(item.get(field), str) or not item[field].strip()]
        if item.get("copy_risk") not in {"none", "low", "medium", "high"}:
            missing.append("copy_risk(enum)")
        if missing:
            findings.append(
                Finding(
                    "license",
                    "BORROWING_RECORD_INCOMPLETE",
                    _relative(path, root),
                    f"record {index} is missing or invalid: {', '.join(sorted(set(missing)))}",
                )
            )
            continue
        records.append(
            BorrowingRecord(
                source=item["source"].strip(),
                license=item["license"].strip(),
                copy_risk=item["copy_risk"],
                implementation_rule=item["implementation_rule"].strip(),
                notice_action=item["notice_action"].strip(),
                allowed_paths=frozenset(str(value).replace("\\", "/") for value in item.get("allowed_paths", [])),
                allowed_sha256=frozenset(str(value).lower() for value in item.get("allowed_sha256", [])),
                allow_copyleft=bool(item.get("allow_copyleft", False)),
            )
        )
    return records, findings


def _copy_allowed(relative: str, digest: str, records: list[BorrowingRecord]) -> bool:
    return any(relative in record.allowed_paths and digest in record.allowed_sha256 for record in records)


def _check_upstream_isolation(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    upstream_path = re.compile(r"research[\\/]upstreams", re.IGNORECASE)
    for path in _build_files(root):
        raw = _read_bytes(path)
        text = _decode(raw) if raw is not None else None
        if text is None:
            continue
        for line_number, line in enumerate(text.splitlines(), start=1):
            if upstream_path.search(line):
                findings.append(
                    Finding(
                        "upstream",
                        "UPSTREAM_BUILD_REFERENCE",
                        f"{_relative(path, root)}:{line_number}",
                        "main build/import/include path references research material",
                    )
                )
                if len(findings) >= MAX_FINDINGS_PER_SCOPE:
                    return findings
    return findings


def _check_license(root: Path, records: list[BorrowingRecord], record_findings: list[Finding]) -> list[Finding]:
    findings = list(record_findings)
    gpl = re.compile(r"SPDX-License-Identifier:\s*(?:A?GPL)|GNU (?:AFFERO )?GENERAL PUBLIC LICENSE", re.IGNORECASE)
    for path in _main_files(root):
        raw = _read_bytes(path)
        text = _decode(raw) if raw is not None else None
        if text is not None and gpl.search(text):
            relative = _relative(path, root)
            digest = _sha(raw or b"")
            allowed = any(
                record.allow_copyleft
                and relative in record.allowed_paths
                and digest in record.allowed_sha256
                and record.notice_action.lower() not in {"none", "n/a"}
                for record in records
            )
            if not allowed:
                findings.append(
                    Finding(
                        "license",
                        "COPYLEFT_MATERIAL",
                        relative,
                        "GPL/AGPL material is not covered by an explicit reviewed record",
                    )
                )
        relative_parts = {part.lower() for part in path.relative_to(root).parts}
        if relative_parts.intersection({"vendor", "third_party", "external"}):
            relative = _relative(path, root)
            digest = _sha(raw or b"")
            if not _copy_allowed(relative, digest, records):
                findings.append(
                    Finding(
                        "license",
                        "UNKNOWN_VENDOR_LICENSE",
                        relative,
                        "vendored material needs a path+digest borrowing record",
                    )
                )
        if len(findings) >= MAX_FINDINGS_PER_SCOPE:
            break
    return findings[:MAX_FINDINGS_PER_SCOPE]


def _load_main_text(root: Path) -> list[MainText]:
    loaded: list[MainText] = []
    for path in _main_files(root):
        if path.suffix.lower() not in TEXT_EXTENSIONS and path.name != "Cargo.toml":
            continue
        raw = _read_bytes(path)
        text = _decode(raw) if raw is not None else None
        if raw is None or text is None:
            continue
        normalized = _normalize_text(text)
        if len(normalized) < 40:
            continue
        loaded.append(
            MainText(
                path=path,
                relative=_relative(path, root),
                text=text,
                raw=raw,
                digest=_sha(raw),
                normalized_digest=_hash_text(normalized),
                normalized_length=len(normalized),
            )
        )
    return loaded


def _directory_signature_index(
    main: list[MainText],
) -> tuple[dict[str, set[str]], dict[str, list[tuple[str, str]]]]:
    signatures: dict[str, set[str]] = defaultdict(set)
    for item in main:
        parts = Path(item.relative).parts
        if len(parts) < 3 or parts[0] != "crates":
            continue
        module = "/".join(parts[:2])
        relative = "/".join(parts[2:])
        if Path(relative).name.lower() not in COMMON_STRUCTURE_NAMES:
            signatures[module].add(relative)
    by_basename: dict[str, list[tuple[str, str]]] = defaultdict(list)
    for module, relatives in signatures.items():
        for relative in relatives:
            by_basename[Path(relative).name].append((module, relative))
    return signatures, by_basename


def _check_copy(root: Path, records: list[BorrowingRecord]) -> list[Finding]:
    upstream_root = _resolve_upstream_root(root)
    if upstream_root is None:
        return [
            Finding(
                "copy",
                "UPSTREAM_CORPUS_MISSING",
                "research/upstreams",
                "copy detection requires the local upstream corpus in this or the main worktree",
            )
        ]
    main = _load_main_text(root)
    findings: list[Finding] = []
    exact_by_size: dict[int, dict[str, MainText]] = defaultdict(dict)
    normalized: dict[str, MainText] = {}
    long_lines: dict[str, MainText] = {}
    token_windows: dict[tuple[str, ...], tuple[MainText, str]] = {}
    identifiers: dict[str, set[str]] = {}
    identifier_index: dict[str, set[str]] = defaultdict(set)
    for item in main:
        if _copy_allowed(item.relative, item.digest, records):
            continue
        exact_by_size[len(item.raw)][item.digest] = item
        normalized[item.normalized_digest] = item
        for line in _long_lines(item.text):
            long_lines.setdefault(line, item)
        tokens = _tokens(item.text)
        for index in range(0, max(0, len(tokens) - TOKEN_WINDOW + 1)):
            window = tuple(tokens[index : index + TOKEN_WINDOW])
            token_windows.setdefault(window, (item, " ".join(window)[:180]))
        identifiers[item.relative] = _identifiers(item.text)
        if len(identifiers[item.relative]) >= 12:
            for identifier in identifiers[item.relative]:
                identifier_index[identifier].add(item.relative)

    reported: set[tuple[str, str]] = set()
    module_signatures, directory_index = _directory_signature_index(main)
    directory_matches: dict[tuple[str, str], set[str]] = defaultdict(set)
    normalized_lengths = [item.normalized_length for item in main]
    min_normalized = min(normalized_lengths, default=40)
    max_normalized = max(normalized_lengths, default=MAX_TEXT_BYTES)
    for upstream_path in _iter_files(upstream_root) or []:
        if len(findings) >= MAX_FINDINGS_PER_SCOPE:
            break
        try:
            size = upstream_path.stat().st_size
        except OSError:
            continue
        upstream_relative = _relative(upstream_path, upstream_root)
        for module, relative in directory_index.get(upstream_path.name, []):
            if upstream_relative == relative or upstream_relative.endswith("/" + relative):
                base = upstream_relative[: -len(relative)].rstrip("/") or "."
                directory_matches[(module, base)].add(relative)
        raw: bytes | None = None
        if size in exact_by_size:
            raw = _read_bytes(upstream_path)
            if raw is not None:
                digest = _sha(raw)
                item = exact_by_size[size].get(digest)
                if item is not None:
                    key = (item.relative, "EXACT_FILE_COPY")
                    if key not in reported:
                        findings.append(
                            Finding(
                                "copy",
                                "EXACT_FILE_COPY",
                                item.relative,
                                f"matches {_relative(upstream_path, upstream_root)}",
                            )
                        )
                        reported.add(key)
        if upstream_path.suffix.lower() not in TEXT_EXTENSIONS or not (32 <= size <= MAX_TEXT_BYTES):
            continue
        if raw is None:
            raw = _read_bytes(upstream_path)
        text = _decode(raw) if raw is not None else None
        if text is None:
            continue
        normalized_text = _normalize_text(text)
        if min_normalized <= len(normalized_text) <= max_normalized:
            item = normalized.get(_hash_text(normalized_text))
            if item is not None:
                key = (item.relative, "NORMALIZED_FILE_COPY")
                if key not in reported:
                    findings.append(
                        Finding(
                            "copy",
                            "NORMALIZED_FILE_COPY",
                            item.relative,
                            f"normalizes to {_relative(upstream_path, upstream_root)}",
                        )
                    )
                    reported.add(key)
        for line in _long_lines(text):
            item = long_lines.get(line)
            if item is not None:
                key = (item.relative, "LONG_EXPRESSION_COPY")
                if key not in reported:
                    findings.append(
                        Finding(
                            "copy",
                            "LONG_EXPRESSION_COPY",
                            item.relative,
                            f"shares a long prompt/error/document expression with {_relative(upstream_path, upstream_root)}",
                        )
                    )
                    reported.add(key)
                break
        upstream_tokens = _tokens(text)
        for index in range(0, max(0, len(upstream_tokens) - TOKEN_WINDOW + 1)):
            match = token_windows.get(tuple(upstream_tokens[index : index + TOKEN_WINDOW]))
            if match is not None:
                item, excerpt = match
                key = (item.relative, "TOKEN_SEQUENCE_COPY")
                if key not in reported:
                    findings.append(
                        Finding(
                            "copy",
                            "TOKEN_SEQUENCE_COPY",
                            item.relative,
                            f"shares a {TOKEN_WINDOW}-token sequence with {_relative(upstream_path, upstream_root)}: {excerpt}",
                        )
                    )
                    reported.add(key)
                break
        upstream_ids = _identifiers(text)
        overlap_counts: dict[str, int] = defaultdict(int)
        for identifier in upstream_ids:
            for relative in identifier_index.get(identifier, ()):
                overlap_counts[relative] += 1
        for relative, overlap_count in overlap_counts.items():
            if overlap_count >= 12 and overlap_count / len(identifiers[relative]) >= 0.75:
                key = (relative, "NAMING_SYSTEM_OVERLAP")
                if key not in reported:
                    findings.append(
                        Finding(
                            "copy",
                            "NAMING_SYSTEM_OVERLAP",
                            relative,
                            f"shares {overlap_count} distinctive identifiers with {upstream_relative}",
                        )
                    )
                    reported.add(key)

    for (module, base), paths in directory_matches.items():
        if len(paths) >= 3 and len(paths) / max(1, len(module_signatures[module])) >= 0.6:
            findings.append(
                Finding(
                    "copy",
                    "DIRECTORY_STRUCTURE_MIRROR",
                    module,
                    f"{len(paths)} distinctive paths mirror upstream subtree {base}",
                )
            )
        if len(findings) >= MAX_FINDINGS_PER_SCOPE:
            break
    return findings[:MAX_FINDINGS_PER_SCOPE]


def run_doctor(root: Path, allow_missing_corpus: bool = False) -> DoctorReport:
    root = root.resolve()
    records, record_findings = _load_records(root)
    findings_by_scope = {
        "upstream": _check_upstream_isolation(root),
        "license": _check_license(root, records, record_findings),
        "copy": _check_copy(root, records),
    }
    checks = []
    for scope, findings in findings_by_scope.items():
        corpus_absent = [finding.code for finding in findings] == ["UPSTREAM_CORPUS_MISSING"]
        if scope == "copy" and allow_missing_corpus and corpus_absent:
            # Hosted CI never has the local corpus: report the gap explicitly, never as a pass.
            checks.append(CheckResult(scope=scope, outcome="skip", blocking=False, findings=findings))
            continue
        checks.append(
            CheckResult(
                scope=scope,
                outcome="pass" if not findings else "fail",
                blocking=bool(findings),
                findings=findings,
            )
        )
    blocking = any(check.blocking for check in checks)
    return DoctorReport(
        schema_version=SCHEMA_VERSION,
        outcome="fail" if blocking else "pass",
        blocking=blocking,
        checks=checks,
    )


def _report_dict(report: DoctorReport) -> dict[str, object]:
    return {
        "schema_version": report.schema_version,
        "outcome": report.outcome,
        "blocking": report.blocking,
        "checks": [
            {
                "scope": check.scope,
                "outcome": check.outcome,
                "blocking": check.blocking,
                "findings": [asdict(finding) for finding in check.findings],
            }
            for check in report.checks
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--json", action="store_true")
    parser.add_argument(
        "--allow-missing-corpus",
        action="store_true",
        help="hosted CI only: skip copy detection when the local corpus is absent",
    )
    args = parser.parse_args(argv)
    report = run_doctor(args.root, allow_missing_corpus=args.allow_missing_corpus)
    if args.json:
        print(json.dumps(_report_dict(report), ensure_ascii=True, sort_keys=True))
    else:
        for check in report.checks:
            print(f"[{check.outcome.upper()}] {check.scope}")
            for finding in check.findings:
                print(f"  {finding.code} {finding.path}: {finding.detail}")
        print(f"compliance-doctor: {report.outcome.upper()}")
    return 1 if report.blocking else 0


if __name__ == "__main__":
    sys.exit(main())
