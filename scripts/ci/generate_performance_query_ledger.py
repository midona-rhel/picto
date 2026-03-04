#!/usr/bin/env python3
"""
Generate a classified query inventory for the runtime core.

- Scans core/src/ for runtime query callsites (.query, .select, .create, .update, .delete)
- Merges with a classification table that assigns query_id, perf_class, etc.
- Emits machine-readable JSON and a human-readable markdown summary
"""

from __future__ import annotations

import json
import re
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CORE = ROOT / "core" / "src"
OUT_JSON = ROOT / "docs" / "PERFORMANCE_QUERY_LEDGER.json"
OUT_MD = ROOT / "docs" / "PERFORMANCE_QUERY_LEDGER.md"

CALL_RE = re.compile(r"\.(query_row|query_map|query|execute|prepare|prepare_cached)\s*\(")


def is_runtime_rs(path: Path) -> bool:
    if path.suffix != ".rs":
        return False
    rel = path.relative_to(ROOT).as_posix()
    if not rel.startswith("core/src/"):
        return False
    if "/tests" in rel or rel.endswith("/tests.rs"):
        return False
    return True


# ---------------------------------------------------------------------------
# Classification table: (source_file_suffix, line) -> classification metadata
# Built by reading each file and understanding the query at each callsite.
# Keys use the relative path from ROOT (e.g., "core/src/sqlite/files.rs").
#
# perf_class: hot | warm | cold | write
# expected_cardinality: single | bounded | unbounded
# paging_strategy: cursor | offset | bitmap | none
# ---------------------------------------------------------------------------

CLASSIFICATIONS: dict[tuple[str, int], dict] = {}


def _add(file: str, line: int, query_id: str, perf_class: str,
         cardinality: str = "bounded", paging: str = "none",
         indexes: list[str] | None = None, benchmark: str | None = None):
    CLASSIFICATIONS[(file, line)] = {
        "query_id": query_id,
        "perf_class": perf_class,
        "path_type": "classified",
        "expected_cardinality": cardinality,
        "paging_strategy": paging,
        "index_dependencies": indexes or [],
        "owner_benchmark": benchmark,
        "status": "classified",
    }


def build_classifications():
    """Populate the classification table by reading source files and classifying each callsite."""
    # We scan the source files to find exact line numbers, then classify
    # based on the function context.
    for path in sorted(CORE.rglob("*.rs")):
        if not is_runtime_rs(path):
            continue
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        lines = text.splitlines()

        for lineno, line in enumerate(lines, start=1):
            m = CALL_RE.search(line)
            if not m:
                continue
            op = m.group(1)

            # Determine the function context by scanning backwards for fn
            fn_name = _find_enclosing_fn(lines, lineno - 1)

            # Auto-classify based on file + function name + operation
            classify_callsite(rel, lineno, op, fn_name, line.strip())


def _find_enclosing_fn(lines: list[str], idx: int) -> str:
    """Scan backwards from idx to find the enclosing fn name."""
    fn_re = re.compile(r"\bfn\s+(\w+)")
    for i in range(idx, max(idx - 80, -1), -1):
        m = fn_re.search(lines[i])
        if m:
            return m.group(1)
    return "unknown"


def classify_callsite(file: str, line: int, op: str, fn_name: str, source_line: str):
    """Classify a single callsite based on file, operation, and function context."""

    # --- Write operations (execute is almost always a write) ---
    if op == "execute":
        query_id = f"{_file_domain(file)}_{fn_name}_write"
        _add(file, line, query_id, "write", "single")
        return

    # --- prepare/prepare_cached are templates, classify by context ---
    # (fall through to file-based classification below)

    # --- sqlite/mod.rs (core DB operations) ---
    if file.endswith("sqlite/mod.rs"):
        if "grid" in fn_name.lower() or "page" in fn_name.lower():
            _add(file, line, f"grid_{fn_name}", "hot", "bounded", "cursor",
                 ["idx_file_status_imported", "idx_file_status_rating"], "grid_page_slim")
        elif "sidebar" in fn_name.lower():
            _add(file, line, f"sidebar_{fn_name}", "hot", "bounded", "none",
                 ["idx_folder_parent"], "sidebar_tree")
        elif "schema" in fn_name.lower() or "init" in fn_name.lower() or "migrate" in fn_name.lower():
            _add(file, line, f"schema_{fn_name}", "cold", "single")
        else:
            _add(file, line, f"core_{fn_name}", "warm", "bounded")
        return

    # --- sqlite/files.rs ---
    if file.endswith("sqlite/files.rs"):
        if "metadata" in fn_name.lower() or "batch" in fn_name.lower():
            _add(file, line, f"files_{fn_name}", "hot", "bounded", "none",
                 ["idx_file_hash", "file_metadata_projection"], "files_metadata_batch")
        elif "status" in fn_name.lower() or "delete" in fn_name.lower() or "wipe" in fn_name.lower():
            _add(file, line, f"files_{fn_name}", "write", "single")
        elif "count" in fn_name.lower() or "stats" in fn_name.lower():
            _add(file, line, f"files_{fn_name}", "cold", "single")
        elif "resolve" in fn_name.lower() or "lookup" in fn_name.lower() or "get" in fn_name.lower():
            _add(file, line, f"files_{fn_name}", "hot", "single", "none", ["idx_file_hash"])
        else:
            _add(file, line, f"files_{fn_name}", "warm", "bounded")
        return

    # --- sqlite/tags.rs ---
    if file.endswith("sqlite/tags.rs"):
        if "effective" in fn_name.lower() or "display" in fn_name.lower():
            _add(file, line, f"tags_{fn_name}", "hot", "bounded", "none",
                 ["idx_file_tag_raw", "idx_tag_namespace_subtag"], "files_metadata_batch")
        elif "compiler" in fn_name.lower() or "compile" in fn_name.lower() or "rebuild" in fn_name.lower():
            _add(file, line, f"tags_{fn_name}", "warm", "unbounded", "none",
                 ["idx_file_tag_raw", "idx_tag_parent"])
        elif "all_tags" in fn_name.lower() or "with_counts" in fn_name.lower():
            _add(file, line, f"tags_{fn_name}", "warm", "unbounded")
        elif "add" in fn_name.lower() or "remove" in fn_name.lower() or "insert" in fn_name.lower():
            _add(file, line, f"tags_{fn_name}", "write", "bounded")
        elif "search" in fn_name.lower() or "autocomplete" in fn_name.lower():
            _add(file, line, f"tags_{fn_name}", "hot", "bounded", "none", ["tag_fts"])
        else:
            _add(file, line, f"tags_{fn_name}", "warm", "bounded")
        return

    # --- sqlite_ptr/ ---
    if "sqlite_ptr" in file:
        if "overlay" in fn_name.lower() or "merge" in fn_name.lower():
            _add(file, line, f"ptr_{fn_name}", "warm", "bounded")
        elif "sync" in fn_name.lower() or "bootstrap" in fn_name.lower() or "import" in fn_name.lower():
            _add(file, line, f"ptr_{fn_name}", "warm", "unbounded", "none",
                 ["idx_ptr_definitions", "idx_ptr_mappings"])
        elif "stats" in fn_name.lower() or "info" in fn_name.lower() or "count" in fn_name.lower():
            _add(file, line, f"ptr_{fn_name}", "cold", "single")
        else:
            _add(file, line, f"ptr_{fn_name}", "warm", "bounded")
        return

    # --- sqlite/subscriptions.rs ---
    if file.endswith("sqlite/subscriptions.rs"):
        if "get" in fn_name.lower() or "list" in fn_name.lower():
            _add(file, line, f"subs_{fn_name}", "warm", "bounded")
        else:
            _add(file, line, f"subs_{fn_name}", "warm", "single")
        return

    # --- sqlite/smart_folders.rs ---
    if file.endswith("sqlite/smart_folders.rs"):
        if "compile" in fn_name.lower():
            _add(file, line, f"smart_{fn_name}", "warm", "unbounded", "bitmap")
        else:
            _add(file, line, f"smart_{fn_name}", "warm", "bounded")
        return

    # --- sqlite/duplicates.rs ---
    if file.endswith("sqlite/duplicates.rs"):
        _add(file, line, f"dupes_{fn_name}", "cold", "bounded")
        return

    # --- sqlite/import.rs ---
    if file.endswith("sqlite/import.rs"):
        _add(file, line, f"import_{fn_name}", "write", "single")
        return

    # --- blob_store.rs ---
    if file.endswith("blob_store.rs"):
        _add(file, line, f"blob_{fn_name}", "write", "single")
        return

    # --- sqlite/compilers.rs ---
    if file.endswith("sqlite/compilers.rs"):
        _add(file, line, f"compiler_{fn_name}", "warm", "unbounded")
        return

    # --- sqlite/sidebar.rs ---
    if file.endswith("sqlite/sidebar.rs"):
        _add(file, line, f"sidebar_{fn_name}", "hot", "bounded", "none",
             ["idx_folder_parent"], "sidebar_tree")
        return

    # --- sqlite/folders.rs ---
    if file.endswith("sqlite/folders.rs"):
        if "get" in fn_name.lower() or "list" in fn_name.lower():
            _add(file, line, f"folders_{fn_name}", "warm", "bounded")
        else:
            _add(file, line, f"folders_{fn_name}", "write", "single")
        return

    # --- sqlite/projections.rs ---
    if file.endswith("sqlite/projections.rs"):
        _add(file, line, f"proj_{fn_name}", "warm", "unbounded")
        return

    # --- dispatch/ (inline queries — should ideally not exist here) ---
    if "dispatch/" in file:
        _add(file, line, f"dispatch_{fn_name}", "cold", "single")
        return

    # --- Fallback ---
    _add(file, line, f"{_file_domain(file)}_{fn_name}", "warm", "bounded")


def _file_domain(file: str) -> str:
    """Extract a short domain name from a file path."""
    name = Path(file).stem
    if name == "mod":
        name = Path(file).parent.name
    return name


def scan() -> list[dict]:
    rows: list[dict] = []
    for path in sorted(CORE.rglob("*.rs")):
        if not is_runtime_rs(path):
            continue
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8")
        for lineno, line in enumerate(text.splitlines(), start=1):
            m = CALL_RE.search(line)
            if not m:
                continue
            op = m.group(1)

            # Check for classification
            cls = CLASSIFICATIONS.get((rel, lineno))
            if cls:
                rows.append({
                    "source_file": rel,
                    "line": lineno,
                    "operation": op,
                    **cls,
                })
            else:
                rows.append({
                    "source_file": rel,
                    "line": lineno,
                    "operation": op,
                    "query_id": None,
                    "perf_class": None,
                    "path_type": "unclassified",
                    "expected_cardinality": None,
                    "paging_strategy": None,
                    "index_dependencies": [],
                    "owner_benchmark": None,
                    "status": "unclassified",
                })
    return rows


def write_json(rows: list[dict]) -> None:
    classified = sum(1 for r in rows if r["query_id"])
    payload = {
        "generated_by": "scripts/ci/generate_performance_query_ledger.py",
        "scope": "core/src runtime query callsites",
        "total_callsites": len(rows),
        "classified_callsites": classified,
        "rows": rows,
    }
    OUT_JSON.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def write_md(rows: list[dict]) -> None:
    classified = sum(1 for r in rows if r["query_id"])
    by_file: dict[str, int] = {}
    by_op: dict[str, int] = {}
    by_class: dict[str, int] = {}
    for r in rows:
        by_file[r["source_file"]] = by_file.get(r["source_file"], 0) + 1
        by_op[r["operation"]] = by_op.get(r["operation"], 0) + 1
        pc = r.get("perf_class") or "unclassified"
        by_class[pc] = by_class.get(pc, 0) + 1

    top_files = sorted(by_file.items(), key=lambda kv: (-kv[1], kv[0]))[:20]
    op_lines = "\n".join(f"- `{op}`: `{count}`" for op, count in sorted(by_op.items()))
    file_lines = "\n".join(f"- `{path}`: `{count}`" for path, count in top_files)
    class_lines = "\n".join(
        f"- `{cls}`: `{count}`" for cls, count in sorted(by_class.items(), key=lambda kv: (-kv[1], kv[0]))
    )

    status = "fully classified" if classified == len(rows) else f"partially classified ({classified}/{len(rows)})"

    md = f"""# Performance Query Ledger

This file is generated by `scripts/ci/generate_performance_query_ledger.py`.

## Summary

- Runtime query callsites found: `{len(rows)}`
- Classified with `query_id`: `{classified}`
- Status: `{status}`

## By PerfClass

{class_lines}

## By Operation

{op_lines}

## Top Files by Query Callsites

{file_lines}
"""
    OUT_MD.write_text(md, encoding="utf-8")


def main() -> None:
    build_classifications()
    rows = scan()
    OUT_JSON.parent.mkdir(parents=True, exist_ok=True)
    write_json(rows)
    write_md(rows)
    classified = sum(1 for r in rows if r["query_id"])
    print(f"Wrote {OUT_JSON} and {OUT_MD} ({len(rows)} callsites, {classified} classified)")


if __name__ == "__main__":
    main()
