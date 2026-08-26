#!/usr/bin/env python3
"""Export a combat HRM checkpoint to the trainer-neutral Rust runtime format."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any

import torch
from torch import nn

from train_hrm_combat import CombatHrm, MODEL_DEFAULTS

DEFAULT_RUNTIME_BATCH_SIZE = 10


class RuntimePolicy(nn.Module):
    """Closed inference graph: both recurrent segments in, action logits out."""

    def __init__(self, model: CombatHrm):
        super().__init__()
        self.model = model

    def forward(self, input_ids: torch.Tensor) -> torch.Tensor:
        carry = None
        logits = None
        for _ in range(MODEL_DEFAULTS["deep_supervision_segments"]):
            carry, logits, _progress = self.model.segment(input_ids, carry)
        if logits is None:
            raise RuntimeError("the runtime policy requires at least one segment")
        # Keep the heavy graph in the requested precision, but expose logits
        # as float32 so every runtime can apply legal masking without a
        # half-precision host dependency.
        return logits.float()


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def export_checkpoint(
    checkpoint_path: Path,
    onnx_path: Path,
    metadata_path: Path,
    precision: str,
    fixed_batch_size: int | None = DEFAULT_RUNTIME_BATCH_SIZE,
) -> None:
    checkpoint: dict[str, Any] = torch.load(
        checkpoint_path,
        map_location="cpu",
        weights_only=False,
    )
    if checkpoint.get("model_defaults") != MODEL_DEFAULTS:
        raise ValueError(
            "checkpoint model defaults differ from this exporter; use the "
            "trainer revision that created the checkpoint"
        )

    vocabulary = checkpoint["vocabulary"]
    action_list = checkpoint["action_list"]
    model = CombatHrm(len(vocabulary), len(action_list))
    model.load_state_dict(checkpoint["model_state"], strict=True)
    runtime = RuntimePolicy(model.eval()).eval()
    if precision == "float16":
        runtime = runtime.half()

    onnx_path.parent.mkdir(parents=True, exist_ok=True)
    pending_onnx = onnx_path.with_suffix(onnx_path.suffix + ".pending")
    dummy = torch.zeros(
        (fixed_batch_size or 2, MODEL_DEFAULTS["max_tokens"]),
        dtype=torch.int64,
    )
    dummy[:, 0] = 2
    with torch.inference_mode():
        torch.onnx.export(
            runtime,
            (dummy,),
            pending_onnx,
            input_names=["input_ids"],
            output_names=["action_logits"],
            dynamic_axes=None
            if fixed_batch_size is not None
            else {
                "input_ids": {0: "batch"},
                "action_logits": {0: "batch"},
            },
            opset_version=20,
            do_constant_folding=True,
            dynamo=True,
            external_data=False,
        )
    pending_onnx.replace(onnx_path)

    metadata = {
        "schema_version": 2,
        "format": "sts-combat-hrm-onnx",
        "numeric_precision": precision,
        "fixed_batch_size": fixed_batch_size,
        "checkpoint": str(checkpoint_path.resolve()),
        "checkpoint_sha256": file_sha256(checkpoint_path),
        "onnx": str(onnx_path.resolve()),
        "onnx_sha256": file_sha256(onnx_path),
        "vocabulary": vocabulary,
        "action_list": action_list,
        "model_defaults": checkpoint["model_defaults"],
        "split_map": {
            str(index): split for index, split in checkpoint["split_map"].items()
        },
    }
    pending_metadata = metadata_path.with_suffix(metadata_path.suffix + ".pending")
    pending_metadata.write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    pending_metadata.replace(metadata_path)
    print(f"exported ONNX policy: {onnx_path}", flush=True)
    print(f"exported Rust metadata: {metadata_path}", flush=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("checkpoint", type=Path)
    parser.add_argument("--onnx", type=Path)
    parser.add_argument("--metadata", type=Path)
    parser.add_argument(
        "--precision",
        choices=("float16", "float32"),
        default="float16",
        help="ONNX compute precision (default: float16)",
    )
    batching = parser.add_mutually_exclusive_group()
    batching.add_argument(
        "--fixed-batch-size",
        type=int,
        default=DEFAULT_RUNTIME_BATCH_SIZE,
        help=f"fixed runtime batch size (default: {DEFAULT_RUNTIME_BATCH_SIZE})",
    )
    batching.add_argument(
        "--dynamic-batch",
        action="store_const",
        const=None,
        dest="fixed_batch_size",
        help="export a tunable dynamic-batch graph instead",
    )
    args = parser.parse_args()
    if args.onnx is None:
        args.onnx = args.checkpoint.with_suffix(".onnx")
    if args.metadata is None:
        args.metadata = args.checkpoint.with_suffix(".runtime.json")
    if args.fixed_batch_size is not None and args.fixed_batch_size <= 0:
        parser.error("--fixed-batch-size must be positive")
    return args


if __name__ == "__main__":
    options = parse_args()
    export_checkpoint(
        options.checkpoint,
        options.onnx,
        options.metadata,
        options.precision,
        options.fixed_batch_size,
    )
