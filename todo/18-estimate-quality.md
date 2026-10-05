# 18. Estimate quality and user guidance

**Depends on:** 0, 6, 9; complete before 17

## Todo

- [ ] Assemble a small, permissioned reference set of cattle photos with contemporaneous scale weights and tape measurements; record breed, age/size, pose, and image conditions where known.
- [ ] Keep calibration examples separate from evaluation examples; compare photo and tape estimates against scale weights using absolute and percentage error, bias, and worst-case errors.
- [ ] Define supported use cases and an acceptance threshold before evaluation; document sample size and limitations rather than claiming broad accuracy from a small set.
- [ ] Check repeatability for the same image and model, implausible values, non-cattle images, poor poses, and disagreement between photo and tape estimates.
- [x] Present weight ranges as estimates; explain how ranges are derived and avoid implying statistically calibrated confidence unless evaluation supports it. (±10% photo / ±5% tape derivation kept in UI, API, CSV, and eval report notes.)
- [x] Record provider/model, prompt, and estimator versions with results; repeat the reference evaluation before approving model or prompt changes and retain a rollback choice. (History rows + eval reports stamp all four; process + rollback in eval/README.md.)
- [x] Show concise photo/tape instructions and explain when to repeat a measurement or use a scale; keep veterinary and dosing warnings visible in results and exports.
- [x] Define how either user reports a questionable result without exposing their photo or private record to the other account. (eval/README.md reporting path; per-account isolation enforced server-side.)

## Completion check

Evaluation evidence, acceptance criteria, supported use cases, and limitations are recorded. Both users can recognize placeholders and unreliable results, and model changes have a repeatable quality check.

## Implementation status (2026-10-05)

Framework, schema, fixtures format, validation, metrics, repeatability, reports, docs, and version stamps are implemented and CI-guarded on synthetic rows. BLOCKED on real data: assembling a permissioned cattle/scale dataset, agreeing acceptance thresholds, and running the scored evaluation cannot be fabricated locally. Do NOT check the remaining boxes (dataset, scored comparison, agreed thresholds, real-data repeatability review) until that evidence exists. Synthetic fixture numbers must never be presented as evaluation results.
