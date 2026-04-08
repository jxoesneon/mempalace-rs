# Repo Scan Pre-Scan Report

- **Target**: `/Users/meilynlopezcubero/mempalace-rs`
- **Scan Time**: 2026-04-08 07:48:36

## 1. Overall Statistics

| Metric                        | Value         |
| ----------------------------- | ------------- |
| Total Files                   | 34640         |
| Total Size (raw)              | 9.42 GB       |
| **Project Source Files**      | **84**        |
| **Project Source Size**       | **351.14 MB** |
| Third-Party Files             | 0             |
| Third-Party Size              | 0 B           |
| Noise Files (build artifacts) | 34556         |
| Noise Size (build artifacts)  | 9.07 GB       |
| Project Code Ratio            | 3.6%          |

## 2. Top-Level Directory Breakdown

| Directory    | Project Files | Project Size | Total Size | Build Systems | Notes          |
| ------------ | ------------- | ------------ | ---------- | ------------- | -------------- |
| `assets`     | 2             | 3.15 MB      | 3.15 MB    | -             |                |
| `benches`    | 1             | 1.67 KB      | 1.67 KB    | -             |                |
| `benchmarks` | 1             | 3.85 KB      | 3.85 KB    | -             |                |
| `examples`   | 1             | 3.32 KB      | 3.32 KB    | -             |                |
| `hooks`      | 3             | 8.25 KB      | 8.25 KB    | -             |                |
| `models`     | 16            | 173.76 MB    | 173.76 MB  | -             |                |
| `scripts`    | 1             | 2.88 KB      | 2.88 KB    | -             |                |
| `src`        | 21            | 278.35 KB    | 278.74 KB  | -             |                |
| `target`     | 0             | 0 B          | 9.07 GB    | -             | build artifact |
| `tests`      | 7             | 17.42 KB     | 17.42 KB   | -             |                |

## 3. Source File Statistics by Tech Stack (project files only)

| Tech Stack     | File Count | Total Size |
| -------------- | ---------- | ---------- |
| C/C++          | 0          | 0 B        |
| Java/Android   | 0          | 0 B        |
| iOS (OC/Swift) | 0          | 0 B        |
| C#/.NET        | 0          | 0 B        |
| Web/JS/TS      | 0          | 0 B        |
| CSS/Style      | 0          | 0 B        |

## 4. Third-Party Dependencies Detected

| Library | Version | Locations                        | Files |    Size |
| ------- | ------- | -------------------------------- | ----: | ------: |
| fmt     | unknown | `target/doc/trait.impl/core/fmt` |     1 | 5.02 KB |

**Third-party container directories** (may contain multiple libraries):

- `target/debug/deps/` (13852 files, 4.47 GB)
- `target/package/mempalace-rs-0.1.0/target/debug/deps/` (2807 files, 1.72 GB)
- `target/release/deps/` (1929 files, 1.55 GB)

## 5. Suspected Code Duplication (directories appearing 3+ times)

No significant directory-level duplication detected.

## 6. Directory Tree (noise filtered, third-party marked)

```text
mempalace-rs/
├── .fastembed_cache/
│   └── models--Qdrant--all-MiniLM-L6-v2-onnx/
│       ├── blobs/
│       ├── refs/
│       └── snapshots/
├── .github/
│   └── workflows/
├── assets/
├── benches/
├── benchmarks/
├── examples/
├── hooks/
├── models/
│   └── models--Qdrant--all-MiniLM-L6-v2-onnx/
│       ├── blobs/
│       ├── refs/
│       └── snapshots/
├── scripts/
├── src/
└── tests/
```

## 7. Git Repositories & Activity

Found **1** git repositories.

| Repository | Total Commits | Recent (1yr) | Last Commit |
| ---------- | ------------- | ------------ | ----------- |
| `(root)`   | 6             | 6            | 2026-04-08  |

## 8. Noise Directory Summary

| Type      | Occurrences (files) | Total Size |
| --------- | ------------------: | ---------- |
| `target/` |               40141 | 11.08 GB   |
| `build/`  |                1652 | 385.96 MB  |
| `.git/`   |                 112 | 3.34 MB    |
| `bin/`    |                   2 | 798 B      |
