"""Compact action-value model for mean-floor self-play.

Unlike the historical multi-head stack, this model is trained from scratch and
has one policy contract: rank legal actions by expected final floor, with HP
carried into the final reached floor as dense continuation value.
"""

from __future__ import annotations

import math
from typing import Any

import torch
from torch import nn
from torch.nn import functional as F
from training_schema import (
    ACTION_PARAMETER_SPECS,
    FEATURE_BUCKETS,
    MEASUREMENT_SPECS,
)

LEGACY_TARGET_NAMES = ("progress_value", "final_floor", "entry_hp_fraction")
TARGET_NAMES = (
    "policy_logit",
    "progress_value",
    "final_floor",
    "entry_hp_fraction",
)


class SelectiveSsmMemory(nn.Module):
    """Compact selective diagonal state-space memory."""

    def __init__(self, hidden_size: int):
        super().__init__()
        self.norm = nn.RMSNorm(hidden_size)
        self.select = nn.Linear(hidden_size, hidden_size * 3, bias=False)
        self.log_decay = nn.Parameter(torch.linspace(-4.0, 0.0, hidden_size))
        self.output = nn.Linear(hidden_size, hidden_size, bias=False)

    def forward(self, embedded: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        selected = self.select(self.norm(embedded)).float()
        delta, candidate, gate = selected.chunk(3, dim=-1)
        rate = F.softplus(self.log_decay.float()).view(1, 1, -1)
        decay = torch.exp(-rate * (0.05 + 0.95 * torch.sigmoid(delta)))
        update = (1.0 - decay) * torch.tanh(candidate)
        visible = mask.unsqueeze(-1)
        decay = torch.where(visible, decay, torch.ones_like(decay))
        update = torch.where(visible, update, torch.zeros_like(update))
        state = torch.zeros_like(update[:, 0])
        for start in range(0, embedded.shape[1], 32):
            block_decay = decay[:, start : start + 32]
            block_update = update[:, start : start + 32]
            prefix = torch.cumprod(block_decay, dim=1).clamp_min(1e-20)
            block_state = prefix * (
                state.unsqueeze(1) + torch.cumsum(block_update / prefix, dim=1)
            )
            state = block_state[:, -1]
        positions = torch.arange(mask.shape[1], device=mask.device).unsqueeze(0)
        last_visible = positions.masked_fill(~mask, 0).max(dim=1).values
        last_gate = gate.gather(
            1,
            last_visible[:, None, None].expand(-1, 1, gate.shape[-1]),
        ).squeeze(1)
        return self.output((state * torch.sigmoid(last_gate)).to(embedded.dtype))


class ActionConditionedStateAttention(nn.Module):
    def __init__(self, hidden_size: int, heads: int = 4):
        super().__init__()
        if hidden_size % heads:
            raise ValueError("hidden size must be divisible by attention heads")
        self.heads = heads
        self.head_size = hidden_size // heads
        self.state_norm = nn.RMSNorm(hidden_size)
        self.action_norm = nn.RMSNorm(hidden_size)
        self.query = nn.Linear(hidden_size, hidden_size, bias=False)
        self.key = nn.Linear(hidden_size, hidden_size, bias=False)
        self.value = nn.Linear(hidden_size, hidden_size, bias=False)
        self.output = nn.Linear(hidden_size, hidden_size, bias=False)

    def forward(
        self,
        state_embedded: torch.Tensor,
        state_mask: torch.Tensor,
        action: torch.Tensor,
    ) -> torch.Tensor:
        batch, tokens, hidden = state_embedded.shape
        query = self.query(self.action_norm(action)).view(
            batch, self.heads, self.head_size
        )
        normalized = self.state_norm(state_embedded)
        key = self.key(normalized).view(batch, tokens, self.heads, self.head_size)
        value = self.value(normalized).view(batch, tokens, self.heads, self.head_size)
        scores = torch.einsum("bhd,bthd->bht", query, key) / math.sqrt(self.head_size)
        visible = state_mask.unsqueeze(1)
        weights = F.softmax(scores.masked_fill(~visible, -1e4).float(), dim=-1)
        weights = weights * visible
        weights = weights / weights.sum(-1, keepdim=True).clamp_min(1e-6)
        attended = torch.einsum("bht,bthd->bhd", weights.to(value.dtype), value)
        return self.output(attended.reshape(batch, hidden))


class CandidateInventoryMemory(nn.Module):
    """Permutation-invariant candidate join against every owned copy."""

    def __init__(self, hidden_size: int):
        super().__init__()
        self.norm = nn.RMSNorm(hidden_size)
        self.match_projection = nn.Sequential(nn.Linear(2, hidden_size), nn.SiLU())
        self.output = nn.Sequential(
            nn.RMSNorm(hidden_size * 5),
            nn.Linear(hidden_size * 5, hidden_size, bias=False),
        )

    def forward(
        self,
        inventory_ids: torch.Tensor,
        candidate_ids: torch.Tensor,
        embedding: nn.Embedding,
    ) -> torch.Tensor:
        batch = inventory_ids.shape[0]
        active = candidate_ids.ne(0).any(1)
        if not bool(active.any()):
            return embedding.weight.new_zeros((batch, embedding.embedding_dim))
        active_indices = active.nonzero(as_tuple=False).squeeze(1)
        inventory_ids = inventory_ids.index_select(0, active_indices)
        candidate_ids = candidate_ids.index_select(0, active_indices)
        inventory_mask = inventory_ids.ne(0)
        candidate_mask = candidate_ids.ne(0)
        inventory = embedding(inventory_ids)
        candidate = embedding(candidate_ids)
        normalized_inventory = F.normalize(self.norm(inventory).float(), dim=-1)
        normalized_candidate = F.normalize(self.norm(candidate).float(), dim=-1)
        visible = candidate_mask.unsqueeze(2) & inventory_mask.unsqueeze(1)
        scores = 8.0 * torch.einsum(
            "bch,bih->bci", normalized_candidate, normalized_inventory
        )
        weights = F.softmax(scores.masked_fill(~visible, -1e4), dim=-1)
        weights = weights * visible
        weights = weights / weights.sum(-1, keepdim=True).clamp_min(1e-6)
        attended_per_candidate = torch.einsum(
            "bci,bih->bch", weights.to(inventory.dtype), inventory
        )
        candidate_count = candidate_mask.sum(1, keepdim=True).clamp_min(1)
        inventory_count = inventory_mask.sum(1, keepdim=True).clamp_min(1)
        candidate_summary = (candidate * candidate_mask.unsqueeze(-1)).sum(
            1
        ) / candidate_count
        attended = (attended_per_candidate * candidate_mask.unsqueeze(-1)).sum(
            1
        ) / candidate_count
        inventory_summary = (inventory * inventory_mask.unsqueeze(-1)).sum(
            1
        ) / inventory_count
        exact_count = (
            (visible & candidate_ids.unsqueeze(2).eq(inventory_ids.unsqueeze(1)))
            .sum((1, 2))
            .float()
            .unsqueeze(1)
        )
        match = self.match_projection(
            torch.cat(
                (
                    torch.log1p(exact_count) / math.log(51.0),
                    exact_count / inventory_count.float(),
                ),
                dim=1,
            )
        ).to(candidate.dtype)
        relation = self.output(
            torch.cat(
                (
                    candidate_summary,
                    attended,
                    candidate_summary * attended,
                    inventory_summary,
                    match,
                ),
                dim=1,
            )
        )
        return relation.new_zeros((batch, relation.shape[-1])).index_copy(
            0, active_indices, relation
        )


class MeanProgressModel(nn.Module):
    """Shared-embedding relational critic with explicit floor and HP heads."""

    def __init__(self, config: dict[str, Any]):
        super().__init__()
        hidden = int(config.get("hidden_size", 96))
        self.numeric_size = int(
            config.get("numeric_measurements", len(MEASUREMENT_SPECS))
        )
        self.action_numeric_input_size = int(
            config.get("action_numeric_measurements", len(ACTION_PARAMETER_SPECS))
        )
        self.target_names = tuple(config.get("target_names", TARGET_NAMES))
        if self.target_names not in (LEGACY_TARGET_NAMES, TARGET_NAMES):
            raise ValueError(
                "mean-progress targets must be either "
                f"{LEGACY_TARGET_NAMES!r} or {TARGET_NAMES!r}"
            )

        self.embedding = nn.Embedding(FEATURE_BUCKETS + 1, hidden, padding_idx=0)
        self.state_projection = nn.Sequential(
            nn.RMSNorm(hidden), nn.Linear(hidden, hidden, bias=False)
        )
        self.action_projection = nn.Sequential(
            nn.RMSNorm(hidden), nn.Linear(hidden, hidden, bias=False)
        )
        self.inventory_projection = nn.Sequential(
            nn.RMSNorm(hidden), nn.Linear(hidden, hidden, bias=False)
        )
        self.candidate_projection = nn.Sequential(
            nn.RMSNorm(hidden), nn.Linear(hidden, hidden, bias=False)
        )
        self.numeric_projection = nn.Sequential(
            nn.RMSNorm(self.numeric_size),
            nn.Linear(self.numeric_size, hidden),
            nn.SiLU(),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.action_numeric_projection = nn.Sequential(
            nn.RMSNorm(self.action_numeric_input_size),
            nn.Linear(self.action_numeric_input_size, hidden),
            nn.SiLU(),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.state_memory = SelectiveSsmMemory(hidden)
        self.history_memory = SelectiveSsmMemory(hidden)
        self.state_attention = ActionConditionedStateAttention(hidden)
        self.inventory_relation = CandidateInventoryMemory(hidden)
        context_width = hidden * 11
        if self.target_names == LEGACY_TARGET_NAMES:
            self.output = nn.Sequential(
                nn.RMSNorm(context_width),
                nn.Linear(context_width, hidden * 3),
                nn.SiLU(),
                nn.Linear(hidden * 3, len(LEGACY_TARGET_NAMES)),
            )
        else:
            self.progress_output = nn.Sequential(
                nn.RMSNorm(context_width),
                nn.Linear(context_width, hidden * 3),
                nn.SiLU(),
                nn.Linear(hidden * 3, len(LEGACY_TARGET_NAMES)),
            )
            self.policy_output = nn.Sequential(
                nn.RMSNorm(context_width),
                nn.Linear(context_width, hidden * 3),
                nn.SiLU(),
                nn.Linear(hidden * 3, 1),
            )

        self.policy_supported_targets = frozenset(TARGET_NAMES)

    @staticmethod
    def pool(embedded: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        return (embedded * mask.unsqueeze(-1)).sum(1) / mask.sum(
            1, keepdim=True
        ).clamp_min(1)

    def forward(
        self,
        state_ids: torch.Tensor,
        action_ids: torch.Tensor,
        numeric: torch.Tensor,
        history_ids: torch.Tensor,
        inventory_ids: torch.Tensor,
        candidate_identity_ids: torch.Tensor,
        action_numeric: torch.Tensor,
    ) -> torch.Tensor:
        state_ids = state_ids.long()
        action_ids = action_ids.long()
        history_ids = history_ids.long()
        inventory_ids = inventory_ids.long()
        candidate_identity_ids = candidate_identity_ids.long()

        state_mask = state_ids.ne(0)
        action_mask = action_ids.ne(0)
        history_mask = history_ids.ne(0)
        inventory_mask = inventory_ids.ne(0)
        candidate_mask = candidate_identity_ids.ne(0)
        state_tokens = self.embedding(state_ids)
        action_tokens = self.embedding(action_ids)
        history_tokens = self.embedding(history_ids)
        inventory_tokens = self.embedding(inventory_ids)
        candidate_tokens = self.embedding(candidate_identity_ids)

        state = self.state_projection(self.pool(state_tokens, state_mask))
        action = self.action_projection(self.pool(action_tokens, action_mask))
        inventory = self.inventory_projection(
            self.pool(inventory_tokens, inventory_mask)
        )
        candidate = self.candidate_projection(
            self.pool(candidate_tokens, candidate_mask)
        )
        state_memory = self.state_memory(state_tokens, state_mask)
        history_memory = self.history_memory(history_tokens, history_mask)
        attention = self.state_attention(state_tokens, state_mask, action)
        relation = self.inventory_relation(
            inventory_ids, candidate_identity_ids, self.embedding
        )
        numeric_context = self.numeric_projection(numeric[:, : self.numeric_size])
        action_numeric_context = self.action_numeric_projection(
            action_numeric[:, : self.action_numeric_input_size]
        )
        context = torch.cat(
            (
                state,
                action,
                state * action,
                state_memory,
                history_memory,
                attention,
                inventory,
                candidate,
                relation,
                numeric_context,
                action_numeric_context,
            ),
            dim=1,
        )
        if self.target_names == LEGACY_TARGET_NAMES:
            return self.output(context)
        return torch.cat(
            (self.policy_output(context.detach()), self.progress_output(context)), dim=1
        )
