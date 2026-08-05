#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Compare two Fabric semantic models (a portal-created one vs a fabio-generated
one) to find gaps/improvements in `fabio semantic-model generate`.

The Fabric portal's "New semantic model" (pick tables -> Direct Lake) has no
REST API, so the portal model must be created by hand. This tool then fetches
BOTH models' definitions via the `fabio` CLI, normalizes them (it understands
the portal's TMDL folder AND fabio's model.bim), and prints a structural diff:
what the portal has that fabio lacks, and what fabio adds that the portal omits.

Typical workflow (run from the fabio repo, authenticated via `fabio auth login`
or `az login` — do NOT set a Fabric-only FABIO_ACCESS_TOKEN, schema reads need
a SQL-scoped token):

  1. In the Fabric portal, open your lakehouse -> "New semantic model" -> pick
     the tables -> Confirm. Copy the new model's id.
  2. Run:
       ./scripts/compare-semantic-models.py \
         --workspace <WS> --portal-id <PORTAL_MODEL_ID> \
         --lakehouse <LH_ID>            # fabio generates its model from this
     (or pass --fabio-id <ID> to compare an already-generated fabio model.)

The fabio model is generated fresh and deleted afterwards unless --keep-fabio.
"""

from __future__ import annotations

import argparse
import base64
import json
import shlex
import subprocess
import sys


# ── fabio CLI plumbing ────────────────────────────────────────────────────────

def fabio_cmd(args: list[str], bin_str: str) -> dict:
    """Run a fabio subcommand and return the parsed JSON envelope `data`."""
    cmd = shlex.split(bin_str) + args
    proc = subprocess.run(cmd, capture_output=True, text=True)
    if proc.returncode != 0:
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"fabio failed: {' '.join(args)}")
    # The JSON envelope is on stdout; the [timing] line (if any) is on stderr.
    for line in proc.stdout.splitlines():
        line = line.strip()
        if line.startswith("{"):
            return json.loads(line)["data"]
    raise SystemExit(f"no JSON from: {' '.join(args)}\n{proc.stdout}")


def get_definition_parts(ws: str, model_id: str, bin_str: str) -> dict[str, str]:
    data = fabio_cmd(
        ["semantic-model", "get-definition", "--workspace", ws, "--id", model_id],
        bin_str,
    )
    parts = {}
    for p in data["definition"]["parts"]:
        parts[p["path"]] = base64.b64decode(p["payload"]).decode("utf-8", "replace")
    return parts


# ── Normalized model shape ────────────────────────────────────────────────────
#
# {
#   "format": "tmdl" | "model.bim",
#   "settings": {compatibilityLevel, defaultMode, culture, dataSourceVersion,
#                annotations: [name...]},
#   "expressions": {name: body},
#   "tables": {name: {"columns": {name: {props...}}, "partition": {...},
#                     "measures": [name...]}},
#   "relationships": [ "from -> to" ],
# }

COLUMN_PROP_KEYS = (
    "dataType", "sourceColumn", "summarizeBy", "formatString", "dataCategory",
    "isHidden", "lineageTag", "sortByColumn",
)


def normalize_model_bim(text: str) -> dict:
    j = json.loads(text)
    model = j.get("model", {})
    tables = {}
    for t in model.get("tables", []):
        cols = {}
        for c in t.get("columns", []):
            props = {k: c[k] for k in COLUMN_PROP_KEYS if k in c}
            if c.get("annotations"):
                props["annotations"] = [a.get("name") for a in c["annotations"]]
            cols[c["name"]] = props
        parts = t.get("partitions", [{}])
        p0 = parts[0] if parts else {}
        src = p0.get("source", {})
        partition = {
            "mode": p0.get("mode"),
            "sourceType": src.get("type"),
            "entityName": src.get("entityName"),
            "schemaName": src.get("schemaName"),
            "expressionSource": src.get("expressionSource"),
        }
        tables[t["name"]] = {
            "columns": cols,
            "partition": partition,
            "measures": [m["name"] for m in t.get("measures", [])],
        }
    exprs = {e["name"]: e.get("expression", "") for e in model.get("expressions", [])}
    rels = [
        f'{r.get("fromTable")}.{r.get("fromColumn")} -> {r.get("toTable")}.{r.get("toColumn")}'
        for r in model.get("relationships", [])
    ]
    return {
        "format": "model.bim",
        "settings": {
            "compatibilityLevel": j.get("compatibilityLevel"),
            "defaultMode": model.get("defaultMode"),
            "culture": model.get("culture"),
            "dataSourceVersion": model.get("defaultPowerBIDataSourceVersion"),
            "annotations": [a.get("name") for a in model.get("annotations", [])],
        },
        "expressions": exprs,
        "tables": tables,
        "relationships": rels,
    }


def _indent(line: str) -> int:
    n = 0
    for ch in line:
        if ch == "\t":
            n += 1
        else:
            break
    return n


def _kv(line: str) -> tuple[str, str | None]:
    s = line.strip()
    if ": " in s:
        k, v = s.split(": ", 1)
        return k.strip(), v.strip()
    return s, None  # flag line (e.g. "isHidden")


def parse_tmdl_table(text: str) -> tuple[str, dict]:
    """Parse a `definition/tables/<T>.tmdl` file into (name, table-dict)."""
    lines = [ln for ln in text.splitlines() if ln.strip()]
    name = ""
    columns: dict[str, dict] = {}
    measures: list[str] = []
    partition: dict = {}
    cur_col = None
    in_partition = False
    for ln in lines:
        ind = _indent(ln)
        k, v = _kv(ln)
        if ind == 0 and k.startswith("table "):
            name = k[len("table "):].strip()
        elif ind == 1 and k.startswith("column "):
            cur_col = k[len("column "):].strip()
            columns[cur_col] = {}
            in_partition = False
        elif ind == 1 and k.startswith("measure "):
            measures.append(k[len("measure "):].split("=")[0].strip())
            cur_col = None
            in_partition = False
        elif ind == 1 and k.startswith("partition "):
            in_partition = True
            cur_col = None
            # "partition FactSales = entity"
            rhs = k.split("=", 1)
            partition["sourceType"] = rhs[1].strip() if len(rhs) > 1 else None
        elif in_partition and k in ("mode", "entityName", "schemaName",
                                     "expressionSource", "sourceLineageTag") and v is not None:
            partition[k] = v
        elif cur_col is not None and ind >= 2:
            if k.startswith("annotation "):
                columns[cur_col].setdefault("annotations", []).append(
                    k[len("annotation "):].split("=")[0].strip()
                )
            elif v is not None:
                columns[cur_col][k] = v
            else:
                columns[cur_col][k] = True  # flag (isHidden, isKey, ...)
    return name, {"columns": columns, "partition": partition, "measures": measures}


def normalize_tmdl(parts: dict[str, str]) -> dict:
    settings = {"compatibilityLevel": None, "defaultMode": None, "culture": None,
                "dataSourceVersion": None, "annotations": []}
    # model.tmdl
    for ln in parts.get("definition/model.tmdl", "").splitlines():
        k, v = _kv(ln)
        if k == "defaultMode" and v:
            settings["defaultMode"] = v
        elif k == "culture" and v:
            settings["culture"] = v
        elif k == "defaultPowerBIDataSourceVersion" and v:
            settings["dataSourceVersion"] = v
        elif k.startswith("annotation "):
            settings["annotations"].append(k[len("annotation "):].split("=")[0].strip())
    # database.tmdl
    for ln in parts.get("definition/database.tmdl", "").splitlines():
        k, v = _kv(ln)
        if k == "compatibilityLevel" and v:
            settings["compatibilityLevel"] = int(v) if v.isdigit() else v
    tables = {}
    for path, text in parts.items():
        if path.startswith("definition/tables/") and path.endswith(".tmdl"):
            nm, tbl = parse_tmdl_table(text)
            if nm:
                tables[nm] = tbl
    # expressions
    exprs = {}
    expr_text = parts.get("definition/expressions.tmdl", "")
    if "expression DatabaseQuery" in expr_text:
        exprs["DatabaseQuery"] = expr_text
    # relationships
    rels = []
    for block in parts.get("definition/relationships.tmdl", "").split("relationship")[1:]:
        frm = to = None
        for ln in block.splitlines():
            k, v = _kv(ln)
            if k == "fromColumn":
                frm = v
            elif k == "toColumn":
                to = v
        if frm and to:
            rels.append(f"{frm} -> {to}")
    return {"format": "tmdl", "settings": settings, "expressions": exprs,
            "tables": tables, "relationships": rels}


def normalize(parts: dict[str, str]) -> dict:
    if "model.bim" in parts:
        return normalize_model_bim(parts["model.bim"])
    return normalize_tmdl(parts)


# ── Diff / report ─────────────────────────────────────────────────────────────

def sql_database_arg(expr: str) -> str | None:
    """Extract the 2nd arg (catalog) of Sql.Database(server, catalog)."""
    i = expr.find("Sql.Database(")
    if i < 0:
        return None
    seg = expr[i + len("Sql.Database("):]
    seg = seg[: seg.find(")")]
    parts = [p.strip().strip('"') for p in seg.split(",")]
    return parts[1] if len(parts) > 1 else None


def looks_like_guid(s: str | None) -> bool:
    return bool(s) and len(s) == 36 and s.count("-") == 4


def ci_match(name: str, keys) -> str | None:
    low = name.lower()
    for k in keys:
        if k.lower() == low:
            return k
    return None


def compare(portal: dict, fabio: dict) -> dict:
    findings: list[str] = []
    portal_extra: list[str] = []

    # Model settings.
    for key in ("compatibilityLevel", "defaultMode", "dataSourceVersion", "culture"):
        pv, fv = portal["settings"].get(key), fabio["settings"].get(key)
        if pv != fv:
            findings.append(f"model.{key}: portal={pv!r} vs fabio={fv!r}")
    pann = set(portal["settings"].get("annotations") or [])
    fann = set(fabio["settings"].get("annotations") or [])
    if pann - fann:
        portal_extra.append(f"model annotations only in portal: {sorted(pann - fann)}")

    # Sql.Database catalog form (GUID vs display name).
    p_expr = next(iter(portal["expressions"].values()), "")
    f_expr = next(iter(fabio["expressions"].values()), "")
    p_cat, f_cat = sql_database_arg(p_expr), sql_database_arg(f_expr)
    if looks_like_guid(p_cat) and not looks_like_guid(f_cat):
        findings.append(
            f"Sql.Database catalog: portal uses the item GUID ({p_cat}), fabio uses "
            f"the display name ({f_cat!r}). The GUID is stable across renames."
        )

    # Tables (case-insensitive name match).
    p_only = [t for t in portal["tables"] if not ci_match(t, fabio["tables"])]
    f_only = [t for t in fabio["tables"] if not ci_match(t, portal["tables"])]
    if p_only:
        findings.append(f"tables only in portal: {p_only}")
    if f_only:
        portal_extra.append(f"tables only in fabio: {f_only}")

    # Per common table: columns, dataTypes, authoring attributes, partition.
    col_attr_gaps: dict[str, int] = {}
    fabio_extra_attrs: dict[str, int] = {}
    dtype_mismatch: list[str] = []
    for pt_name, pt in portal["tables"].items():
        ft_name = ci_match(pt_name, fabio["tables"])
        if not ft_name:
            continue
        ft = fabio["tables"][ft_name]
        for pc_name, pc in pt["columns"].items():
            fc_name = ci_match(pc_name, ft["columns"])
            if not fc_name:
                findings.append(f"column only in portal: {pt_name}.{pc_name}")
                continue
            fc = ft["columns"][fc_name]
            if pc.get("dataType") and fc.get("dataType") and pc["dataType"] != fc["dataType"]:
                dtype_mismatch.append(
                    f"{pt_name}.{pc_name}: portal {pc['dataType']} vs fabio {fc['dataType']}"
                )
            # Authoring attributes present on portal columns but not fabio's.
            for attr in ("summarizeBy", "formatString", "dataCategory", "lineageTag",
                         "sortByColumn", "annotations"):
                if attr in pc and attr not in fc:
                    col_attr_gaps[attr] = col_attr_gaps.get(attr, 0) + 1
                if attr in fc and attr not in pc:
                    fabio_extra_attrs[attr] = fabio_extra_attrs.get(attr, 0) + 1
        # Partition shape.
        pp, fp = pt.get("partition", {}), ft.get("partition", {})
        for k in ("mode", "sourceType", "schemaName", "expressionSource"):
            if pp.get(k) != fp.get(k):
                findings.append(
                    f"{pt_name} partition.{k}: portal={pp.get(k)!r} vs fabio={fp.get(k)!r}"
                )
        # entityName should be the physical (lowercase) table name in both.
        if pp.get("entityName") and fp.get("entityName") \
                and pp["entityName"].lower() != fp["entityName"].lower():
            findings.append(
                f"{pt_name} partition.entityName: portal={pp['entityName']!r} vs fabio={fp['entityName']!r}"
            )

    for attr, n in sorted(col_attr_gaps.items()):
        findings.append(f"{n} portal columns set '{attr}' that fabio omits")
    for attr, n in sorted(fabio_extra_attrs.items()):
        portal_extra.append(f"{n} fabio columns set '{attr}' that the portal omits")
    findings.extend(dtype_mismatch)

    # Relationships.
    if portal["relationships"] and not fabio["relationships"]:
        findings.append(
            f"portal has {len(portal['relationships'])} relationship(s), fabio has 0 "
            "(NOTE: the portal's pick-tables flow does NOT auto-create relationships — "
            "these were likely added manually; confirm before treating as a gap)"
        )

    return {"gaps_fabio_missing": findings, "fabio_extra_portal_omits": portal_extra}


def print_report(portal: dict, fabio: dict, result: dict) -> None:
    def hdr(s):
        print("\n" + s)
        print("-" * len(s))

    hdr("FORMAT")
    print(f"  portal: {portal['format']}   fabio: {fabio['format']}")
    hdr("MODEL SETTINGS")
    for k in ("compatibilityLevel", "defaultMode", "dataSourceVersion", "culture"):
        print(f"  {k:22} portal={portal['settings'].get(k)!r:<14} fabio={fabio['settings'].get(k)!r}")
    hdr("TABLES")
    print(f"  portal: {sorted(portal['tables'])}")
    print(f"  fabio : {sorted(fabio['tables'])}")

    hdr("GAPS — portal has / does that fabio LACKS")
    if result["gaps_fabio_missing"]:
        for g in result["gaps_fabio_missing"]:
            print(f"  [-] {g}")
    else:
        print("  (none)")

    hdr("EXTRA — fabio adds / does that the portal OMITS")
    if result["fabio_extra_portal_omits"]:
        for g in result["fabio_extra_portal_omits"]:
            print(f"  [+] {g}")
    else:
        print("  (none)")
    print()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--workspace", required=True)
    ap.add_argument("--portal-id", required=True, help="portal-created model id")
    ap.add_argument("--fabio-id", help="existing fabio model id (else one is generated)")
    ap.add_argument("--lakehouse", help="lakehouse id to generate the fabio model from")
    ap.add_argument("--warehouse", help="warehouse id to generate the fabio model from")
    ap.add_argument("--tables", help="comma-separated tables for generation")
    ap.add_argument("--schema", default="dbo")
    ap.add_argument("--keep-fabio", action="store_true",
                    help="do not delete a fabio model this tool generated")
    ap.add_argument("--fabio-bin", default="cargo run -q --bin fabio --",
                    help="how to invoke the fabio CLI")
    ap.add_argument("--json", action="store_true", help="emit machine-readable findings")
    a = ap.parse_args()

    generated_id = None
    fabio_id = a.fabio_id
    if not fabio_id:
        if not (a.lakehouse or a.warehouse):
            raise SystemExit("Provide --fabio-id, or --lakehouse/--warehouse to generate one.")
        gen_args = ["semantic-model", "generate", "--workspace", a.workspace,
                    "--name", "fabio_parity_probe", "--schema", a.schema]
        if a.lakehouse:
            gen_args += ["--lakehouse", a.lakehouse]
        if a.warehouse:
            gen_args += ["--warehouse", a.warehouse]
        if a.tables:
            gen_args += ["--tables", a.tables]
        sys.stderr.write("Generating fabio model...\n")
        gen = fabio_cmd(gen_args, a.fabio_bin)
        fabio_id = generated_id = gen["id"]
        sys.stderr.write(f"  fabio model id: {fabio_id}\n")

    try:
        portal = normalize(get_definition_parts(a.workspace, a.portal_id, a.fabio_bin))
        fabio = normalize(get_definition_parts(a.workspace, fabio_id, a.fabio_bin))
        result = compare(portal, fabio)
        if a.json:
            print(json.dumps({"portal": portal, "fabio": fabio, **result}, indent=2))
        else:
            print_report(portal, fabio, result)
    finally:
        if generated_id and not a.keep_fabio:
            sys.stderr.write(f"Deleting generated fabio model {generated_id}...\n")
            fabio_cmd(["semantic-model", "delete", "--workspace", a.workspace,
                       "--id", generated_id], a.fabio_bin)


if __name__ == "__main__":
    main()
