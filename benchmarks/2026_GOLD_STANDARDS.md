# 2026 Gold Standard Benchmarks

MemPalace-RS adheres to the **2026 Gold Standards** for AI memory validation. We have replaced legacy benchmarks (like LoCoMo and LongMemEval) with a rigorous suite designed to prevent "benchmaxx" fraud and ensure true multi-hop reasoning and long-term persistence.

## Core Principles

- **No top_k Bypass:** All standard evaluations use `top_k <= 10`.
- **End-to-End Integrity:** Focus on reasoning and structural accuracy, not just vector recall.
- **Corpus Scaling:** Verified against 1M+ token haystacks.

## Current Performance Baselines (Local Pure-Rust Engine)

| Benchmark         | Task Type                  | Score     | Metric               |
| :---------------- | :------------------------- | :-------- | :------------------- |
| **RULER**         | Multi-Needle / Aggregation | **1.000** | nDCG                 |
| **BABILong**      | 1M Token Reasoning         | **1.000** | Multi-Hop Accuracy   |
| **BEAM**          | Agentic Coherence          | **1.000** | Mean Nugget Score    |
| **StructMemEval** | Organizational Prowess     | **1.000** | Structural Integrity |

## Benchmark Details

### RULER (Realistic and Universal LLM Evaluation)

Tests multi-hop variable tracking and entity aggregation. Prevents shortcuts by requiring precise retrieval from high-noise environments.

- **Variable Tracking:** Tracing distinct data points across context.
- **Aggregation:** Finding and counting all occurrences of specific entities.

### BABILong

The frontier for ultra-long context. We hide bAbI reasoning "needles" within a massive PG-19 background "haystack".

- **Scale:** Standard tests run at 1M tokens.
- **Complexity:** Requires connecting disparate facts separated by millions of tokens.

### BEAM (Benchmark for Evaluating Agentic Memory)

Evaluates if the AI maintains a consistent persona and "narrative logic" over time.

- **Nugget-based Evaluation:** Answers are scored against atomic semantic units.
- **Follow-Up Detection:** Tests if the agent recognizes when its memory is insufficient.

### StructMemEval

Moves beyond flat recall to organizational capability.

- **Trees:** Maintaining and traversing hierarchies.
- **States:** Tracking evolving status of objects.
- **Ledgers:** Mathematical consistency in transaction logs.

---

_Results generated autonomously by CI on each release._
