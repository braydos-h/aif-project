# Estimate-quality evaluation

Repeatable measurement of photo and tape estimates against verified scale
weights (roadmap item 18, a launch gate before item 17).

## Status: real-data acceptance BLOCKED

No permissioned cattle reference dataset exists in this repository yet.
`fixtures/example.json` is **synthetic format documentation only** — its
numbers were invented to exercise the tooling and must never be presented
as evaluation evidence. The acceptance gate stays blocked until real,
permissioned data is evaluated.

## Running an evaluation

```sh
cargo run --manifest-path backend/Cargo.toml --bin aif-eval -- \
  --dataset eval/fixtures/example.json --out /tmp/opencode/eval-report.json
```

With `AIF_AI_BACKEND=ollama` (plus `OLLAMA_API_KEY`) photo rows run the
real provider; with `AIF_AI_BACKEND=none` photo rows are deterministic
placeholders and the report says so (`photo_placeholder_rows`,
`provider: "none"`). Tape rows always run offline via Schaeffer's formula.

Options: `--repeat N` (1–5, default 2) reruns each photo row to measure
repeatability (`repeatability_max_diff_kg`).

## Dataset schema

```json
{
  "name": "herd-october-2026",
  "version": "2026-10-20-v1",
  "acceptance_mape_pct": 15.0,
  "acceptance_bias_pct": 5.0,
  "rows": [
    {
      "id": "cow-001",
      "scale_weight_kg": 512.5,
      "heart_girth_cm": 185.0,
      "body_length_cm": 152.0,
      "image": "cow-001.jpg",
      "split": "evaluation",
      "synthetic": false,
      "breed": "Angus",
      "notes": "side view, daylight, standing square"
    }
  ]
}
```

- `scale_weight_kg`: contemporaneous verified scale weight (required).
- `image`: inline base64/data-URI, or a path relative to the dataset file.
- `split`: `calibration` (prompt tuning) or `evaluation` (scoring).
  Calibration rows never score.
- `synthetic`: must be `false` for acceptance data.

Validation rejects non-finite/out-of-range scale weights, tape values
outside 50–300 cm, unknown splits, and datasets with no evaluation rows.

## Report

`tape_vs_scale` and `photo_vs_scale` carry `n`, `mae_kg`, `mape_pct`,
`bias_kg`, `bias_pct`, worst-case error plus the responsible row id,
`within_10_pct` / `within_20_pct` fractions, and `implausible` counts.
Every report stamps `provider`, `model`, `prompt_version`, and
`estimator_version`. Exit status is `3` when the dataset's acceptance
thresholds fail, `0` on pass, `2` on CLI misuse, `1` on runtime failure.

## Process (required before model/prompt changes)

1. Assemble the permissioned set: contemporaneous scale weights plus side
   photos and tape measures; record breed/age/pose/lighting where known.
2. Keep calibration and evaluation rows split in the dataset file.
3. Run `aif-eval`, archive the JSON report with the dataset version.
4. Approve the change only if the gate passes **and** the operator judges
   the sample adequate; record the decision, sample size, and limitations.
5. Keep the previous model/prompt available for rollback.

Weight ranges stay labeled as estimates. Never claim statistically
calibrated confidence unless the data supports it.

## Reporting a questionable result

Either user can flag a suspicious estimate from the Account view (note
the request id from history detail). Reports are handled per account —
never share one user's photo or record with the other account when
investigating.
