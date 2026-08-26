#!/usr/bin/env python3
"""Deep, conservative repository hygiene audit for ny-gemini-acp.

Diagnostic-only: this script never deletes or rewrites repository files.
It reports cleanup candidates with severity and confidence so findings can be
reviewed before any destructive change.

The audit is deliberately Rust/Cargo-aware. In particular, it uses Cargo
metadata to discover executable/library/test/bench/example roots, recursively
follows Rust `mod foo;` declarations, ignores comments when checking
architecture boundaries, and parses Cargo manifests with TOML instead of
mistaking arbitrary manifest keys for dependencies.

Checks:
  1. exact duplicate files by SHA-256
  2. highly similar Rust declarations
  3. Rust modules that are unreachable from discovered Cargo targets
  4. stale/legacy architecture names
  5. workspace members whose manifests or source roots are missing
  6. repeated direct dependencies not using workspace versioning
  7. forbidden runtime -> ACP/Gemini production references
  8. TODO/FIXME/HACK/comment debt
  9. large source files worth a manual responsibility review
 10. README path drift

Exit codes:
  0 = audit completed; no HIGH-severity findings
  1 = HIGH-severity findings exist
  2 = usage/environment error
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable


SKIP_DIRS = {
    ".git",
    "target",
    ".cargo",
    "node_modules",
    "vendor",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
}
TEXT_SUFFIXES = {
    ".rs",
    ".toml",
    ".md",
    ".txt",
    ".sh",
    ".yml",
    ".yaml",
    ".json",
    ".py",
}
RUST_SUFFIX = ".rs"

LEGACY_PATTERNS = (
    re.compile(r"gemini[-_]acp[-_](runtime|config|agent|encaps|tools)", re.IGNORECASE),
    re.compile(r"gemini[-_]acp[-_](core|client|provider)", re.IGNORECASE),
)
ARCHITECTURE_PATTERNS = {
    "runtime_acp": re.compile(
        r"agent[_-]client[_-]protocol|schema::v1|PromptRequest|InitializeRequest|NewSessionRequest|McpServer"
    ),
    "runtime_gemini": re.compile(
        r"\bGemini\b|\bgemini\b|google|sapisid|web2api|cookie_file|auth_user"
    ),
}
TODO_PATTERN = re.compile(r"\b(TODO|FIXME|HACK|XXX)\b", re.IGNORECASE)
COMMENTED_CODE_PATTERN = re.compile(
    r"^\s*//\s*(?:pub\s+|fn\s+|impl\s+|struct\s+|enum\s+|use\s+|mod\s+|let\s+|const\s+|match\s+|if\s+|for\s+|while\s+|return\b)",
    re.MULTILINE,
)
MOD_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.MULTILINE
)
PATH_MOD_PATTERN = re.compile(
    r"^\s*#\s*\[path\s*=\s*\"([^\"]+)\"\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;",
    re.MULTILINE,
)
DECL_PATTERN = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:fn|struct|enum|trait|type|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)",
    re.MULTILINE,
)


@dataclass
class Finding:
    category: str
    severity: str
    confidence: str
    subject: str
    evidence: str
    action: str


class Audit:
    def __init__(self, root: Path) -> None:
        self.root = root.resolve()
        self.findings: list[Finding] = []
        self.files: list[Path] = []
        self.text_cache: dict[Path, str] = {}
        self.cargo_roots: set[Path] = set()

    def add(
        self,
        category: str,
        severity: str,
        confidence: str,
        subject: str,
        evidence: str,
        action: str,
    ) -> None:
        self.findings.append(
            Finding(category, severity, confidence, subject, evidence, action)
        )

    def walk(self) -> None:
        files: list[Path] = []
        for base, dirs, names in os.walk(self.root):
            dirs[:] = sorted(
                d for d in dirs if d not in SKIP_DIRS and not d.startswith(".")
            )
            for name in names:
                path = Path(base) / name
                if path.suffix.lower() in TEXT_SUFFIXES:
                    files.append(path.resolve())
        self.files = sorted(files)

    def read_text(self, path: Path) -> str:
        cached = self.text_cache.get(path)
        if cached is not None:
            return cached
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            text = ""
        self.text_cache[path] = text
        return text

    def rel(self, path: Path) -> str:
        return path.relative_to(self.root).as_posix()

    def rust_files(self) -> Iterable[Path]:
        return (p for p in self.files if p.suffix == RUST_SUFFIX)

    @staticmethod
    def strip_comments_preserving_strings(text: str) -> str:
        """Remove Rust comments while preserving strings, chars, and lifetimes.

        This is deliberately not a Rust parser. The important invariant for the
        repository graph is that Rust lifetimes such as `'a` and `'_` must stay
        in normal code state; treating every apostrophe as a char literal can
        swallow the rest of a source file and produce false dead-module findings.
        """
        out: list[str] = []
        i = 0
        n = len(text)
        state = "code"
        while i < n:
            ch = text[i]
            nxt = text[i + 1] if i + 1 < n else ""
            if state == "code":
                if ch == "/" and nxt == "/":
                    state = "line_comment"
                    out.extend("  ")
                    i += 2
                    continue
                if ch == "/" and nxt == "*":
                    state = "block_comment"
                    out.extend("  ")
                    i += 2
                    continue
                if ch == '"':
                    state = "string"
                    out.append(ch)
                    i += 1
                    continue
                if ch == "'":
                    # Rust lifetimes (`'a`, `'_`, `'static`) are code, not char
                    # literals. Enter char state only when a closing quote is
                    # visible before the end of the line.
                    line_end = text.find("\n", i + 1)
                    if line_end == -1:
                        line_end = n
                    closing = text.find("'", i + 1, min(line_end, i + 4))
                    if closing != -1:
                        state = "char"
                    out.append(ch)
                    i += 1
                    continue
                out.append(ch)
                i += 1
                continue
            if state == "line_comment":
                if ch == "\n":
                    state = "code"
                    out.append("\n")
                else:
                    out.append(" ")
                i += 1
                continue
            if state == "block_comment":
                if ch == "*" and nxt == "/":
                    state = "code"
                    out.extend("  ")
                    i += 2
                else:
                    out.append("\n" if ch == "\n" else " ")
                    i += 1
                continue
            if state == "string":
                out.append(ch)
                if ch == "\\" and i + 1 < n:
                    out.append(text[i + 1])
                    i += 2
                    continue
                if ch == '"':
                    state = "code"
                i += 1
                continue
            if state == "char":
                out.append(ch)
                if ch == "\\" and i + 1 < n:
                    out.append(text[i + 1])
                    i += 2
                    continue
                if ch == "'":
                    state = "code"
                i += 1
        return "".join(out)

    def check_duplicate_files(self) -> None:
        by_hash: dict[str, list[Path]] = defaultdict(list)
        for path in self.files:
            try:
                data = path.read_bytes()
            except OSError:
                continue
            if len(data) < 64:
                continue
            digest = hashlib.sha256(data).hexdigest()
            by_hash[digest].append(path)

        for digest, paths in by_hash.items():
            if len(paths) < 2:
                continue
            rels = sorted(self.rel(p) for p in paths)
            severity = "HIGH" if any(p.suffix == RUST_SUFFIX for p in paths) else "MEDIUM"
            self.add(
                "duplicate-files",
                severity,
                "HIGH",
                ", ".join(rels),
                f"identical SHA-256: {digest[:16]}…; {len(paths)} files share exact bytes",
                "decide which copy is canonical and remove or consolidate only after usage is verified",
            )

    def check_duplicate_rust_declarations(self) -> None:
        fingerprints: dict[str, list[tuple[Path, str]]] = defaultdict(list)
        for path in self.rust_files():
            text = self.read_text(path)
            for match in DECL_PATTERN.finditer(text):
                name = match.group(1)
                start = max(0, match.start() - 220)
                end = min(len(text), match.end() + 900)
                snippet = self.strip_comments_preserving_strings(text[start:end])
                normalized = re.sub(r"\s+", " ", snippet).strip()
                if len(normalized) < 180:
                    continue
                digest = hashlib.sha256(normalized.encode("utf-8")).hexdigest()
                fingerprints[digest].append((path, name))

        for digest, entries in fingerprints.items():
            unique_paths = sorted({self.rel(p) for p, _ in entries})
            if len(unique_paths) < 2:
                continue
            self.add(
                "duplicate-rust-declarations",
                "MEDIUM",
                "MEDIUM",
                ", ".join(f"{self.rel(p)}::{n}" for p, n in entries),
                f"same normalized declaration neighborhood fingerprint {digest[:16]}… across {len(unique_paths)} files",
                "inspect whether the logic is truly duplicated; prefer a shared canonical helper when the semantics are the same",
            )

    def cargo_metadata_roots(self) -> set[Path]:
        try:
            proc = subprocess.run(
                ["cargo", "metadata", "--no-deps", "--format-version", "1"],
                cwd=self.root,
                text=True,
                capture_output=True,
                check=True,
            )
            payload = json.loads(proc.stdout)
        except (OSError, subprocess.CalledProcessError, json.JSONDecodeError):
            return set()

        roots: set[Path] = set()
        for package in payload.get("packages", []):
            for target in package.get("targets", []):
                src = target.get("src_path")
                if src:
                    roots.add(Path(src).resolve())
        return roots

    def rust_module_children(self, path: Path) -> set[Path]:
        text = self.strip_comments_preserving_strings(self.read_text(path))
        children: set[Path] = set()
        parent = path.parent
        for module in MOD_PATTERN.findall(text):
            candidates = (parent / f"{module}.rs", parent / module / "mod.rs")
            for candidate in candidates:
                if candidate.exists():
                    children.add(candidate.resolve())
                    break
        for relative in PATH_MOD_PATTERN.findall(text):
            candidate = (parent / relative).resolve()
            if candidate.exists():
                children.add(candidate)
        return children

    def check_unreferenced_rust_modules(self) -> None:
        roots = self.cargo_metadata_roots()
        self.cargo_roots = roots
        if not roots:
            return

        reachable: set[Path] = set()
        queue = sorted(roots)
        while queue:
            current = queue.pop(0)
            if current in reachable or not current.exists() or current.suffix != RUST_SUFFIX:
                continue
            reachable.add(current)
            for child in sorted(self.rust_module_children(current)):
                if child not in reachable:
                    queue.append(child)

        for path in self.rust_files():
            rel = self.rel(path)
            if rel.startswith("target/"):
                continue
            if path not in reachable:
                self.add(
                    "possibly-unreferenced-module",
                    "MEDIUM",
                    "HIGH",
                    rel,
                    "Rust file is not reachable from any Cargo target root using recursive `mod` declarations",
                    "verify generated/cfg/path-based inclusion; otherwise treat as a strong dead-file candidate",
                )

    def check_legacy_and_stale_names(self) -> None:
        for path in self.files:
            rel = self.rel(path)
            if rel == "scripts/audit-repository-hygiene.py":
                continue
            for line_no, line in enumerate(self.read_text(path).splitlines(), 1):
                if any(pattern.search(line) for pattern in LEGACY_PATTERNS):
                    production = rel.startswith("crates/") and "/tests/" not in rel
                    self.add(
                        "legacy-name",
                        "HIGH" if production else "LOW",
                        "HIGH" if production else "MEDIUM",
                        f"{rel}:{line_no}",
                        line.strip()[:240],
                        "rename/isolate historical references so the old architecture identity cannot leak back into production",
                    )

    def check_workspace_consistency(self) -> None:
        cargo = self.root / "Cargo.toml"
        if not cargo.exists():
            self.add(
                "workspace",
                "HIGH",
                "HIGH",
                "Cargo.toml",
                "workspace manifest missing",
                "restore or inspect repository root layout",
            )
            return
        try:
            data = tomllib.loads(self.read_text(cargo))
        except tomllib.TOMLDecodeError as exc:
            self.add(
                "workspace",
                "HIGH",
                "HIGH",
                "Cargo.toml",
                f"TOML parse error: {exc}",
                "repair the workspace manifest",
            )
            return
        members = data.get("workspace", {}).get("members", [])
        for member in members:
            crate = self.root / member
            manifest = crate / "Cargo.toml"
            src = crate / "src"
            if not manifest.exists() or not src.exists():
                self.add(
                    "workspace-member",
                    "HIGH",
                    "HIGH",
                    member,
                    f"manifest exists={manifest.exists()}, src exists={src.exists()}",
                    "remove stale workspace membership or restore the missing crate",
                )

    def check_dependency_repetition(self) -> None:
        manifests = sorted(self.root.glob("crates/*/Cargo.toml"))
        occurrences: dict[str, list[tuple[Path, bool]]] = defaultdict(list)
        sections = ("dependencies", "dev-dependencies", "build-dependencies")
        for manifest in manifests:
            try:
                data = tomllib.loads(self.read_text(manifest))
            except tomllib.TOMLDecodeError:
                continue
            for section in sections:
                for dep_name, spec in data.get(section, {}).items():
                    uses_workspace = isinstance(spec, dict) and spec.get("workspace") is True
                    occurrences[dep_name].append((manifest, uses_workspace))

        for dep, entries in sorted(occurrences.items()):
            if len(entries) < 3:
                continue
            non_workspace = [path for path, workspace in entries if not workspace]
            if len(non_workspace) < 3:
                continue
            paths = sorted({self.rel(path) for path in non_workspace})
            self.add(
                "dependency-repetition",
                "LOW",
                "MEDIUM",
                dep,
                f"direct dependency is repeated without workspace inheritance in {len(paths)} crates: {', '.join(paths)}",
                "consider moving its version/features to [workspace.dependencies] while keeping crate-local feature choices explicit when needed",
            )

    def check_runtime_boundaries(self) -> None:
        for path in self.rust_files():
            rel = self.rel(path)
            if not rel.startswith("crates/agent-runtime/"):
                continue
            if "/tests/" in rel:
                continue
            text = self.strip_comments_preserving_strings(self.read_text(path))
            for kind, pattern in ARCHITECTURE_PATTERNS.items():
                if pattern.search(text):
                    self.add(
                        "runtime-boundary",
                        "HIGH",
                        "HIGH",
                        f"{rel}::{kind}",
                        "provider-neutral runtime contains ACP/Gemini-specific production reference",
                        "move the protocol/provider-specific reference to the adapter/provider layer",
                    )

    def check_maintenance_debt(self) -> None:
        for path in self.files:
            rel = self.rel(path)
            if rel == "scripts/audit-repository-hygiene.py":
                continue
            text = self.read_text(path)
            todo_count = len(TODO_PATTERN.findall(text))
            commented_code = len(COMMENTED_CODE_PATTERN.findall(text))
            if todo_count:
                self.add(
                    "maintenance-debt",
                    "LOW",
                    "HIGH",
                    rel,
                    f"{todo_count} TODO/FIXME/HACK markers",
                    "review and either resolve, convert into tracked issues, or remove obsolete markers",
                )
            if commented_code >= 3:
                self.add(
                    "commented-code",
                    "LOW",
                    "MEDIUM",
                    rel,
                    f"{commented_code} lines look like commented-out Rust code",
                    "remove dead commented code or restore it intentionally through version control",
                )

    def check_large_files(self) -> None:
        for path in self.rust_files():
            text = self.read_text(path)
            lines = text.count("\n") + (1 if text else 0)
            if lines < 900:
                continue
            self.add(
                "large-source-file",
                "LOW",
                "HIGH",
                self.rel(path),
                f"{lines} lines of Rust source",
                "review whether responsibilities can be split without creating needless abstraction or cross-module coupling",
            )

    def check_readme_paths(self) -> None:
        readme = self.root / "README.md"
        if not readme.exists():
            return
        for line_no, line in enumerate(self.read_text(readme).splitlines(), 1):
            for match in re.finditer(r"(?<![A-Za-z0-9_./-])((?:crates|scripts|docs)/[A-Za-z0-9_./-]+)", line):
                candidate = match.group(1).rstrip(".,:;`)")
                if candidate and not (self.root / candidate).exists():
                    self.add(
                        "readme-drift",
                        "LOW",
                        "HIGH",
                        f"README.md:{line_no}",
                        f"referenced path does not exist: {candidate}",
                        "update the documentation path or restore the referenced artifact",
                    )

    def run(self) -> None:
        self.walk()
        self.check_duplicate_files()
        self.check_duplicate_rust_declarations()
        self.check_unreferenced_rust_modules()
        self.check_legacy_and_stale_names()
        self.check_workspace_consistency()
        self.check_dependency_repetition()
        self.check_runtime_boundaries()
        self.check_maintenance_debt()
        self.check_large_files()
        self.check_readme_paths()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--format", choices=("text", "json"), default="text")
    parser.add_argument("--output", type=Path)
    parser.add_argument("root", nargs="?", default=".")
    args = parser.parse_args()

    root = Path(args.root).resolve()
    if not root.exists():
        print(f"error: repository root does not exist: {root}", file=sys.stderr)
        return 2

    audit = Audit(root)
    audit.run()

    counts = Counter(f.severity for f in audit.findings)
    payload = {
        "root": str(root),
        "files_scanned": len(audit.files),
        "cargo_roots": len(audit.cargo_roots),
        "findings": [asdict(f) for f in audit.findings],
    }

    if args.format == "json":
        rendered = json.dumps(payload, indent=2, ensure_ascii=False) + "\n"
    else:
        lines = [
            "ny-gemini-acp — deep repository hygiene audit",
            f"root: {root}",
            f"files scanned: {len(audit.files)}",
            f"cargo roots: {len(audit.cargo_roots)}",
            f"findings: {len(audit.findings)} (HIGH={counts.get('HIGH', 0)}, MEDIUM={counts.get('MEDIUM', 0)}, LOW={counts.get('LOW', 0)})",
        ]
        grouped: dict[str, list[Finding]] = defaultdict(list)
        for finding in audit.findings:
            grouped[finding.category].append(finding)
        for category in sorted(grouped):
            lines.extend(["", f"## {category}"])
            for finding in grouped[category]:
                lines.extend(
                    [
                        f"[{finding.severity}/{finding.confidence}] {finding.subject}",
                        f"  evidence: {finding.evidence}",
                        f"  action:   {finding.action}",
                    ]
                )
        rendered = "\n".join(lines) + "\n"

    if args.output:
        args.output.write_text(rendered, encoding="utf-8")
    else:
        print(rendered, end="")

    return 1 if counts.get("HIGH", 0) else 0


if __name__ == "__main__":
    raise SystemExit(main())
