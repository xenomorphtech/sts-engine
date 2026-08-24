#!/usr/bin/env python3
"""ES v4 trainer for the Defect HTN policy.

The default campaign is A20 and warm-starts from params_a20.json when it
exists, otherwise from the A0 bake in params_default.json.  The optimizer uses
mirrored sampling, common random numbers, full-population signed rank
utilities, and separable-NES per-dimension step sizes. Second-boss arrival is
an explicit positive signal so conversion training cannot crowd out reach.
Checkpoint validation sources are never reused and bakes average the last
generation means instead of selecting a noisy validation argmax.

Examples:
  python3 tools/opt_params4.py train --generations 80
  python3 tools/opt_params4.py train --remote box=/opt/sts/sts-htn,concurrent=24
  python3 tools/opt_params4.py bake --honest-count 10000
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import random
import re
import secrets
import subprocess
import sys
import tempfile
from typing import Iterable, Sequence


ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"
DEFAULT_BIN = ROOT / "target" / "release" / "sts-htn"
DEFAULT_A0_PARAMS = TOOLS / "params_default.json"
DEFAULT_A20_PARAMS = TOOLS / "params_a20.json"
DEFAULT_A0_STATE = TOOLS / "opt_state_a0_v4.json"
DEFAULT_A20_STATE = TOOLS / "opt_state_a20_reach_v4.json"
DEFAULT_REACH_WEIGHT = 1500.0

SUMMARY = re.compile(
    r"seeds=(?P<count>\d+).*wins=(?P<wins>\d+).*capped=(?P<capped>\d+)"
    r".*win_rate=(?P<win_rate>[\d.]+)%.*mean_floor_achieved=(?P<floor>[\d.]+)"
)
BOSS_PROGRESS = re.compile(
    r"a20_second_boss_entries=(?P<second_entries>\d+)"
    r".*mean_a20_second_boss_entry_hp_fraction=(?P<second_hp_fraction>[\d.]+)"
    r".*a20_second_boss_clears=(?P<second_clears>\d+)"
    r".*final_boss_entries=(?P<final_entries>\d+)"
    r".*mean_final_boss_entry_hp_fraction=(?P<final_hp_fraction>[\d.]+)"
)
LAST_BOSS_DAMAGE = re.compile(
    r"last_boss_fights=(?P<fights>\d+)"
    r".*last_boss_remaining_hp_sum=(?P<remaining_hp>\d+)"
    r".*mean_last_boss_damage_fraction=(?P<damage_fraction>[\d.]+)"
)


@dataclass(frozen=True)
class Dimension:
    name: str
    initial: float
    sigma: float
    low: float
    high: float


@dataclass
class Evaluation:
    count: int
    wins: int
    capped: int
    mean_floor: float
    outcomes: dict[int, bool]
    a20_second_boss_entries: int = 0
    mean_a20_second_boss_entry_hp_fraction: float = 0.0
    a20_second_boss_clears: int = 0
    final_boss_entries: int = 0
    mean_final_boss_entry_hp_fraction: float = 0.0
    last_boss_fights: int = 0
    last_boss_remaining_hp_sum: int = 0
    mean_last_boss_damage_fraction: float = 0.0
    gauntlet_count: int = 0
    gauntlet_wins: int = 0
    gauntlet_mean_damage_fraction: float = 0.0
    gauntlet_weight: float = 0.0
    reach_weight: float = 0.0
    stdout: str = ""

    @property
    def win_rate(self) -> float:
        return self.wins / self.count

    @property
    def floor_weight(self) -> float:
        # At A20 the floor signal carries the gradient until wins are common.
        return max(0.5, 6.0 * (1.0 - self.win_rate))

    @property
    def fitness(self) -> float:
        # The 0.3 normalized dense term from the roadmap becomes 300 on the
        # existing 0..1000 win-rate fitness scale.  Unreached Heart entries
        # contribute zero because the engine averages across every seed.
        full_run = (
            self.win_rate * 1000.0
            + self.floor_weight * self.mean_floor
            + self.reach_weight * self.a20_second_boss_entries / self.count
            + 300.0 * self.mean_a20_second_boss_entry_hp_fraction
            + 300.0 * self.a20_second_boss_clears / self.count
            + 300.0 * self.mean_final_boss_entry_hp_fraction
            + 300.0 * self.mean_last_boss_damage_fraction
        )
        if not self.gauntlet_count or self.gauntlet_weight <= 0.0:
            return full_run
        gauntlet = (
            1000.0 * self.gauntlet_wins / self.gauntlet_count
            + 300.0 * self.gauntlet_mean_damage_fraction
        )
        return full_run + self.gauntlet_weight * gauntlet


def numeric_leaves(value, prefix: str = "") -> Iterable[tuple[str, float]]:
    if isinstance(value, dict):
        for key, child in value.items():
            child_prefix = f"{prefix}.{key}" if prefix else key
            yield from numeric_leaves(child, child_prefix)
    elif isinstance(value, (int, float)) and not isinstance(value, bool):
        yield prefix, float(value)


def set_leaf(value: dict, name: str, number: float) -> None:
    parts = name.split(".")
    current = value
    for part in parts[:-1]:
        current = current[part]
    current[parts[-1]] = number


def dimension_for(name: str, value: float) -> Dimension:
    family = name.split(".", 1)[0]
    if family in {"pick", "upgrade", "boss_relic"}:
        sigma = {"pick": 14.0, "upgrade": 12.0, "boss_relic": 8.0}[family]
        high = {"pick": 360.0, "upgrade": 350.0, "boss_relic": 160.0}[family]
        return Dimension(name, value, sigma, 0.0, high)
    if 0.0 <= value <= 1.0:
        return Dimension(name, value, max(abs(value) * 0.10, 0.02), 0.0, 1.0)
    low, high = sorted((value * 0.3, value * 2.2))
    if value == 0.0:
        low, high = -1.0, 1.0
    return Dimension(name, value, max(abs(value) * 0.10, 0.03), low, high)


class ParamSpace:
    def __init__(self, template: dict):
        self.template = template
        self.dimensions = [dimension_for(name, value) for name, value in numeric_leaves(template)]
        self.names = [dimension.name for dimension in self.dimensions]
        if len(self.names) != len(set(self.names)):
            raise ValueError("duplicate parameter path")

    @classmethod
    def load(cls, path: Path) -> "ParamSpace":
        with path.open() as handle:
            return cls(json.load(handle))

    def vector(self) -> list[float]:
        return [dimension.initial for dimension in self.dimensions]

    def sigmas(self) -> list[float]:
        return [dimension.sigma for dimension in self.dimensions]

    def clamp(self, index: int, value: float) -> float:
        dimension = self.dimensions[index]
        return min(max(value, dimension.low), dimension.high)

    def to_json(self, vector: Sequence[float]) -> dict:
        if len(vector) != len(self.names):
            raise ValueError(f"vector has {len(vector)} dims, expected {len(self.names)}")
        # JSON round-tripping is a simple, reliable deep copy for this template.
        result = json.loads(json.dumps(self.template))
        for name, value in zip(self.names, vector):
            set_leaf(result, name, value)
        return result


def rank_utilities(fitnesses: Sequence[float]) -> list[float]:
    """Return standard SNES utilities, averaging ranks across exact ties."""
    count = len(fitnesses)
    if count < 2:
        raise ValueError("rank utilities require at least two candidates")
    raw = [max(0.0, math.log(count / 2.0 + 1.0) - math.log(rank + 1.0)) for rank in range(count)]
    total = sum(raw)
    ordered = sorted(range(count), key=lambda index: fitnesses[index], reverse=True)
    utilities = [0.0] * count
    offset = 0
    while offset < count:
        end = offset + 1
        score = fitnesses[ordered[offset]]
        while end < count and fitnesses[ordered[end]] == score:
            end += 1
        tied = sum(raw[offset:end]) / (end - offset) / total - 1.0 / count
        for rank in range(offset, end):
            utilities[ordered[rank]] = tied
        offset = end
    # Avoid accumulated float error becoming a tiny drift in every dimension.
    correction = sum(utilities) / count
    return [utility - correction for utility in utilities]


def snes_update(
    space: ParamSpace,
    mean: Sequence[float],
    sigmas: Sequence[float],
    epsilons: Sequence[Sequence[float]],
    utilities: Sequence[float],
    active: Sequence[int],
    global_scale: float,
    mean_rate: float,
    sigma_rate: float,
) -> tuple[list[float], list[float]]:
    if len(epsilons) != len(utilities):
        raise ValueError("one utility is required per perturbation")
    new_mean = list(mean)
    new_sigmas = list(sigmas)
    base_sigmas = space.sigmas()
    for index in active:
        mean_gradient = sum(weight * epsilon[index] for weight, epsilon in zip(utilities, epsilons))
        sigma_gradient = sum(
            weight * (epsilon[index] * epsilon[index] - 1.0)
            for weight, epsilon in zip(utilities, epsilons)
        )
        new_mean[index] = space.clamp(
            index,
            mean[index] + mean_rate * global_scale * sigmas[index] * mean_gradient,
        )
        adapted = sigmas[index] * math.exp(sigma_rate * sigma_gradient)
        new_sigmas[index] = min(max(adapted, base_sigmas[index] * 0.05), base_sigmas[index] * 20.0)
    return new_mean, new_sigmas


def tail_average(means: Sequence[Sequence[float]], width: int) -> list[float]:
    if not means:
        raise ValueError("state has no generation means to bake")
    tail = means[-width:]
    dimensions = len(tail[0])
    if any(len(mean) != dimensions for mean in tail):
        raise ValueError("tail means have inconsistent dimensions")
    return [sum(mean[index] for mean in tail) / len(tail) for index in range(dimensions)]


def load_stage_schedule(path: Path | None) -> list[dict]:
    if path is None:
        return []
    with path.open() as handle:
        schedule = json.load(handle)
    if not isinstance(schedule, list):
        raise ValueError("stage file must contain a JSON list")
    normalized = []
    for item in schedule:
        generation = int(item["generation"])
        patterns = list(item["patterns"])
        for pattern in patterns:
            re.compile(pattern)
        normalized.append({"generation": generation, "patterns": patterns})
    return sorted(normalized, key=lambda item: item["generation"])


def active_dimensions(names: Sequence[str], generation: int, schedule: Sequence[dict]) -> list[int]:
    if not schedule:
        return list(range(len(names)))
    patterns = [
        re.compile(pattern)
        for stage in schedule
        if stage["generation"] <= generation
        for pattern in stage["patterns"]
    ]
    return [index for index, name in enumerate(names) if any(pattern.search(name) for pattern in patterns)]


def derive_source(master_seed: int, purpose: str, generation: int, offset: int = 0) -> int:
    payload = f"{master_seed}:{purpose}:{generation}:{offset}".encode()
    number = int.from_bytes(hashlib.sha256(payload).digest()[:8], "big")
    return number & ((1 << 63) - 1)


def parse_batch_output(output: str) -> Evaluation:
    match = SUMMARY.search(output)
    if not match:
        raise RuntimeError(f"no batch summary in evaluator output: {output[:500]}")
    outcomes: dict[int, bool] = {}
    capped_seeds: list[int] = []
    header_seen = False
    for line in output.splitlines():
        if line.startswith("seed\toutcome\t"):
            header_seen = True
            continue
        if not header_seen or "\t" not in line:
            continue
        columns = line.split("\t")
        try:
            seed = int(columns[0])
        except (ValueError, IndexError):
            continue
        outcome = columns[1].strip().lower()
        outcomes[seed] = outcome == "win"
        if outcome == "capped":
            capped_seeds.append(seed)
    boss_progress = BOSS_PROGRESS.search(output)
    boss_damage = LAST_BOSS_DAMAGE.search(output)
    evaluation = Evaluation(
        count=int(match.group("count")),
        wins=int(match.group("wins")),
        capped=int(match.group("capped")),
        mean_floor=float(match.group("floor")),
        outcomes=outcomes,
        a20_second_boss_entries=(
            int(boss_progress.group("second_entries")) if boss_progress else 0
        ),
        mean_a20_second_boss_entry_hp_fraction=(
            float(boss_progress.group("second_hp_fraction")) if boss_progress else 0.0
        ),
        a20_second_boss_clears=(
            int(boss_progress.group("second_clears")) if boss_progress else 0
        ),
        final_boss_entries=int(boss_progress.group("final_entries")) if boss_progress else 0,
        mean_final_boss_entry_hp_fraction=(
            float(boss_progress.group("final_hp_fraction")) if boss_progress else 0.0
        ),
        last_boss_fights=int(boss_damage.group("fights")) if boss_damage else 0,
        last_boss_remaining_hp_sum=(
            int(boss_damage.group("remaining_hp")) if boss_damage else 0
        ),
        mean_last_boss_damage_fraction=(
            float(boss_damage.group("damage_fraction")) if boss_damage else 0.0
        ),
        stdout=output,
    )
    if evaluation.capped:
        raise RuntimeError(
            f"evaluator capped {evaluation.capped} seeds {capped_seeds}; capped seeds are loop bugs"
        )
    return evaluation


def aggregate_evaluations(evaluations: Sequence[Evaluation]) -> Evaluation:
    if not evaluations:
        raise ValueError("cannot aggregate zero evaluations")
    count = sum(evaluation.count for evaluation in evaluations)
    gauntlet_count = sum(evaluation.gauntlet_count for evaluation in evaluations)
    outcomes: dict[int, bool] = {}
    for evaluation in evaluations:
        duplicate_seeds = outcomes.keys() & evaluation.outcomes.keys()
        if duplicate_seeds:
            raise ValueError(f"evaluation cohorts overlap on seed {next(iter(duplicate_seeds))}")
        outcomes.update(evaluation.outcomes)
    return Evaluation(
        count=count,
        wins=sum(evaluation.wins for evaluation in evaluations),
        capped=sum(evaluation.capped for evaluation in evaluations),
        mean_floor=sum(evaluation.mean_floor * evaluation.count for evaluation in evaluations) / count,
        outcomes=outcomes,
        a20_second_boss_entries=sum(
            evaluation.a20_second_boss_entries for evaluation in evaluations
        ),
        mean_a20_second_boss_entry_hp_fraction=sum(
            evaluation.mean_a20_second_boss_entry_hp_fraction * evaluation.count
            for evaluation in evaluations
        ) / count,
        a20_second_boss_clears=sum(
            evaluation.a20_second_boss_clears for evaluation in evaluations
        ),
        final_boss_entries=sum(evaluation.final_boss_entries for evaluation in evaluations),
        mean_final_boss_entry_hp_fraction=sum(
            evaluation.mean_final_boss_entry_hp_fraction * evaluation.count
            for evaluation in evaluations
        ) / count,
        last_boss_fights=sum(evaluation.last_boss_fights for evaluation in evaluations),
        last_boss_remaining_hp_sum=sum(
            evaluation.last_boss_remaining_hp_sum for evaluation in evaluations
        ),
        mean_last_boss_damage_fraction=sum(
            evaluation.mean_last_boss_damage_fraction * evaluation.count
            for evaluation in evaluations
        ) / count,
        gauntlet_count=gauntlet_count,
        gauntlet_wins=sum(evaluation.gauntlet_wins for evaluation in evaluations),
        gauntlet_mean_damage_fraction=(
            sum(
                evaluation.gauntlet_mean_damage_fraction * evaluation.gauntlet_count
                for evaluation in evaluations
            )
            / gauntlet_count
            if gauntlet_count
            else 0.0
        ),
        gauntlet_weight=evaluations[0].gauntlet_weight,
        reach_weight=evaluations[0].reach_weight,
    )


@dataclass(frozen=True)
class Worker:
    host: str | None
    binary: str
    concurrent: int

    @property
    def label(self) -> str:
        return self.host or "local"


def parse_remote(spec: str) -> Worker:
    # Format: HOST=/absolute/path/to/sts-htn[,concurrent=N]
    target, *options = spec.split(",")
    if "=" not in target:
        raise ValueError("--remote must be HOST=/absolute/path/to/sts-htn[,concurrent=N]")
    host, binary = target.split("=", 1)
    concurrent = 12
    for option in options:
        key, separator, value = option.partition("=")
        if separator != "=" or key != "concurrent":
            raise ValueError(f"unknown remote worker option: {option}")
        concurrent = int(value)
    if not host or not binary or concurrent < 1:
        raise ValueError(f"invalid remote worker: {spec}")
    return Worker(host, binary, concurrent)


class EvaluatorPool:
    def __init__(
        self,
        workers: Sequence[Worker],
        ascension: int,
        timeout: int,
        boss_gauntlet: Path | None = None,
        gauntlet_weight: float = 0.0,
        reach_weight: float = 0.0,
    ):
        if not workers:
            raise ValueError("at least one evaluator worker is required")
        self.workers = list(workers)
        self.ascension = ascension
        self.timeout = timeout
        self.boss_gauntlet = boss_gauntlet
        self.gauntlet_weight = gauntlet_weight
        self.reach_weight = reach_weight

    def _command(
        self,
        worker: Worker,
        params_path: str,
        count: int,
        source: int,
        boss_gauntlet: str | None = None,
    ) -> list[str]:
        command = [
            worker.binary,
            "--character", "DEFECT",
            f"--a{self.ascension}",
            "--concurrent", str(worker.concurrent),
        ]
        if boss_gauntlet is not None:
            command.extend(["--boss-gauntlet-jsonl", boss_gauntlet])
        else:
            command.extend(["--count", str(count), "--seed-source", str(source)])
        return command

    def evaluate(
        self,
        worker: Worker,
        params: dict,
        count: int,
        source: int,
        tag: str,
        include_gauntlet: bool = True,
    ) -> Evaluation:
        safe_tag = re.sub(r"[^A-Za-z0-9_.-]", "_", tag)
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".json", prefix=f"sts_es4_{safe_tag}_", delete=False
        ) as handle:
            json.dump(params, handle)
            local_path = handle.name
        remote_path = f"/tmp/sts_es4_{os.getpid()}_{safe_tag}_{secrets.token_hex(4)}.json"
        remote_gauntlet_path = f"{remote_path}.boss.jsonl"
        failed = False
        try:
            if worker.host is None:
                environment = dict(os.environ, STS_HTN_PARAMS=local_path)
                completed = subprocess.run(
                    self._command(worker, local_path, count, source),
                    env=environment,
                    capture_output=True,
                    text=True,
                    timeout=self.timeout,
                )
                gauntlet_completed = None
                if include_gauntlet and self.boss_gauntlet is not None:
                    gauntlet_completed = subprocess.run(
                        self._command(
                            worker,
                            local_path,
                            count,
                            source,
                            str(self.boss_gauntlet),
                        ),
                        env=environment,
                        capture_output=True,
                        text=True,
                        timeout=self.timeout,
                    )
            else:
                copied = subprocess.run(
                    ["scp", "-q", local_path, f"{worker.host}:{remote_path}"],
                    capture_output=True,
                    text=True,
                    timeout=120,
                )
                if copied.returncode:
                    raise RuntimeError(f"scp to {worker.label} failed: {copied.stderr.strip()}")
                if include_gauntlet and self.boss_gauntlet is not None:
                    copied = subprocess.run(
                        ["scp", "-q", str(self.boss_gauntlet), f"{worker.host}:{remote_gauntlet_path}"],
                        capture_output=True,
                        text=True,
                        timeout=120,
                    )
                    if copied.returncode:
                        raise RuntimeError(
                            f"gauntlet scp to {worker.label} failed: {copied.stderr.strip()}"
                        )
                remote_command = ["env", f"STS_HTN_PARAMS={remote_path}"] + self._command(
                    worker, remote_path, count, source
                )
                completed = subprocess.run(
                    ["ssh", worker.host, *remote_command],
                    capture_output=True,
                    text=True,
                    timeout=self.timeout,
                )
                gauntlet_completed = None
                if include_gauntlet and self.boss_gauntlet is not None:
                    remote_command = ["env", f"STS_HTN_PARAMS={remote_path}"] + self._command(
                        worker,
                        remote_path,
                        count,
                        source,
                        remote_gauntlet_path,
                    )
                    gauntlet_completed = subprocess.run(
                        ["ssh", worker.host, *remote_command],
                        capture_output=True,
                        text=True,
                        timeout=self.timeout,
                    )
            if completed.returncode:
                raise RuntimeError(
                    f"evaluator {worker.label} exited {completed.returncode}: "
                    f"{completed.stderr[-1000:]} {completed.stdout[-1000:]}"
                )
            if gauntlet_completed is not None and gauntlet_completed.returncode:
                raise RuntimeError(
                    f"gauntlet evaluator {worker.label} exited {gauntlet_completed.returncode}: "
                    f"{gauntlet_completed.stderr[-1000:]} {gauntlet_completed.stdout[-1000:]}"
                )
            try:
                evaluation = parse_batch_output(completed.stdout)
                evaluation.reach_weight = self.reach_weight
                if gauntlet_completed is not None:
                    gauntlet = parse_batch_output(gauntlet_completed.stdout)
                    evaluation.gauntlet_count = gauntlet.count
                    evaluation.gauntlet_wins = gauntlet.wins
                    evaluation.gauntlet_mean_damage_fraction = (
                        gauntlet.mean_last_boss_damage_fraction
                    )
                    evaluation.gauntlet_weight = self.gauntlet_weight
                    evaluation.stdout += "\n" + gauntlet.stdout
                return evaluation
            except RuntimeError as error:
                failed = True
                raise RuntimeError(
                    f"{tag} on {worker.label}: {error}; candidate params retained at {local_path}"
                ) from error
        except RuntimeError as error:
            failed = True
            if "candidate params retained" in str(error):
                raise
            raise RuntimeError(
                f"{tag} on {worker.label}: {error}; candidate params retained at {local_path}"
            ) from error
        finally:
            if not failed:
                try:
                    os.unlink(local_path)
                except FileNotFoundError:
                    pass
            if worker.host is not None:
                subprocess.run(
                    ["ssh", worker.host, "rm", "-f", remote_path, remote_gauntlet_path],
                    capture_output=True,
                    text=True,
                    timeout=30,
                )

    def evaluate_many(
        self,
        parameter_sets: Sequence[dict],
        count: int,
        source: int,
        tag: str,
        include_gauntlet: bool = True,
    ) -> list[Evaluation]:
        available: queue.Queue[Worker] = queue.Queue()
        for worker in self.workers:
            available.put(worker)

        def run(index: int, params: dict) -> tuple[int, Evaluation]:
            worker = available.get()
            try:
                return index, self.evaluate(
                    worker,
                    params,
                    count,
                    source,
                    f"{tag}_{index}",
                    include_gauntlet,
                )
            finally:
                available.put(worker)

        results: list[Evaluation | None] = [None] * len(parameter_sets)
        with ThreadPoolExecutor(max_workers=len(self.workers)) as executor:
            futures = [executor.submit(run, index, params) for index, params in enumerate(parameter_sets)]
            for future in as_completed(futures):
                index, evaluation = future.result()
                results[index] = evaluation
        return [result for result in results if result is not None]


def atomic_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    with temporary.open("w") as handle:
        json.dump(value, handle, indent=1)
        handle.write("\n")
    os.replace(temporary, path)


def initial_state(space: ParamSpace, args, schedule: list[dict]) -> dict:
    levels = [1.0, 0.6, 0.45, 0.30, 0.25] if args.ascension == 20 else [0.6, 0.45, 0.30, 0.25]
    return {
        "version": 4,
        "ascension": args.ascension,
        "names": space.names,
        "mean": space.vector(),
        "sigmas": space.sigmas(),
        "global_levels": levels,
        "global_level": 0,
        "last_global_change": 0,
        "generation": 0,
        "master_seed": args.master_seed,
        "tail_width": args.tail_width,
        "tail_means": [],
        "history": [],
        "validations": [],
        "validation_sources": [],
        "stage_schedule": schedule,
        "start_params": str(args.start_params),
    }


def align_state(state: dict, space: ParamSpace, ascension: int) -> dict:
    if state.get("version") != 4:
        raise ValueError("state is not an ES v4 state")
    if state["ascension"] != ascension:
        raise ValueError(f"state is A{state['ascension']}, requested A{ascension}")
    old_names = state["names"]
    removed = set(old_names) - set(space.names)
    if removed:
        raise ValueError(f"start params removed optimizer dimensions: {sorted(removed)[:5]}")
    if old_names != space.names:
        old_mean = dict(zip(old_names, state["mean"]))
        old_sigma = dict(zip(old_names, state["sigmas"]))
        state["mean"] = [old_mean.get(name, dimension.initial) for name, dimension in zip(space.names, space.dimensions)]
        state["sigmas"] = [old_sigma.get(name, dimension.sigma) for name, dimension in zip(space.names, space.dimensions)]
        for mean in state["tail_means"]:
            prior = dict(zip(old_names, mean))
            mean[:] = [prior.get(name, dimension.initial) for name, dimension in zip(space.names, space.dimensions)]
        state["names"] = space.names
    return state


def maybe_anneal_global(state: dict, warmup_generations: int, window_generations: int, min_gain: float) -> bool:
    if state["global_level"] >= len(state["global_levels"]) - 1:
        return False
    if state["ascension"] == 20 and state["generation"] < warmup_generations:
        return False
    validations = state["validations"]
    current = validations[-1]
    last_change = state.get("last_global_change", 0)
    earlier = [
        item
        for item in validations[:-1]
        if item["generation"] >= last_change
        and current["generation"] - item["generation"] >= window_generations
    ]
    if not earlier:
        return False
    reference = earlier[-1]
    current_progress = current.get("anneal_score", current["win_rate"])
    reference_progress = reference.get("anneal_score", reference["win_rate"])
    if current_progress - reference_progress >= min_gain:
        return False
    state["global_level"] += 1
    state["last_global_change"] = state["generation"]
    return True


def validate(
    pool: EvaluatorPool,
    space: ParamSpace,
    state: dict,
    args,
) -> None:
    evaluations = []
    baseline_evaluations = []
    sources = []
    for offset in range(args.validation_cohorts):
        source = derive_source(state["master_seed"], "validation", state["generation"], offset)
        if source in state["validation_sources"]:
            raise RuntimeError(f"validation source reused: {source}")
        sources.append(source)
        evaluations.append(
            pool.evaluate(
                pool.workers[offset % len(pool.workers)],
                space.to_json(state["mean"]),
                args.validation_count,
                source,
                f"validation_g{state['generation']}_{offset}",
                include_gauntlet=False,
            )
        )
        baseline_evaluations.append(
            pool.evaluate(
                pool.workers[offset % len(pool.workers)],
                space.template,
                args.validation_count,
                source,
                f"validation_baseline_g{state['generation']}_{offset}",
                include_gauntlet=False,
            )
        )
    candidate = aggregate_evaluations(evaluations)
    baseline = aggregate_evaluations(baseline_evaluations)
    total = candidate.count
    wins = candidate.wins
    paired = mcnemar(candidate, baseline)
    point = {
        "generation": state["generation"],
        "sources": sources,
        "count": total,
        "wins": wins,
        "win_rate": wins / total,
        "a20_second_boss_entry_rate": candidate.a20_second_boss_entries / total,
        "anneal_score": (
            wins / total
            + pool.reach_weight / 1000.0 * candidate.a20_second_boss_entries / total
        ),
        "mean_floor": candidate.mean_floor,
        "a20_second_boss_entries": candidate.a20_second_boss_entries,
        "mean_a20_second_boss_entry_hp_fraction": (
            candidate.mean_a20_second_boss_entry_hp_fraction
        ),
        "a20_second_boss_clears": candidate.a20_second_boss_clears,
        "final_boss_entries": candidate.final_boss_entries,
        "mean_final_boss_entry_hp_fraction": candidate.mean_final_boss_entry_hp_fraction,
        "last_boss_fights": candidate.last_boss_fights,
        "last_boss_remaining_hp_sum": candidate.last_boss_remaining_hp_sum,
        "mean_last_boss_damage_fraction": candidate.mean_last_boss_damage_fraction,
        "baseline_wins": baseline.wins,
        "baseline_mean_floor": baseline.mean_floor,
        "baseline_a20_second_boss_entries": baseline.a20_second_boss_entries,
        "baseline_mean_a20_second_boss_entry_hp_fraction": (
            baseline.mean_a20_second_boss_entry_hp_fraction
        ),
        "baseline_a20_second_boss_clears": baseline.a20_second_boss_clears,
        "baseline_final_boss_entries": baseline.final_boss_entries,
        "baseline_mean_final_boss_entry_hp_fraction": (
            baseline.mean_final_boss_entry_hp_fraction
        ),
        "baseline_last_boss_fights": baseline.last_boss_fights,
        "baseline_last_boss_remaining_hp_sum": baseline.last_boss_remaining_hp_sum,
        "baseline_mean_last_boss_damage_fraction": baseline.mean_last_boss_damage_fraction,
        "mcnemar": paired,
    }
    state["validations"].append(point)
    state["validation_sources"].extend(sources)
    annealed = maybe_anneal_global(
        state,
        args.a20_warmup,
        args.slope_window,
        args.slope_min_gain,
    )
    suffix = f" sigma->{state['global_levels'][state['global_level']]:.2f}" if annealed else ""
    print(
        f"  fresh validation {wins}/{total} ({100.0 * wins / total:.2f}%) "
        f"baseline={baseline.wins}/{baseline.count} "
        f"paired_delta={paired['candidate_only_wins'] - paired['baseline_only_wins']} "
        f"floor={point['mean_floor']:.2f} second_boss={point['a20_second_boss_entries']} "
        f"second_clears={point['a20_second_boss_clears']} "
        f"baseline_second={baseline.a20_second_boss_entries} "
        f"baseline_clears={baseline.a20_second_boss_clears} "
        f"heart_entries={point['final_boss_entries']} "
        f"boss_hp_left={point['last_boss_remaining_hp_sum']} "
        f"heart_hp={point['mean_final_boss_entry_hp_fraction']:.4f}{suffix}",
        flush=True,
    )


def build_workers(args) -> list[Worker]:
    workers = [Worker(None, str(args.binary), args.concurrent)]
    workers.extend(parse_remote(spec) for spec in args.remote)
    if workers[0].binary and not Path(workers[0].binary).exists():
        raise FileNotFoundError(f"local evaluator binary does not exist: {workers[0].binary}")
    return workers


def train(args) -> int:
    space = ParamSpace.load(args.start_params)
    schedule = load_stage_schedule(args.stage_file)
    requested_reach_weight = args.reach_weight
    if args.population < 4 or args.population % 2:
        raise ValueError("--population must be an even number >= 4")
    if args.seeds < 1:
        raise ValueError("--seeds must be positive")
    if args.gauntlet_weight < 0.0:
        raise ValueError("--gauntlet-weight must be non-negative")
    if args.boss_gauntlet is not None and not args.boss_gauntlet.is_file():
        raise ValueError(f"boss gauntlet does not exist: {args.boss_gauntlet}")
    if args.state.exists():
        with args.state.open() as handle:
            state = align_state(json.load(handle), space, args.ascension)
        if schedule and schedule != state["stage_schedule"]:
            raise ValueError("stage schedule differs from the resumed state")
    else:
        state = initial_state(space, args, schedule)
    if requested_reach_weight is None:
        args.reach_weight = float(state.get("reach_weight", DEFAULT_REACH_WEIGHT))
    if args.reach_weight < 0.0:
        raise ValueError("--reach-weight must be non-negative")
    state["reach_weight"] = args.reach_weight
    stored_gauntlet = state.get("boss_gauntlet")
    if args.boss_gauntlet is None and stored_gauntlet:
        args.boss_gauntlet = Path(stored_gauntlet)
        args.gauntlet_weight = float(state.get("gauntlet_weight", args.gauntlet_weight))
    elif args.boss_gauntlet is not None:
        state["boss_gauntlet"] = str(args.boss_gauntlet)
        state["gauntlet_weight"] = args.gauntlet_weight
    if args.boss_gauntlet is not None and not args.boss_gauntlet.is_file():
        raise ValueError(f"boss gauntlet does not exist: {args.boss_gauntlet}")
    pool = EvaluatorPool(
        build_workers(args),
        args.ascension,
        args.timeout,
        args.boss_gauntlet,
        args.gauntlet_weight,
        args.reach_weight,
    )
    print(
        f"ES v4 A{args.ascension}: dims={len(space.names)} population={args.population} "
        f"seeds={args.seeds} workers={','.join(worker.label for worker in pool.workers)} "
        f"start_gen={state['generation']} "
        f"boss_gauntlet={args.boss_gauntlet or '-'} weight={args.gauntlet_weight:.2f} "
        f"reach_weight={args.reach_weight:.1f}",
        flush=True,
    )
    for _ in range(args.generations):
        generation = state["generation"]
        # A generation-local stream makes resume-at-generation-N bit-identical
        # to an uninterrupted campaign at generation N.
        rng = random.Random(derive_source(state["master_seed"], "optimizer", generation))
        active = active_dimensions(space.names, generation, state["stage_schedule"])
        if not active:
            raise RuntimeError(f"stage schedule activates no dimensions at generation {generation}")
        global_scale = state["global_levels"][state["global_level"]]
        epsilons: list[list[float]] = []
        candidates: list[list[float]] = []
        for _pair in range(args.population // 2):
            epsilon = [0.0] * len(space.names)
            for index in active:
                epsilon[index] = rng.gauss(0.0, 1.0)
            for sign in (1.0, -1.0):
                signed = [sign * value for value in epsilon]
                candidate = list(state["mean"])
                for index in active:
                    candidate[index] = space.clamp(
                        index,
                        state["mean"][index] + global_scale * state["sigmas"][index] * signed[index],
                    )
                epsilons.append(signed)
                candidates.append(candidate)
        source = derive_source(state["master_seed"], "search", generation)
        evaluations = pool.evaluate_many(
            [space.to_json(candidate) for candidate in candidates],
            args.seeds,
            source,
            f"g{generation}",
        )
        fitnesses = [evaluation.fitness for evaluation in evaluations]
        utilities = rank_utilities(fitnesses)
        state["mean"], state["sigmas"] = snes_update(
            space,
            state["mean"],
            state["sigmas"],
            epsilons,
            utilities,
            active,
            global_scale,
            args.mean_rate,
            args.sigma_rate,
        )
        state["generation"] += 1
        state["tail_means"].append(state["mean"][:])
        state["tail_means"] = state["tail_means"][-state["tail_width"] :]
        ordered = sorted(evaluations, key=lambda evaluation: evaluation.fitness, reverse=True)
        sigma_ratios = [sigma / base for sigma, base in zip(state["sigmas"], space.sigmas())]
        record = {
            "generation": state["generation"],
            "source": source,
            "active_dimensions": len(active),
            "top_fitness": ordered[0].fitness,
            "median_fitness": ordered[len(ordered) // 2].fitness,
            "top_wins": ordered[0].wins,
            "population_wins": sum(evaluation.wins for evaluation in evaluations),
            "top_final_boss_entries": ordered[0].final_boss_entries,
            "top_a20_second_boss_entries": ordered[0].a20_second_boss_entries,
            "top_mean_a20_second_boss_entry_hp_fraction": (
                ordered[0].mean_a20_second_boss_entry_hp_fraction
            ),
            "top_a20_second_boss_clears": ordered[0].a20_second_boss_clears,
            "population_final_boss_entries": sum(
                evaluation.final_boss_entries for evaluation in evaluations
            ),
            "population_a20_second_boss_entries": sum(
                evaluation.a20_second_boss_entries for evaluation in evaluations
            ),
            "top_last_boss_fights": ordered[0].last_boss_fights,
            "top_last_boss_remaining_hp_sum": ordered[0].last_boss_remaining_hp_sum,
            "top_mean_last_boss_damage_fraction": ordered[0].mean_last_boss_damage_fraction,
            "top_gauntlet_wins": ordered[0].gauntlet_wins,
            "gauntlet_count": ordered[0].gauntlet_count,
            "top_gauntlet_mean_damage_fraction": (
                ordered[0].gauntlet_mean_damage_fraction
            ),
            "reach_weight": args.reach_weight,
            "global_sigma": global_scale,
            "sigma_ratio_min": min(sigma_ratios[index] for index in active),
            "sigma_ratio_median": sorted(sigma_ratios[index] for index in active)[len(active) // 2],
            "sigma_ratio_max": max(sigma_ratios[index] for index in active),
        }
        state["history"].append(record)
        print(
            f"gen {state['generation']} top={record['top_fitness']:.1f} "
            f"median={record['median_fitness']:.1f} top_wins={record['top_wins']}/{args.seeds} "
            f"second_boss={record['top_a20_second_boss_entries']} "
            f"second_clears={record['top_a20_second_boss_clears']} "
            f"heart_entries={record['top_final_boss_entries']} "
            f"boss_hp_left={record['top_last_boss_remaining_hp_sum']}/"
            f"{record['top_last_boss_fights']}losses "
            f"damage={record['top_mean_last_boss_damage_fraction']:.4f} "
            f"gauntlet={record['top_gauntlet_wins']}/{record['gauntlet_count']} "
            f"sigma={global_scale:.2f} dims={len(active)} "
            f"snes_sigma={record['sigma_ratio_median']:.2f}x",
            flush=True,
        )
        if args.validation_every and state["generation"] % args.validation_every == 0:
            validate(pool, space, state, args)
        atomic_json(args.state, state)
    return 0


def mcnemar(candidate: Evaluation, baseline: Evaluation) -> dict:
    common = set(candidate.outcomes) & set(baseline.outcomes)
    candidate_only = sum(candidate.outcomes[seed] and not baseline.outcomes[seed] for seed in common)
    baseline_only = sum(baseline.outcomes[seed] and not candidate.outcomes[seed] for seed in common)
    discordant = candidate_only + baseline_only
    if discordant:
        tail = sum(math.comb(discordant, index) for index in range(min(candidate_only, baseline_only) + 1))
        # Keep the binomial tail exact as an integer, then enter floating
        # point in log space. Direct division overflows once the discordant
        # cohort is large even though the final probability is representable.
        log_p_value = math.log(2.0) + math.log(tail) - discordant * math.log(2.0)
        p_value = 1.0 if log_p_value >= 0.0 else math.exp(log_p_value)
    else:
        p_value = 1.0
    return {
        "paired_seeds": len(common),
        "candidate_only_wins": candidate_only,
        "baseline_only_wins": baseline_only,
        "discordant": discordant,
        "exact_two_sided_p": p_value,
    }


def compare_checkpoint(args) -> int:
    with args.state.open() as handle:
        state = json.load(handle)
    space = ParamSpace.load(args.start_params)
    state = align_state(state, space, args.ascension)
    vector = (
        tail_average(state["tail_means"], state.get("tail_width", 8))
        if args.tail
        else state["mean"]
    )
    candidate_params = space.to_json(vector)
    baseline_params = ParamSpace.load(args.baseline).template
    sources = args.source or [secrets.randbelow(1 << 63)]
    pool = EvaluatorPool(build_workers(args), args.ascension, args.timeout)
    candidates = []
    baselines = []
    for offset, source in enumerate(sources):
        candidates.append(
            pool.evaluate(
                pool.workers[offset % len(pool.workers)],
                candidate_params,
                args.count,
                source,
                f"compare_candidate_{offset}",
            )
        )
        baselines.append(
            pool.evaluate(
                pool.workers[offset % len(pool.workers)],
                baseline_params,
                args.count,
                source,
                f"compare_baseline_{offset}",
            )
        )
    candidate = aggregate_evaluations(candidates)
    baseline = aggregate_evaluations(baselines)
    paired = mcnemar(candidate, baseline)
    report = {
        "generation": state["generation"],
        "candidate": "tail" if args.tail else "mean",
        "sources": sources,
        "count": candidate.count,
        "candidate_wins": candidate.wins,
        "baseline_wins": baseline.wins,
        "candidate_mean_floor": candidate.mean_floor,
        "baseline_mean_floor": baseline.mean_floor,
        "candidate_a20_second_boss_entries": candidate.a20_second_boss_entries,
        "candidate_a20_second_boss_clears": candidate.a20_second_boss_clears,
        "baseline_a20_second_boss_entries": baseline.a20_second_boss_entries,
        "baseline_a20_second_boss_clears": baseline.a20_second_boss_clears,
        "candidate_final_boss_entries": candidate.final_boss_entries,
        "baseline_final_boss_entries": baseline.final_boss_entries,
        "candidate_last_boss_remaining_hp_sum": candidate.last_boss_remaining_hp_sum,
        "baseline_last_boss_remaining_hp_sum": baseline.last_boss_remaining_hp_sum,
        "candidate_mean_last_boss_damage_fraction": candidate.mean_last_boss_damage_fraction,
        "baseline_mean_last_boss_damage_fraction": baseline.mean_last_boss_damage_fraction,
        "mcnemar": paired,
    }
    print(json.dumps(report, indent=2))
    return 0


def bake(args) -> int:
    with args.state.open() as handle:
        state = json.load(handle)
    space = ParamSpace.load(args.start_params)
    state = align_state(state, space, args.ascension)
    vector = tail_average(state["tail_means"], state.get("tail_width", 8))
    params = space.to_json(vector)
    report = None
    if args.honest_count:
        pool = EvaluatorPool(build_workers(args), args.ascension, args.timeout)
        source = args.honest_source if args.honest_source is not None else secrets.randbelow(1 << 63)
        if source in state.get("validation_sources", []):
            raise ValueError("honest source was already used for checkpoint validation")
        candidate = pool.evaluate(pool.workers[0], params, args.honest_count, source, "honest_candidate")
        baseline_params = ParamSpace.load(args.baseline).template
        baseline = pool.evaluate(pool.workers[0], baseline_params, args.honest_count, source, "honest_baseline")
        report = {
            "source": source,
            "count": args.honest_count,
            "candidate_wins": candidate.wins,
            "candidate_win_rate": candidate.win_rate,
            "candidate_mean_floor": candidate.mean_floor,
            "candidate_a20_second_boss_entries": candidate.a20_second_boss_entries,
            "candidate_a20_second_boss_clears": candidate.a20_second_boss_clears,
            "candidate_mean_a20_second_boss_entry_hp_fraction": (
                candidate.mean_a20_second_boss_entry_hp_fraction
            ),
            "candidate_final_boss_entries": candidate.final_boss_entries,
            "candidate_mean_final_boss_entry_hp_fraction": (
                candidate.mean_final_boss_entry_hp_fraction
            ),
            "candidate_last_boss_fights": candidate.last_boss_fights,
            "candidate_last_boss_remaining_hp_sum": candidate.last_boss_remaining_hp_sum,
            "candidate_mean_last_boss_damage_fraction": (
                candidate.mean_last_boss_damage_fraction
            ),
            "baseline": str(args.baseline),
            "baseline_wins": baseline.wins,
            "baseline_win_rate": baseline.win_rate,
            "baseline_mean_floor": baseline.mean_floor,
            "baseline_a20_second_boss_entries": baseline.a20_second_boss_entries,
            "baseline_a20_second_boss_clears": baseline.a20_second_boss_clears,
            "baseline_mean_a20_second_boss_entry_hp_fraction": (
                baseline.mean_a20_second_boss_entry_hp_fraction
            ),
            "baseline_final_boss_entries": baseline.final_boss_entries,
            "baseline_mean_final_boss_entry_hp_fraction": (
                baseline.mean_final_boss_entry_hp_fraction
            ),
            "baseline_last_boss_fights": baseline.last_boss_fights,
            "baseline_last_boss_remaining_hp_sum": baseline.last_boss_remaining_hp_sum,
            "baseline_mean_last_boss_damage_fraction": (
                baseline.mean_last_boss_damage_fraction
            ),
            "mcnemar": mcnemar(candidate, baseline),
        }
        print(
            f"honest A{args.ascension}: candidate={candidate.wins}/{candidate.count} "
            f"baseline={baseline.wins}/{baseline.count} "
            f"paired_delta={report['mcnemar']['candidate_only_wins'] - report['mcnemar']['baseline_only_wins']} "
            f"p={report['mcnemar']['exact_two_sided_p']:.4g}",
            flush=True,
        )
    atomic_json(args.output, params)
    bake_record = {
        "generation": state["generation"],
        "tail_means": min(len(state["tail_means"]), state.get("tail_width", 8)),
        "output": str(args.output),
        "honest": report,
    }
    state.setdefault("bakes", []).append(bake_record)
    atomic_json(args.state, state)
    print(
        f"baked tail-average of {bake_record['tail_means']} means to {args.output}",
        flush=True,
    )
    return 0


def default_start_params(ascension: int) -> Path:
    if ascension == 20 and DEFAULT_A20_PARAMS.exists():
        return DEFAULT_A20_PARAMS
    return DEFAULT_A0_PARAMS


def add_evaluator_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--binary", type=Path, default=DEFAULT_BIN)
    parser.add_argument("--concurrent", type=int, default=12, help="threads used by the local binary")
    parser.add_argument(
        "--remote",
        action="append",
        default=[],
        metavar="HOST=BINARY[,concurrent=N]",
        help="add one SSH evaluator; may be repeated",
    )
    parser.add_argument("--timeout", type=int, default=1800)


def parse_args(argv: Sequence[str] | None = None):
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    train_parser = subparsers.add_parser("train", help="run ES generations")
    train_parser.add_argument("--generations", type=int, default=80)
    train_parser.add_argument("--ascension", type=int, choices=(0, 20), default=20)
    train_parser.add_argument("--start-params", type=Path)
    train_parser.add_argument("--state", type=Path)
    train_parser.add_argument("--population", type=int, default=24)
    train_parser.add_argument("--seeds", type=int, default=250)
    train_parser.add_argument("--mean-rate", type=float, default=1.0)
    train_parser.add_argument("--sigma-rate", type=float, default=0.06)
    train_parser.add_argument("--tail-width", type=int, default=8)
    train_parser.add_argument("--master-seed", type=int, default=0xA20E54)
    train_parser.add_argument("--validation-every", type=int, default=6)
    train_parser.add_argument("--validation-count", type=int, default=1000)
    train_parser.add_argument("--validation-cohorts", type=int, default=2)
    train_parser.add_argument("--a20-warmup", type=int, default=30)
    train_parser.add_argument("--slope-window", type=int, default=12)
    train_parser.add_argument(
        "--slope-min-gain",
        type=float,
        default=0.004,
        help="hold global sigma when validation win rate gains at least this much",
    )
    train_parser.add_argument(
        "--stage-file",
        type=Path,
        help="JSON stages with generation/patterns; activations are cumulative",
    )
    train_parser.add_argument(
        "--boss-gauntlet",
        type=Path,
        help="A20 second-boss action-prefix JSONL evaluated for every candidate",
    )
    train_parser.add_argument(
        "--gauntlet-weight",
        type=float,
        default=0.35,
        help="weight of boss-clear and boss-damage fitness alongside full-run reach",
    )
    train_parser.add_argument(
        "--reach-weight",
        type=float,
        help=(
            "fitness points per second-boss entry rate; defaults to 1500 and "
            "is restored from resumed state"
        ),
    )
    add_evaluator_args(train_parser)

    bake_parser = subparsers.add_parser("bake", help="tail-average and honestly evaluate a bake")
    bake_parser.add_argument("--ascension", type=int, choices=(0, 20), default=20)
    bake_parser.add_argument("--start-params", type=Path)
    bake_parser.add_argument("--state", type=Path)
    bake_parser.add_argument("--output", type=Path)
    bake_parser.add_argument("--baseline", type=Path)
    bake_parser.add_argument("--honest-count", type=int, default=10000)
    bake_parser.add_argument("--honest-source", type=int)
    add_evaluator_args(bake_parser)

    compare_parser = subparsers.add_parser(
        "compare", help="pair a checkpoint against its start policy on identical cohorts"
    )
    compare_parser.add_argument("--ascension", type=int, choices=(0, 20), default=20)
    compare_parser.add_argument("--start-params", type=Path)
    compare_parser.add_argument("--state", type=Path)
    compare_parser.add_argument("--baseline", type=Path)
    compare_parser.add_argument("--count", type=int, default=1000, help="seeds per source")
    compare_parser.add_argument("--source", action="append", type=int)
    compare_parser.add_argument("--tail", action="store_true", help="compare the tail average")
    add_evaluator_args(compare_parser)

    args = parser.parse_args(argv)
    if args.state is None:
        args.state = DEFAULT_A20_STATE if args.ascension == 20 else DEFAULT_A0_STATE
    if args.command == "bake" and args.output is None:
        args.output = DEFAULT_A20_PARAMS if args.ascension == 20 else DEFAULT_A0_PARAMS
    if args.start_params is None:
        # A bake may create params_a20.json after this campaign began.  Resume
        # against the original template so bounds and sigma baselines do not
        # silently change underneath an existing state.
        resumed_start = None
        if args.state.exists():
            try:
                with args.state.open() as handle:
                    resumed_start = json.load(handle).get("start_params")
            except (OSError, ValueError):
                resumed_start = None
        args.start_params = Path(resumed_start) if resumed_start else default_start_params(args.ascension)
    if args.command in {"bake", "compare"} and args.baseline is None:
        args.baseline = args.start_params
    return args


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv)
    if args.command == "train":
        return train(args)
    if args.command == "bake":
        return bake(args)
    if args.command == "compare":
        return compare_checkpoint(args)
    raise AssertionError(args.command)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (ValueError, RuntimeError, FileNotFoundError, subprocess.TimeoutExpired) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
