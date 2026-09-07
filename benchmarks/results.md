# Stacked detector evaluation

Fixed synthetic corpus: eight positive pairs (five development, three holdout) and four negative controls. Every layer uses the same input fingerprint. Labels were frozen before implementation; a missing React import in one fixture was corrected for all reruns.

| Layer | Positive coverage | Additional positives |
|---|---:|---:|
| Baseline names + clones | 1/8 | — |
| Broader extraction | 4/8 | 3 |
| Normalized bodies | 6/8 | 2 |
| Statement blocks | 8/8 | 2 |
| Grouped audit | 8/8 | 0 |

Body evidence matches five positive controls and no negatives; block evidence emits 15 positive fragments and no negatives. Names still flags all four negative controls. The combined audit ranks all eight positive labels before them. This is a small regression corpus, not a real-world recall or precision estimate.

A frozen production corpus, with fixed exclusions, yielded 2,606 baseline name pairs and 365 clone fragments. Extraction increased name pairs to 4,837. Bodies added 264 candidates (131 without prior overlapping evidence); blocks emitted 557 fragments (319 without prior containing-function evidence). Manual samples confirmed three new body cleanup opportunities and five block opportunities; these are conservative subsets, not all candidates.

Grouping adds no detector. It compresses 1,186 structural fragments into 363 structural review groups (5,055 total groups including name-only evidence). The combined run took about 62 seconds versus about 118 seconds for separate engines; concurrent machine activity means these are observations, not controlled performance benchmarks. Detailed source locations and reports remain local because the production corpus is private.

Raw location/range pair deltas are not unique cleanup counts. IDF changes can remove as well as add name candidates; overlapping blocks can describe one task; grouped output uses representative links and enclosing-function coordinates. Inspect both sides before extracting code.
