# 18. Estimate quality and user guidance

**Depends on:** 0, 6, 9; complete before 17

## Todo

- [ ] Assemble a small, permissioned reference set of cattle photos with contemporaneous scale weights and tape measurements; record breed, age/size, pose, and image conditions where known.
- [ ] Keep calibration examples separate from evaluation examples; compare photo and tape estimates against scale weights using absolute and percentage error, bias, and worst-case errors.
- [ ] Define supported use cases and an acceptance threshold before evaluation; document sample size and limitations rather than claiming broad accuracy from a small set.
- [ ] Check repeatability for the same image and model, implausible values, non-cattle images, poor poses, and disagreement between photo and tape estimates.
- [ ] Present weight ranges as estimates; explain how ranges are derived and avoid implying statistically calibrated confidence unless evaluation supports it.
- [ ] Record provider/model, prompt, and estimator versions with results; repeat the reference evaluation before approving model or prompt changes and retain a rollback choice.
- [ ] Show concise photo/tape instructions and explain when to repeat a measurement or use a scale; keep veterinary and dosing warnings visible in results and exports.
- [ ] Define how either user reports a questionable result without exposing their photo or private record to the other account.

## Completion check

Evaluation evidence, acceptance criteria, supported use cases, and limitations are recorded. Both users can recognize placeholders and unreliable results, and model changes have a repeatable quality check.
