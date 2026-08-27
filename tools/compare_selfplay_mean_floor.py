#!/usr/bin/env python3
"""Compare two teacher-free policies solely by paired mean final floor.

Boss reaches, wins, and the floor distribution are emitted as diagnostics. They
never participate in checkpoint selection.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any


def load_results(path: Path) -> dict[int, dict[str, Any]]:
    results: dict[int, dict[str, Any]] = {}
    with path.open() as source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            row = json.loads(line)
            seed = int(row["seed"])
            if seed in results:
                raise ValueError(f"{path}:{line_number}: duplicate seed {seed}")
            results[seed] = row
    if not results:
        raise ValueError(f"{path}: no results")
    return results


def sample_variance(values: list[float], mean: float) -> float:
    if len(values) < 2:
        return 0.0
    return sum((value - mean) ** 2 for value in values) / (len(values) - 1)


def compare(baseline_path: Path, candidate_path: Path) -> dict[str, Any]:
    baseline = load_results(baseline_path)
    candidate = load_results(candidate_path)
    if baseline.keys() != candidate.keys():
        missing = sorted(baseline.keys() - candidate.keys())[:10]
        extra = sorted(candidate.keys() - baseline.keys())[:10]
        raise ValueError(
            "paired comparison requires identical seeds; "
            f"missing={missing}, extra={extra}"
        )

    seeds = sorted(baseline)
    baseline_floors = [float(baseline[seed]["max_floor"]) for seed in seeds]
    candidate_floors = [float(candidate[seed]["max_floor"]) for seed in seeds]
    deltas = [after - before for before, after in zip(baseline_floors, candidate_floors)]
    count = len(seeds)
    baseline_mean = sum(baseline_floors) / count
    candidate_mean = sum(candidate_floors) / count
    mean_delta = sum(deltas) / count
    standard_error = math.sqrt(sample_variance(deltas, mean_delta) / count)
    ci_radius = 1.959963984540054 * standard_error

    def diagnostics(rows: dict[int, dict[str, Any]]) -> dict[str, Any]:
        histogram: dict[str, int] = {}
        for row in rows.values():
            floor = str(int(row["max_floor"]))
            histogram[floor] = histogram.get(floor, 0) + 1
        return {
            "wins": sum(
                row.get("outcome") == "act3_boss_victory" for row in rows.values()
            ),
            "floor_histogram": dict(sorted(histogram.items(), key=lambda pair: int(pair[0]))),
        }

    return {
        "selection_objective": "paired_mean_final_floor",
        "accept_candidate": mean_delta > 0.0,
        "episodes": count,
        "baseline": {
            "path": str(baseline_path),
            "mean_final_floor": baseline_mean,
            **diagnostics(baseline),
        },
        "candidate": {
            "path": str(candidate_path),
            "mean_final_floor": candidate_mean,
            **diagnostics(candidate),
        },
        "paired_delta": {
            "mean": mean_delta,
            "standard_error": standard_error,
            "confidence_95": [mean_delta - ci_radius, mean_delta + ci_radius],
            "improved_seeds": sum(delta > 0 for delta in deltas),
            "unchanged_seeds": sum(delta == 0 for delta in deltas),
            "regressed_seeds": sum(delta < 0 for delta in deltas),
        },
        "diagnostic_metrics_affect_selection": False,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    parser.add_argument("--output", type=Path)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    result = compare(args.baseline, args.candidate)
    rendered = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.output is not None:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(rendered)
    print(rendered, end="")


if __name__ == "__main__":
    main()
