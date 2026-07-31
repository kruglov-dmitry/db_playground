#!/usr/bin/env python3
"""Build a self-contained HTML comparison report from explain-lookups results."""

from __future__ import annotations

import argparse
import html
import json
from collections import defaultdict
from pathlib import Path


COLORS = ("#4f46e5", "#f97316", "#059669", "#dc2626")


def number(value: int | float) -> str:
    return f"{value:,}"


def bytes_human(value: int) -> str:
    units = ("B", "KiB", "MiB", "GiB", "TiB")
    size = float(value)
    for unit in units:
        if size < 1024 or unit == units[-1]:
            return f"{size:.2f} {unit}"
        size /= 1024
    raise AssertionError("unreachable")


def load_results(results_dir: Path) -> list[dict]:
    rows = []
    for metadata_path in sorted(results_dir.glob("*.meta.json")):
        stem = metadata_path.name.removesuffix(".meta.json")
        plan_path = results_dir / f"{stem}.plan.json"
        if not plan_path.exists():
            print(f"Skipping {metadata_path.name}: matching plan is missing")
            continue
        metadata = json.loads(metadata_path.read_text())
        explain = json.loads(plan_path.read_text())[0]
        plan = explain["Plan"]
        rows.append(
            {
                "label": metadata["label"],
                "lookups": int(metadata["lookup_count"]),
                "captured_at": metadata["captured_at_utc"],
                "version": metadata["server_version"].split(" on ")[0],
                "node": plan["Node Type"],
                "execution_ms": float(explain["Execution Time"]),
                "planning_ms": float(explain["Planning Time"]),
                "hits": int(plan.get("Shared Hit Blocks", 0)),
                "reads": int(plan.get("Shared Read Blocks", 0)),
                "table_bytes": int(metadata["table_size_bytes"]),
                "index_bytes": int(metadata["index_size_bytes"]),
                "database_bytes": int(metadata["database_size_bytes"]),
                "rows_estimate": int(metadata["row_count_estimate"]),
            }
        )
    return sorted(rows, key=lambda row: (row["lookups"], row["label"]))


def chart(rows: list[dict], key: str, title: str, suffix: str, formatter) -> str:
    by_count: dict[int, list[dict]] = defaultdict(list)
    for row in rows:
        by_count[row["lookups"]].append(row)
    maximum = max(row[key] for row in rows) or 1
    width, height = 840, 245
    left, top, bottom = 65, 28, 45
    plot_width, plot_height = width - left - 28, height - top - bottom
    groups = sorted(by_count.items())
    group_width = plot_width / len(groups)
    labels = sorted({row["label"] for row in rows})
    color = {label: COLORS[index % len(COLORS)] for index, label in enumerate(labels)}
    marks = []
    for group_index, (count, group) in enumerate(groups):
        group = sorted(group, key=lambda row: row["label"])
        bar_width = min(52, (group_width * 0.72) / len(group))
        start = left + group_index * group_width + (group_width - bar_width * len(group)) / 2
        for row_index, row in enumerate(group):
            value = row[key]
            bar_height = plot_height * value / maximum
            x = start + row_index * bar_width
            y = top + plot_height - bar_height
            marks.append(
                f'<rect x="{x:.1f}" y="{y:.1f}" width="{bar_width - 5:.1f}" height="{bar_height:.1f}" fill="{color[row["label"]]}" rx="3">'
                f'<title>{html.escape(row["label"])} · {count} lookups: {formatter(value)}</title></rect>'
            )
            marks.append(
                f'<text x="{x + (bar_width - 5) / 2:.1f}" y="{max(16, y - 7):.1f}" text-anchor="middle" class="value">{formatter(value)}</text>'
            )
        center = left + group_index * group_width + group_width / 2
        marks.append(f'<text x="{center:.1f}" y="{height - 17}" text-anchor="middle" class="axis">{number(count)} lookups</text>')
    legend = "".join(
        f'<span><i style="background:{color[label]}"></i>{html.escape(label)}</span>' for label in labels
    )
    return f'''<section class="chart-card">
  <div class="chart-heading"><h2>{title}</h2><div class="legend">{legend}</div></div>
  <svg viewBox="0 0 {width} {height}" role="img" aria-label="{html.escape(title)}">
    <line x1="{left}" y1="{top + plot_height}" x2="{width - 28}" y2="{top + plot_height}" class="axis-line"/>
    <text x="8" y="{top + 8}" class="axis">{formatter(maximum)}{suffix}</text>
    <text x="8" y="{top + plot_height}" class="axis">0</text>
    {''.join(marks)}
  </svg>
</section>'''


def build_report(rows: list[dict]) -> str:
    latest_by_label: dict[str, dict] = {}
    for row in rows:
        latest_by_label[row["label"]] = row
    table_rows = "".join(
        "<tr>"
        f"<td>{html.escape(row['label'])}</td><td>{number(row['lookups'])}</td>"
        f"<td>{html.escape(row['node'])}</td><td>{row['planning_ms']:.3f} ms</td>"
        f"<td>{row['execution_ms']:.3f} ms</td><td>{number(row['hits'])}</td><td>{number(row['reads'])}</td>"
        "</tr>"
        for row in rows
    )
    size_rows = "".join(
        "<tr>"
        f"<td>{html.escape(label)}</td><td>{bytes_human(row['table_bytes'])}</td>"
        f"<td>{bytes_human(row['index_bytes'])}</td><td>{bytes_human(row['database_bytes'])}</td>"
        f"<td>{number(row['rows_estimate'])}</td>"
        "</tr>"
        for label, row in sorted(latest_by_label.items())
    )
    return f'''<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>PostgreSQL UUID benchmark comparison</title>
<style>
  :root {{ color-scheme: light dark; --ink:#172033; --muted:#5d6a7e; --line:#d7dce5; --surface:#f7f8fb; }}
  body {{ margin:0; padding:32px; font:15px/1.45 system-ui,sans-serif; color:var(--ink); max-width:1100px; margin-inline:auto; }}
  h1 {{ margin:0 0 4px; font-size:28px; }} h2 {{ margin:0; font-size:16px; }} .sub {{ color:var(--muted); margin:0 0 28px; }}
  .grid {{ display:grid; grid-template-columns:repeat(2,minmax(0,1fr)); gap:20px; }}
  .chart-card, .table-card {{ border:1px solid var(--line); border-radius:10px; padding:16px; background:var(--surface); }}
  .chart-heading {{ display:flex; align-items:center; justify-content:space-between; gap:10px; }} .legend {{ display:flex; gap:12px; flex-wrap:wrap; font-size:13px; }}
  .legend span {{ display:inline-flex; align-items:center; gap:5px; }} .legend i {{ width:10px; height:10px; border-radius:50%; }}
  svg {{ width:100%; display:block; margin-top:12px; }} .axis {{ fill:var(--muted); font-size:12px; }} .value {{ fill:var(--ink); font-size:11px; }} .axis-line {{ stroke:var(--line); }}
  .table-card {{ margin-top:20px; overflow-x:auto; }} table {{ border-collapse:collapse; width:100%; }} th,td {{ text-align:right; padding:9px 10px; border-bottom:1px solid var(--line); white-space:nowrap; }} th:first-child,td:first-child,th:nth-child(3),td:nth-child(3) {{ text-align:left; }} th {{ color:var(--muted); font-weight:600; }}
  @media (max-width:700px) {{ body {{ padding:18px; }} .grid {{ grid-template-columns:1fr; }} }}
</style></head><body>
<h1>PostgreSQL UUID benchmark</h1>
<p class="sub">Execution-plan comparison for UUIDv4 on PostgreSQL 17 and UUIDv7 on PostgreSQL 18.</p>
<div class="grid">
{chart(rows, 'execution_ms', 'Execution time', ' ms', lambda value: f'{value:.3f} ms')}
{chart(rows, 'reads', 'Shared buffer reads', '', lambda value: number(value))}
</div>
<section class="table-card"><h2>Lookup plans</h2><table><thead><tr><th>Dataset</th><th>Lookups</th><th>Scan node</th><th>Planning</th><th>Execution</th><th>Buffer hits</th><th>Buffer reads</th></tr></thead><tbody>{table_rows}</tbody></table></section>
<section class="table-card"><h2>Storage snapshot</h2><table><thead><tr><th>Dataset</th><th>Table</th><th>Primary-key index</th><th>Database</th><th>Estimated rows</th></tr></thead><tbody>{size_rows}</tbody></table></section>
</body></html>'''


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=Path("results"), help="Directory created by explain-lookups.sh")
    parser.add_argument("--output", type=Path, default=Path("results/comparison.html"), help="HTML report path")
    args = parser.parse_args()
    rows = load_results(args.input)
    if not rows:
        raise SystemExit(f"No matching *.plan.json and *.meta.json pairs found in {args.input}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(build_report(rows))
    print(f"Wrote {args.output} using {len(rows)} result pairs")


if __name__ == "__main__":
    main()
