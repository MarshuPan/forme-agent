#!/usr/bin/env python3
"""Blocking M3 release audit with content-addressed typed evidence."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import sys
import tomllib
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Callable, Iterable


SCHEMA_VERSION = 1
TOOL_VERSION = "forme-release-audit/3"
ARTIFACT_TYPE = "forme-m3-release-audit"
ARTIFACT_DOMAIN = b"forme-m3-release-audit-v1\0"
ARTIFACT_PREFIX = "docs/acceptance/m3-release-audit-artifacts/"
SELF_REFERENTIAL_EVIDENCE = {
    "docs/acceptance/m3-c-acceptance-report.md",
    "docs/acceptance/m3-acceptance-report.md",
}
EXPECTED_CHECK_IDS = [
    "release_tree",
    "license_notice",
    "direct_dependencies",
    "borrowing_records",
    "upstream_copy",
    "acceptance_artifacts",
    "vulnerability_audit",
]
RELEASE_TARGET = "x86_64-pc-windows-gnu"
MAX_FINDINGS = 100
BAD_LICENSE = re.compile(r"(?:^|[^A-Z])(?:A?GPL|UNKNOWN|UNLICENSED)(?:[^A-Z]|$)", re.I)
PRIVATE_PATH = re.compile(r"(?:[A-Za-z]:\\Users\\[^\\\s]+|/(?:Users|home)/[^/\s]+)", re.I)
SECRET_VALUE = re.compile(
    r"(?:credential:|secret:|bearer\s+|api[_-]?key\s*[=:]|sk-[A-Za-z0-9_-]{12,})",
    re.I,
)
SENSITIVE_KEYS = {
    "api_key",
    "apikey",
    "authorization",
    "credential",
    "password",
    "secret",
    "secret_bindings",
    "token",
}


@dataclass(frozen=True)
class AuditFinding:
    code: str
    path: str
    detail: str


@dataclass(frozen=True)
class ComplianceCheckResult:
    check_id: str
    outcome: str
    blocking: bool
    tool: str
    tool_version: str
    input_digest: str
    findings: list[AuditFinding]
    observations: list[AuditFinding]


@dataclass(frozen=True)
class ReleaseAuditBody:
    schema_version: int
    tool_version: str
    root_digest: str
    outcome: str
    blocking: bool
    checks: list[ComplianceCheckResult]


@dataclass(frozen=True)
class ReleaseAuditArtifact:
    schema_version: int
    artifact_type: str
    digest: str
    body: ReleaseAuditBody


@dataclass(frozen=True)
class ReleaseAuditReceipt:
    path: Path
    digest: str
    content_ref: str


@dataclass(frozen=True)
class DependencyDecl:
    name: str
    version: str
    manifest: str


@dataclass(frozen=True)
class DependencyTableRow:
    name: str
    version: str
    source: str
    repository: str
    license: str
    release_action: str


@dataclass(frozen=True)
class VulnerabilityScan:
    tool: str
    version: str
    completed: bool
    return_code: int
    payload: object | None
    error: str | None


VulnerabilityRunner = Callable[[Path], VulnerabilityScan]


def _sha256(data: bytes) -> str:
    return f"sha256:{hashlib.sha256(data).hexdigest()}"


def _canonical(value: object) -> bytes:
    return json.dumps(value, ensure_ascii=True, sort_keys=True, separators=(",", ":")).encode(
        "utf-8"
    )


def _run(command: list[str], root: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        command,
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def _git_index_entries(root: Path) -> tuple[dict[str, tuple[str, str]], str | None]:
    result = _run(["git", "ls-files", "--stage", "-z"], root)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip() or "git index scan failed"
        return {}, detail
    entries: dict[str, tuple[str, str]] = {}
    for raw in result.stdout.split("\0"):
        if not raw:
            continue
        try:
            metadata, relative = raw.split("\t", 1)
            mode, object_id, stage = metadata.split(" ", 2)
        except ValueError:
            return {}, "git index returned an invalid stage entry"
        if stage != "0":
            return {}, f"git index contains an unmerged entry for {relative}"
        entries[relative] = (mode, object_id)
    return entries, None


def _digest_paths(root: Path, paths: Iterable[str]) -> str:
    index, index_error = _git_index_entries(root)
    digest = hashlib.sha256()
    for relative in sorted(set(paths)):
        normalized = relative.replace("\\", "/")
        digest.update(normalized.encode("utf-8"))
        digest.update(b"\0")
        entry = index.get(normalized)
        if entry is not None:
            mode, object_id = entry
            digest.update(f"git:{mode}:{object_id}".encode("ascii"))
        else:
            path = root / normalized
            if path.is_file():
                digest.update(b"untracked:")
                digest.update(hashlib.sha256(path.read_bytes()).digest())
            else:
                digest.update(b"<missing>")
        digest.update(b"\0")
    if index_error is not None:
        digest.update(b"index-error\0")
        digest.update(index_error.encode("utf-8", errors="replace"))
    return f"sha256:{digest.hexdigest()}"


def _git_files(root: Path) -> tuple[list[str], list[AuditFinding]]:
    result = _run(["git", "ls-files", "-z"], root)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip() or "git ls-files failed"
        return [], [AuditFinding("GIT_TRACKED_SCAN_FAILED", ".", detail)]
    return sorted(path for path in result.stdout.split("\0") if path), []


def _result(
    check_id: str,
    tool: str,
    version: str,
    input_digest: str,
    findings: list[AuditFinding],
    observations: list[AuditFinding] | None = None,
) -> ComplianceCheckResult:
    findings = findings[:MAX_FINDINGS]
    return ComplianceCheckResult(
        check_id=check_id,
        outcome="pass" if not findings else "fail",
        blocking=bool(findings),
        tool=tool,
        tool_version=version,
        input_digest=input_digest,
        findings=findings,
        observations=(observations or [])[:MAX_FINDINGS],
    )


def _release_tree_check(root: Path) -> tuple[ComplianceCheckResult, list[str], str]:
    tracked, findings = _git_files(root)
    status = _run(["git", "status", "--porcelain=v1", "--untracked-files=all"], root)
    if status.returncode != 0:
        findings.append(
            AuditFinding(
                "GIT_STATUS_FAILED",
                ".",
                (status.stderr or status.stdout).strip() or "git status failed",
            )
        )
    elif status.stdout.strip():
        findings.append(
            AuditFinding(
                "RELEASE_TREE_DIRTY",
                ".",
                "tracked, staged, or untracked changes are present",
            )
        )
    for relative in tracked:
        lower = relative.lower()
        name = Path(lower).name
        if lower.startswith("research/upstreams/"):
            findings.append(
                AuditFinding(
                    "UPSTREAM_IN_RELEASE_TREE",
                    relative,
                    "research upstream material is tracked in the release tree",
                )
            )
        if (name.startswith(".env") and name != ".env.example") or name in {
            "gateway.token",
            "id_rsa",
            "id_ed25519",
        } or Path(name).suffix in {".key", ".p12", ".pem", ".pfx"}:
            findings.append(
                AuditFinding(
                    "CREDENTIAL_FILE_TRACKED",
                    relative,
                    "credential-bearing filenames cannot enter the release tree",
                )
            )
    digest_paths = [
        path
        for path in tracked
        if not path.startswith(ARTIFACT_PREFIX) and path not in SELF_REFERENTIAL_EVIDENCE
    ]
    root_digest = _digest_paths(root, digest_paths)
    input_digest = _sha256(
        _canonical(
            {
                "root_digest": root_digest,
                "status": status.stdout if status.returncode == 0 else "scan-failed",
            }
        )
    )
    return _result("release_tree", "git", _tool_version(root, ["git", "--version"]), input_digest, findings), tracked, root_digest


def _tool_version(root: Path, command: list[str]) -> str:
    result = _run(command, root)
    if result.returncode != 0:
        return "unavailable"
    return (result.stdout or result.stderr).strip().splitlines()[0]


def _manifest_dependency_tables(payload: dict[str, object]) -> Iterable[dict[str, object]]:
    for key in ("dependencies", "dev-dependencies", "build-dependencies"):
        value = payload.get(key)
        if isinstance(value, dict):
            yield value
    targets = payload.get("target")
    if isinstance(targets, dict):
        for target in targets.values():
            if not isinstance(target, dict):
                continue
            for key in ("dependencies", "dev-dependencies", "build-dependencies"):
                value = target.get(key)
                if isinstance(value, dict):
                    yield value


def _dependency_declarations(root: Path) -> tuple[list[DependencyDecl], list[AuditFinding], list[str]]:
    declarations: dict[tuple[str, str], DependencyDecl] = {}
    findings: list[AuditFinding] = []
    manifests = sorted((root / "crates").glob("*/Cargo.toml"))
    paths = [_relative(path, root) for path in manifests]
    for manifest in manifests:
        relative = _relative(manifest, root)
        try:
            payload = tomllib.loads(manifest.read_text(encoding="utf-8"))
        except (OSError, tomllib.TOMLDecodeError) as error:
            findings.append(AuditFinding("MANIFEST_INVALID", relative, str(error)))
            continue
        for table in _manifest_dependency_tables(payload):
            for name, specification in table.items():
                if isinstance(specification, str):
                    version = specification
                    path = None
                elif isinstance(specification, dict):
                    version = specification.get("version")
                    path = specification.get("path")
                else:
                    findings.append(
                        AuditFinding(
                            "DEPENDENCY_SPEC_INVALID",
                            relative,
                            f"{name} has an unsupported dependency specification",
                        )
                    )
                    continue
                if path is not None:
                    resolved = (manifest.parent / str(path)).resolve()
                    try:
                        resolved.relative_to((root / "crates").resolve())
                    except ValueError:
                        findings.append(
                            AuditFinding(
                                "EXTERNAL_PATH_DEPENDENCY",
                                relative,
                                f"{name} points outside the project crate tree",
                            )
                        )
                    continue
                if not isinstance(version, str) or not re.fullmatch(
                    r"=[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?", version
                ):
                    findings.append(
                        AuditFinding(
                            "UNPINNED_DEPENDENCY",
                            relative,
                            f"{name} must use one exact =major.minor.patch version",
                        )
                    )
                    normalized = version.lstrip("=") if isinstance(version, str) else "unknown"
                else:
                    normalized = version[1:]
                declaration = DependencyDecl(name, normalized, relative)
                declarations[(name, normalized)] = declaration
    return sorted(declarations.values(), key=lambda item: (item.name, item.version)), findings, paths


def _dependency_table(root: Path) -> tuple[dict[str, DependencyTableRow], list[AuditFinding]]:
    path = root / "docs/compliance/third-party-dependencies.md"
    findings: list[AuditFinding] = []
    rows: dict[str, DependencyTableRow] = {}
    if not path.is_file():
        return rows, [
            AuditFinding(
                "DEPENDENCY_TABLE_MISSING",
                "docs/compliance/third-party-dependencies.md",
                "direct dependency source and license inventory is required",
            )
        ]
    for line_number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.startswith("|"):
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 9 or cells[0] == "Package" or set(cells[0]) <= {"-", ":"}:
            continue
        row = DependencyTableRow(
            name=cells[0],
            version=cells[1],
            source=cells[2],
            repository=cells[3],
            license=cells[4],
            release_action=cells[8],
        )
        if row.name in rows:
            findings.append(
                AuditFinding(
                    "DEPENDENCY_TABLE_DUPLICATE",
                    f"docs/compliance/third-party-dependencies.md:{line_number}",
                    f"duplicate dependency row for {row.name}",
                )
            )
        rows[row.name] = row
    return rows, findings


def _metadata(root: Path) -> tuple[dict[str, object] | None, AuditFinding | None, str]:
    version = _tool_version(root, ["cargo", "--version"])
    result = _run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        root,
    )
    if result.returncode != 0:
        return None, AuditFinding(
            "LOCK_METADATA_FAILED",
            "Cargo.lock",
            (result.stderr or result.stdout).strip() or "cargo metadata failed",
        ), version
    try:
        return json.loads(result.stdout), None, version
    except json.JSONDecodeError as error:
        return None, AuditFinding("LOCK_METADATA_INVALID", "Cargo.lock", str(error)), version


def _dependency_check(root: Path) -> tuple[ComplianceCheckResult, list[DependencyDecl]]:
    declarations, findings, manifest_paths = _dependency_declarations(root)
    rows, row_findings = _dependency_table(root)
    findings.extend(row_findings)
    lock_path = root / "Cargo.lock"
    lock_packages: set[tuple[str, str]] = set()
    if not lock_path.is_file():
        findings.append(AuditFinding("LOCKFILE_MISSING", "Cargo.lock", "Cargo.lock is required"))
    else:
        try:
            lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
            lock_packages = {
                (str(package.get("name")), str(package.get("version")))
                for package in lock.get("package", [])
                if isinstance(package, dict)
            }
        except (OSError, tomllib.TOMLDecodeError) as error:
            findings.append(AuditFinding("LOCKFILE_INVALID", "Cargo.lock", str(error)))
    metadata, metadata_error, cargo_version = _metadata(root)
    if metadata_error is not None:
        findings.append(metadata_error)
    package_metadata: dict[tuple[str, str], dict[str, object]] = {}
    if metadata is not None:
        package_metadata = {
            (str(package.get("name")), str(package.get("version"))): package
            for package in metadata.get("packages", [])
            if isinstance(package, dict) and package.get("source") is not None
        }
    for declaration in declarations:
        key = (declaration.name, declaration.version)
        row = rows.get(declaration.name)
        if key not in lock_packages:
            findings.append(
                AuditFinding(
                    "DIRECT_DEPENDENCY_NOT_LOCKED",
                    declaration.manifest,
                    f"{declaration.name} {declaration.version} is absent from Cargo.lock",
                )
            )
        if row is None:
            findings.append(
                AuditFinding(
                    "DIRECT_DEPENDENCY_UNDOCUMENTED",
                    declaration.manifest,
                    f"{declaration.name} has no third-party inventory row",
                )
            )
            continue
        if row.version != declaration.version:
            findings.append(
                AuditFinding(
                    "DEPENDENCY_TABLE_DRIFT",
                    "docs/compliance/third-party-dependencies.md",
                    f"{declaration.name} table={row.version} manifest={declaration.version}",
                )
            )
        if "crates.io" not in row.source or not row.repository or BAD_LICENSE.search(row.license):
            findings.append(
                AuditFinding(
                    "DEPENDENCY_SOURCE_OR_LICENSE_INVALID",
                    "docs/compliance/third-party-dependencies.md",
                    f"{declaration.name} lacks an allowed source, repository, or license",
                )
            )
        if not row.release_action or row.release_action.lower() in {"none", "n/a"}:
            findings.append(
                AuditFinding(
                    "DEPENDENCY_RELEASE_ACTION_MISSING",
                    "docs/compliance/third-party-dependencies.md",
                    f"{declaration.name} lacks a release action",
                )
            )
        package = package_metadata.get(key)
        if package is not None:
            license_value = str(package.get("license") or "UNKNOWN")
            source_value = str(package.get("source") or "")
            if BAD_LICENSE.search(license_value) or "crates.io-index" not in source_value:
                findings.append(
                    AuditFinding(
                        "LOCKED_DEPENDENCY_LICENSE_OR_SOURCE_BLOCKED",
                        "Cargo.lock",
                        f"{declaration.name} {declaration.version} resolved to {license_value} from {source_value}",
                    )
                )
            if re.sub(r"\s+", "", license_value).lower() != re.sub(
                r"\s+", "", row.license
            ).lower():
                findings.append(
                    AuditFinding(
                        "DEPENDENCY_LICENSE_TABLE_DRIFT",
                        "docs/compliance/third-party-dependencies.md",
                        f"{declaration.name} table={row.license} metadata={license_value}",
                    )
                )
    for row in rows.values():
        if "crates.io" not in row.source:
            continue
        version = row.version.split(" ", 1)[0]
        if (row.name, version) not in lock_packages:
            findings.append(
                AuditFinding(
                    "DEPENDENCY_TABLE_NOT_LOCKED",
                    "docs/compliance/third-party-dependencies.md",
                    f"{row.name} {version} is documented but absent from Cargo.lock",
                )
            )
    paths = manifest_paths + [
        "Cargo.lock",
        "docs/compliance/third-party-dependencies.md",
    ]
    return _result(
        "direct_dependencies",
        "cargo-metadata",
        cargo_version,
        _digest_paths(root, paths),
        findings,
    ), declarations


def _license_notice_check(root: Path, dependencies: list[DependencyDecl]) -> ComplianceCheckResult:
    findings: list[AuditFinding] = []
    license_path = root / "LICENSE"
    notice_path = root / "NOTICE"
    license_text = license_path.read_text(encoding="utf-8") if license_path.is_file() else ""
    notice_text = notice_path.read_text(encoding="utf-8") if notice_path.is_file() else ""
    if not license_text:
        findings.append(AuditFinding("LICENSE_MISSING", "LICENSE", "root project license is required"))
    elif BAD_LICENSE.search(license_text) or not (
        "Apache License" in license_text and "Version 2.0" in license_text
    ):
        findings.append(
            AuditFinding(
                "PROJECT_LICENSE_UNSUPPORTED",
                "LICENSE",
                "the frozen release profile requires the Apache-2.0 project license",
            )
        )
    if not notice_text:
        findings.append(AuditFinding("NOTICE_MISSING", "NOTICE", "root NOTICE is required"))
    else:
        if "forme" not in notice_text.lower() or "forme contributors" not in notice_text:
            findings.append(
                AuditFinding(
                    "NOTICE_PROJECT_ATTRIBUTION_MISSING",
                    "NOTICE",
                    "NOTICE must identify forme and its contributors",
                )
            )
        for dependency in dependencies:
            if f"{dependency.name} {dependency.version}" not in notice_text:
                findings.append(
                    AuditFinding(
                        "NOTICE_DEPENDENCY_MISSING",
                        "NOTICE",
                        f"{dependency.name} {dependency.version} is absent from NOTICE",
                    )
                )
    workspace_path = root / "Cargo.toml"
    if workspace_path.is_file():
        try:
            workspace = tomllib.loads(workspace_path.read_text(encoding="utf-8"))
            declared = workspace.get("workspace", {}).get("package", {}).get("license")
            if declared != "Apache-2.0":
                findings.append(
                    AuditFinding(
                        "WORKSPACE_LICENSE_DRIFT",
                        "Cargo.toml",
                        "workspace.package.license must be Apache-2.0",
                    )
                )
        except (OSError, tomllib.TOMLDecodeError) as error:
            findings.append(AuditFinding("WORKSPACE_MANIFEST_INVALID", "Cargo.toml", str(error)))
    return _result(
        "license_notice",
        "forme-license-review",
        TOOL_VERSION,
        _digest_paths(root, ["Cargo.toml", "LICENSE", "NOTICE"]),
        findings,
    )


def _borrowing_check(root: Path) -> ComplianceCheckResult:
    relative = "compliance/borrowing-records.json"
    path = root / relative
    findings: list[AuditFinding] = []
    if not path.is_file():
        findings.append(
            AuditFinding("BORROWING_RECORDS_MISSING", relative, "borrowing records are required")
        )
    else:
        try:
            payload = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            payload = None
            findings.append(AuditFinding("BORROWING_RECORDS_INVALID", relative, str(error)))
        if isinstance(payload, dict):
            records = payload.get("records")
            if payload.get("schema_version") != 1 or not isinstance(records, list):
                findings.append(
                    AuditFinding(
                        "BORROWING_RECORDS_INVALID",
                        relative,
                        "schema_version=1 and a records array are required",
                    )
                )
            else:
                for index, record in enumerate(records):
                    if not isinstance(record, dict):
                        findings.append(
                            AuditFinding(
                                "BORROWING_RECORD_INVALID",
                                relative,
                                f"record {index} is not an object",
                            )
                        )
                        continue
                    required = (
                        "source",
                        "license",
                        "copy_risk",
                        "implementation_rule",
                        "notice_action",
                    )
                    missing = [
                        key
                        for key in required
                        if not isinstance(record.get(key), str) or not record[key].strip()
                    ]
                    if missing or record.get("copy_risk") not in {"none", "low", "medium", "high"}:
                        findings.append(
                            AuditFinding(
                                "BORROWING_RECORD_INCOMPLETE",
                                relative,
                                f"record {index} is incomplete",
                            )
                        )
                    license_value = str(record.get("license", "UNKNOWN"))
                    if BAD_LICENSE.search(license_value):
                        findings.append(
                            AuditFinding(
                                "BORROWING_LICENSE_BLOCKED",
                                relative,
                                f"record {index} uses blocked or unknown license {license_value}",
                            )
                        )
                    paths = record.get("allowed_paths", [])
                    digests = record.get("allowed_sha256", [])
                    if not isinstance(paths, list) or not isinstance(digests, list) or len(paths) != len(digests):
                        findings.append(
                            AuditFinding(
                                "BORROWING_ALLOWLIST_INVALID",
                                relative,
                                f"record {index} path and digest allowlists must be paired",
                            )
                        )
                    elif any(not re.fullmatch(r"[0-9a-fA-F]{64}", str(value)) for value in digests):
                        findings.append(
                            AuditFinding(
                                "BORROWING_ALLOWLIST_INVALID",
                                relative,
                                f"record {index} contains a non-SHA256 allow entry",
                            )
                        )
    return _result(
        "borrowing_records",
        "forme-borrowing-review",
        TOOL_VERSION,
        _digest_paths(root, [relative]),
        findings,
    )


def _load_doctor() -> object:
    path = Path(__file__).with_name("compliance_doctor.py")
    name = "forme_release_audit_compliance_doctor"
    if name in sys.modules:
        return sys.modules[name]
    specification = importlib.util.spec_from_file_location(name, path)
    if specification is None or specification.loader is None:
        raise RuntimeError("compliance doctor module could not be loaded")
    module = importlib.util.module_from_spec(specification)
    sys.modules[name] = module
    specification.loader.exec_module(module)
    return module


def _originality_check(root: Path, root_digest: str) -> ComplianceCheckResult:
    findings: list[AuditFinding] = []
    doctor = None
    corpus_digest = "sha256:" + hashlib.sha256(b"<scanner-load-failed>").hexdigest()
    try:
        doctor = _load_doctor()
        corpus_digest = doctor.upstream_corpus_digest(root)
        report = doctor.run_doctor(root)
        for check in report.checks:
            for finding in check.findings:
                findings.append(
                    AuditFinding(
                        f"ORIGINALITY_{finding.code}",
                        finding.path,
                        finding.detail,
                    )
                )
        if report.blocking and not findings:
            findings.append(
                AuditFinding(
                    "ORIGINALITY_SCAN_FAILED_CLOSED",
                    ".",
                    "compliance doctor returned blocking without typed findings",
                )
            )
    except Exception as error:  # fail closed across scanner loading and execution
        findings.append(AuditFinding("ORIGINALITY_SCAN_FAILED", ".", str(error)))
    return _result(
        "upstream_copy",
        "compliance-doctor",
        doctor.TOOL_VERSION if doctor is not None else "unavailable",
        _sha256(
            _canonical(
                {
                    "release_tree_digest": root_digest,
                    "upstream_corpus_digest": corpus_digest,
                }
            )
        ),
        findings,
    )


def _has_value(value: object) -> bool:
    return value not in (None, "", [], {})


def _scan_json_value(value: object, relative: str, pointer: str, findings: list[AuditFinding]) -> None:
    if len(findings) >= MAX_FINDINGS:
        return
    if isinstance(value, dict):
        for key, nested in value.items():
            next_pointer = f"{pointer}/{key}"
            if key.lower() in SENSITIVE_KEYS and _has_value(nested):
                findings.append(
                    AuditFinding(
                        "ARTIFACT_SECRET_FIELD",
                        relative,
                        f"non-empty sensitive field at {next_pointer}",
                    )
                )
            _scan_json_value(nested, relative, next_pointer, findings)
    elif isinstance(value, list):
        for index, nested in enumerate(value):
            _scan_json_value(nested, relative, f"{pointer}/{index}", findings)
    elif isinstance(value, str):
        if SECRET_VALUE.search(value):
            findings.append(
                AuditFinding(
                    "ARTIFACT_SECRET_VALUE",
                    relative,
                    f"secret-like value at {pointer}",
                )
            )
        if PRIVATE_PATH.search(value):
            findings.append(
                AuditFinding(
                    "ARTIFACT_PRIVATE_PATH",
                    relative,
                    f"private absolute path at {pointer}",
                )
            )


def _artifact_check(root: Path, tracked: list[str]) -> ComplianceCheckResult:
    findings: list[AuditFinding] = []
    artifacts = sorted(
        relative
        for relative in tracked
        if relative.startswith("docs/acceptance/")
        and relative.endswith(".json")
        and not relative.startswith(ARTIFACT_PREFIX)
    )
    if not artifacts:
        findings.append(
            AuditFinding(
                "ACCEPTANCE_ARTIFACTS_MISSING",
                "docs/acceptance",
                "release acceptance requires repository-owned JSON evidence",
            )
        )
    for relative in artifacts:
        path = root / relative
        if path.is_symlink() or not path.is_file():
            findings.append(
                AuditFinding(
                    "ARTIFACT_NOT_REGULAR",
                    relative,
                    "acceptance artifact must be a regular tracked file",
                )
            )
            continue
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            findings.append(AuditFinding("ARTIFACT_JSON_INVALID", relative, str(error)))
            continue
        _scan_json_value(value, relative, "", findings)
    return _result(
        "acceptance_artifacts",
        "forme-artifact-scan",
        TOOL_VERSION,
        _digest_paths(root, artifacts),
        findings,
    )


def cargo_audit_runner(root: Path) -> VulnerabilityScan:
    version_result = _run(["cargo-audit", "--version"], root)
    version = (
        (version_result.stdout or version_result.stderr).strip().splitlines()[0]
        if version_result.returncode == 0
        else "unavailable"
    )
    if version_result.returncode != 0:
        return VulnerabilityScan(
            tool="cargo-audit",
            version=version,
            completed=False,
            return_code=version_result.returncode,
            payload=None,
            error=(version_result.stderr or version_result.stdout).strip()
            or "cargo-audit is unavailable",
        )
    result = _run(["cargo-audit", "audit", "--json", "--file", "Cargo.lock"], root)
    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        return VulnerabilityScan(
            tool="cargo-audit",
            version=version,
            completed=False,
            return_code=result.returncode,
            payload=None,
            error=f"cargo-audit did not return typed JSON: {error}",
        )
    return VulnerabilityScan(
        tool="cargo-audit",
        version=version,
        completed=True,
        return_code=result.returncode,
        payload=payload,
        error=None,
    )


def _package_reachability(root: Path, name: str, version: str) -> tuple[bool | None, str | None]:
    result = _run(
        [
            "cargo",
            "tree",
            "--locked",
            "--target",
            RELEASE_TARGET,
            "-i",
            f"{name}@{version}",
        ],
        root,
    )
    if result.returncode != 0:
        return None, (result.stderr or result.stdout).strip() or "cargo tree failed"
    return bool(result.stdout.strip()), None


WARNING_KINDS = {
    "unsound": "advisory",
    "yanked": "yanked",
    "unmaintained": "informational",
    "notice": "informational",
}


def _warning_entries(
    warnings: object,
) -> tuple[list[tuple[str, str, str, str | None]], list[AuditFinding]]:
    entries: list[tuple[str, str, str, str | None]] = []
    findings: list[AuditFinding] = []
    if not isinstance(warnings, dict):
        return [], [
            AuditFinding(
                "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                "Cargo.lock",
                "cargo-audit warnings must be an object of warning-kind lists",
            )
        ]
    for kind, raw_entries in warnings.items():
        if kind not in WARNING_KINDS:
            findings.append(
                AuditFinding(
                    "VULNERABILITY_WARNING_KIND_UNKNOWN",
                    "Cargo.lock",
                    f"cargo-audit returned unknown warning kind {kind!r}",
                )
            )
            continue
        if not isinstance(raw_entries, list):
            findings.append(
                AuditFinding(
                    "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                    "Cargo.lock",
                    f"cargo-audit warning kind {kind!r} is not a list",
                )
            )
            continue
        for entry in raw_entries:
            if not isinstance(entry, dict):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        f"cargo-audit warning kind {kind!r} contains a non-object entry",
                    )
                )
                continue
            entry_kind = entry.get("kind")
            if entry_kind is not None and entry_kind != kind:
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        f"cargo-audit warning kind {kind!r} disagrees with entry kind {entry_kind!r}",
                    )
                )
                continue
            package = entry.get("package")
            if not isinstance(package, dict):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        f"cargo-audit {kind!r} warning lacks package identity",
                    )
                )
                continue
            name = package.get("name")
            version = package.get("version")
            if not isinstance(name, str) or not name or not isinstance(version, str) or not version:
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        f"cargo-audit {kind!r} warning has an empty package identity",
                    )
                )
                continue
            if WARNING_KINDS[kind] == "advisory":
                advisory = entry.get("advisory")
                advisory_id = advisory.get("id") if isinstance(advisory, dict) else None
                if not isinstance(advisory_id, str) or not advisory_id:
                    findings.append(
                        AuditFinding(
                            "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                            "Cargo.lock",
                            f"cargo-audit {kind!r} warning lacks advisory identity",
                        )
                    )
                    continue
            else:
                advisory_id = None
            entries.append((kind, name, version, advisory_id))
    return entries, findings


def _vulnerability_check(root: Path, runner: VulnerabilityRunner) -> ComplianceCheckResult:
    findings: list[AuditFinding] = []
    observations: list[AuditFinding] = []
    try:
        scan = runner(root)
    except Exception as error:  # scanner failure is blocking
        scan = VulnerabilityScan("vulnerability-scanner", "unavailable", False, -1, None, str(error))
    if not scan.completed or scan.payload is None:
        findings.append(
            AuditFinding(
                "VULNERABILITY_SCAN_FAILED",
                "Cargo.lock",
                scan.error or "vulnerability scanner did not complete",
            )
        )
    else:
        payload = scan.payload
        if not isinstance(payload, dict):
            findings.append(
                AuditFinding(
                    "VULNERABILITY_SCAN_PAYLOAD_INVALID",
                    "Cargo.lock",
                    "cargo-audit JSON root must be an object",
                )
            )
            payload = None
        if payload is None:
            return _result(
                "vulnerability_audit",
                scan.tool,
                scan.version,
                _digest_paths(root, ["Cargo.lock"]),
                findings,
                observations,
            )
        vulnerabilities = payload.get("vulnerabilities")
        count = 0
        vulnerability_list: list[object] = []
        if not isinstance(vulnerabilities, dict):
            findings.append(
                AuditFinding(
                    "VULNERABILITY_REPORT_INCOMPLETE",
                    "Cargo.lock",
                    "cargo-audit vulnerabilities must be an object with count and list",
                )
            )
        else:
            raw_count = vulnerabilities.get("count")
            raw_list = vulnerabilities.get("list", [])
            if not isinstance(raw_list, list):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        "cargo-audit vulnerabilities.list must be a list",
                    )
                )
            else:
                vulnerability_list = raw_list
            if (
                not isinstance(raw_count, int)
                or isinstance(raw_count, bool)
                or raw_count < 0
            ):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        "cargo-audit vulnerabilities.count must be a non-negative integer",
                    )
                )
            else:
                count = raw_count
        if count != len(vulnerability_list):
            findings.append(
                AuditFinding(
                    "VULNERABILITY_REPORT_INCOMPLETE",
                    "Cargo.lock",
                    f"cargo-audit count={count} but typed list={len(vulnerability_list)}",
                )
            )
        for vulnerability in vulnerability_list:
            if not isinstance(vulnerability, dict):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        "cargo-audit returned a non-object advisory entry",
                    )
                )
                continue
            package = vulnerability.get("package")
            advisory = vulnerability.get("advisory")
            if not isinstance(package, dict) or not isinstance(advisory, dict):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        "cargo-audit advisory lacks package or advisory identity",
                    )
                )
                continue
            name = package.get("name")
            version = package.get("version")
            advisory_id = advisory.get("id")
            if not all(
                isinstance(value, str) and value
                for value in (name, version, advisory_id)
            ):
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REPORT_INCOMPLETE",
                        "Cargo.lock",
                        "cargo-audit advisory identity is empty",
                    )
                )
                continue
            reachable, reachability_error = _package_reachability(root, name, version)
            if reachable is None:
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_REACHABILITY_FAILED",
                        "Cargo.lock",
                        f"{advisory_id} {name} {version}: "
                        + (reachability_error or "cargo tree failed"),
                    )
                )
            elif reachable:
                findings.append(
                    AuditFinding(
                        "RELEASE_TARGET_VULNERABILITY",
                        "Cargo.lock",
                        f"{advisory_id} affects reachable {name} {version} on {RELEASE_TARGET}",
                    )
                )
            else:
                observations.append(
                    AuditFinding(
                        "TARGET_UNREACHABLE_ADVISORY",
                        "Cargo.lock",
                        f"{advisory_id} affects locked {name} {version} only outside {RELEASE_TARGET}",
                    )
                )
        if "warnings" not in payload:
            warning_entries = []
            warning_findings = [
                AuditFinding(
                    "VULNERABILITY_WARNING_REPORT_INCOMPLETE",
                    "Cargo.lock",
                    "cargo-audit warnings field is missing",
                )
            ]
        else:
            warning_entries, warning_findings = _warning_entries(payload["warnings"])
        findings.extend(warning_findings)
        for kind, name, version, advisory_id in warning_entries:
            reachable, reachability_error = _package_reachability(root, name, version)
            if reachable is None:
                findings.append(
                    AuditFinding(
                        "VULNERABILITY_WARNING_REACHABILITY_FAILED",
                        "Cargo.lock",
                        f"{kind} warning for {name} {version}: "
                        + (reachability_error or "cargo tree failed"),
                    )
                )
                continue
            if kind == "unsound":
                if reachable:
                    findings.append(
                        AuditFinding(
                            "RELEASE_TARGET_ADVISORY_WARNING",
                            "Cargo.lock",
                            f"cargo-audit unsound warning {advisory_id} affects reachable {name} {version} on {RELEASE_TARGET}",
                        )
                    )
                else:
                    observations.append(
                        AuditFinding(
                            "TARGET_UNREACHABLE_ADVISORY_WARNING",
                            "Cargo.lock",
                            f"cargo-audit unsound warning {advisory_id} affects locked {name} {version} only outside {RELEASE_TARGET}",
                        )
                    )
            elif kind == "yanked":
                if reachable:
                    findings.append(
                        AuditFinding(
                            "RELEASE_TARGET_YANKED_PACKAGE",
                            "Cargo.lock",
                            f"cargo-audit yanked warning affects reachable {name} {version} on {RELEASE_TARGET}",
                        )
                    )
                else:
                    observations.append(
                        AuditFinding(
                            "TARGET_UNREACHABLE_YANKED_PACKAGE",
                            "Cargo.lock",
                            f"cargo-audit yanked warning affects locked {name} {version} only outside {RELEASE_TARGET}",
                        )
                    )
            elif reachable:
                observations.append(
                    AuditFinding(
                        "RELEASE_TARGET_INFORMATIONAL_WARNING",
                        "Cargo.lock",
                        f"cargo-audit {kind} warning affects reachable {name} {version} on {RELEASE_TARGET}",
                    )
                )
            else:
                observations.append(
                    AuditFinding(
                        "TARGET_UNREACHABLE_INFORMATIONAL_WARNING",
                        "Cargo.lock",
                        f"cargo-audit {kind} warning affects locked {name} {version} only outside {RELEASE_TARGET}",
                    )
                )
        if (
            scan.return_code != 0
            and not vulnerability_list
            and not warning_entries
            and not warning_findings
        ):
            findings.append(
                AuditFinding(
                    "VULNERABILITY_SCAN_FAILED",
                    "Cargo.lock",
                    f"scanner exited {scan.return_code} without typed advisory findings",
                )
            )
    return _result(
        "vulnerability_audit",
        scan.tool,
        scan.version,
        _digest_paths(root, ["Cargo.lock"]),
        findings,
        observations,
    )


def run_release_audit(
    root: Path,
    vulnerability_runner: VulnerabilityRunner = cargo_audit_runner,
) -> ReleaseAuditBody:
    root = root.resolve()
    release_tree, tracked, root_digest = _release_tree_check(root)
    dependencies, declarations = _dependency_check(root)
    checks = [
        release_tree,
        _license_notice_check(root, declarations),
        dependencies,
        _borrowing_check(root),
        _originality_check(root, root_digest),
        _artifact_check(root, tracked),
        _vulnerability_check(root, vulnerability_runner),
    ]
    blocking = any(check.blocking for check in checks)
    return ReleaseAuditBody(
        schema_version=SCHEMA_VERSION,
        tool_version=TOOL_VERSION,
        root_digest=root_digest,
        outcome="fail" if blocking else "pass",
        blocking=blocking,
        checks=checks,
    )


def _artifact_digest(body: ReleaseAuditBody) -> str:
    return _sha256(ARTIFACT_DOMAIN + _canonical(asdict(body)))


def write_artifact(root: Path, body: ReleaseAuditBody) -> ReleaseAuditReceipt:
    root.mkdir(parents=True, exist_ok=True)
    if not root.is_dir():
        raise ValueError("release audit artifact root is not a directory")
    digest = _artifact_digest(body)
    suffix = digest.removeprefix("sha256:")
    artifact = ReleaseAuditArtifact(
        schema_version=SCHEMA_VERSION,
        artifact_type=ARTIFACT_TYPE,
        digest=digest,
        body=body,
    )
    path = root / f"release-audit-{suffix}.json"
    entries = list(root.iterdir())
    unexpected = [entry for entry in entries if entry != path]
    if unexpected:
        raise ValueError("release audit artifact root contains an unexpected entry")
    encoded = json.dumps(asdict(artifact), ensure_ascii=True, sort_keys=True, indent=2).encode(
        "utf-8"
    ) + b"\n"
    if path.exists() and path.read_bytes() != encoded:
        raise ValueError("release audit digest path is bound to different content")
    if not path.exists():
        path.write_bytes(encoded)
    return verify_artifact_set(root, require_pass=False)


def _strict_keys(value: dict[str, object], expected: set[str], label: str) -> None:
    if set(value) != expected:
        raise ValueError(f"{label} fields are incomplete or unknown")


def _body_from_dict(value: object) -> ReleaseAuditBody:
    if not isinstance(value, dict):
        raise ValueError("release audit body is not an object")
    _strict_keys(
        value,
        {"schema_version", "tool_version", "root_digest", "outcome", "blocking", "checks"},
        "release audit body",
    )
    checks_value = value["checks"]
    if not isinstance(checks_value, list):
        raise ValueError("release audit checks are not an array")
    checks: list[ComplianceCheckResult] = []
    for raw_check in checks_value:
        if not isinstance(raw_check, dict):
            raise ValueError("release audit check is not an object")
        _strict_keys(
            raw_check,
            {
                "check_id",
                "outcome",
                "blocking",
                "tool",
                "tool_version",
                "input_digest",
                "findings",
                "observations",
            },
            "release audit check",
        )
        raw_findings = raw_check["findings"]
        if not isinstance(raw_findings, list):
            raise ValueError("release audit findings are not an array")
        findings: list[AuditFinding] = []
        for raw_finding in raw_findings:
            if not isinstance(raw_finding, dict):
                raise ValueError("release audit finding is not an object")
            _strict_keys(raw_finding, {"code", "path", "detail"}, "release audit finding")
            findings.append(AuditFinding(**raw_finding))
        raw_observations = raw_check["observations"]
        if not isinstance(raw_observations, list):
            raise ValueError("release audit observations are not an array")
        observations: list[AuditFinding] = []
        for raw_observation in raw_observations:
            if not isinstance(raw_observation, dict):
                raise ValueError("release audit observation is not an object")
            _strict_keys(raw_observation, {"code", "path", "detail"}, "release audit observation")
            observations.append(AuditFinding(**raw_observation))
        checks.append(
            ComplianceCheckResult(
                check_id=str(raw_check["check_id"]),
                outcome=str(raw_check["outcome"]),
                blocking=bool(raw_check["blocking"]),
                tool=str(raw_check["tool"]),
                tool_version=str(raw_check["tool_version"]),
                input_digest=str(raw_check["input_digest"]),
                findings=findings,
                observations=observations,
            )
        )
    return ReleaseAuditBody(
        schema_version=int(value["schema_version"]),
        tool_version=str(value["tool_version"]),
        root_digest=str(value["root_digest"]),
        outcome=str(value["outcome"]),
        blocking=bool(value["blocking"]),
        checks=checks,
    )


def verify_artifact_set(root: Path, require_pass: bool = True) -> ReleaseAuditReceipt:
    entries = list(root.iterdir()) if root.is_dir() else []
    if len(entries) != 1 or not entries[0].is_file() or entries[0].is_symlink():
        raise ValueError("release audit artifact root must contain exactly one regular file")
    path = entries[0]
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"release audit artifact is invalid JSON: {error}") from error
    if not isinstance(raw, dict):
        raise ValueError("release audit artifact is not an object")
    _strict_keys(raw, {"schema_version", "artifact_type", "digest", "body"}, "release audit artifact")
    body = _body_from_dict(raw["body"])
    digest = _artifact_digest(body)
    suffix = digest.removeprefix("sha256:")
    if (
        raw["schema_version"] != SCHEMA_VERSION
        or raw["artifact_type"] != ARTIFACT_TYPE
        or raw["digest"] != digest
        or path.name != f"release-audit-{suffix}.json"
    ):
        raise ValueError("release audit artifact type, digest, or filename does not match")
    if body.schema_version != SCHEMA_VERSION or body.tool_version != TOOL_VERSION:
        raise ValueError("release audit body schema is unsupported")
    if [check.check_id for check in body.checks] != EXPECTED_CHECK_IDS:
        raise ValueError("release audit check set is incomplete or reordered")
    for check in body.checks:
        blocked = bool(check.findings)
        if (
            check.blocking != blocked
            or check.outcome != ("fail" if blocked else "pass")
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", check.input_digest)
        ):
            raise ValueError(f"release audit check {check.check_id} is internally inconsistent")
    blocking = any(check.blocking for check in body.checks)
    if body.blocking != blocking or body.outcome != ("fail" if blocking else "pass"):
        raise ValueError("release audit aggregate outcome is internally inconsistent")
    if require_pass and blocking:
        raise ValueError("release audit artifact contains a blocking result")
    return ReleaseAuditReceipt(path, digest, f"artifact:{digest}")


def compare_artifact_sets(first: Path, second: Path) -> ReleaseAuditReceipt:
    first_receipt = verify_artifact_set(first)
    second_receipt = verify_artifact_set(second)
    if first_receipt.digest != second_receipt.digest:
        raise ValueError("release audit artifact does not match the current clean-tree audit")
    return second_receipt


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--verify", type=Path)
    parser.add_argument("--compare", type=Path, nargs=2, metavar=("CURRENT", "REPOSITORY"))
    args = parser.parse_args(argv)
    if args.compare is not None:
        try:
            receipt = compare_artifact_sets(args.compare[0].resolve(), args.compare[1].resolve())
        except (OSError, ValueError) as error:
            print(f"M3 RELEASE AUDIT COMPARISON: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] release-audit-current {receipt.digest} {receipt.content_ref}")
        print("M3 RELEASE AUDIT COMPARISON: PASS")
        return 0
    if args.verify is not None:
        try:
            receipt = verify_artifact_set(args.verify.resolve())
        except (OSError, ValueError) as error:
            print(f"M3 RELEASE AUDIT VERIFICATION: FAIL: {error}", file=sys.stderr)
            return 1
        print(f"[PASS] release-audit {receipt.digest} {receipt.content_ref}")
        print("M3 RELEASE AUDIT VERIFICATION: PASS")
        return 0
    root = args.root.resolve()
    output = (
        args.output.resolve()
        if args.output is not None
        else root / "docs/acceptance/m3-release-audit-artifacts"
    )
    body = run_release_audit(root)
    try:
        receipt = write_artifact(output, body)
    except (OSError, ValueError) as error:
        print(f"M3 RELEASE AUDIT: FAIL: artifact write failed: {error}", file=sys.stderr)
        return 1
    for check in body.checks:
        print(f"[{check.outcome.upper()}] {check.check_id} {check.input_digest}")
        for finding in check.findings:
            print(f"  {finding.code} {finding.path}: {finding.detail}")
        for observation in check.observations:
            print(f"  [OBSERVED] {observation.code} {observation.path}: {observation.detail}")
    print(f"release audit artifact {receipt.digest} {receipt.path}")
    print(f"M3 RELEASE AUDIT: {body.outcome.upper()}")
    return 1 if body.blocking else 0


if __name__ == "__main__":
    sys.exit(main())
