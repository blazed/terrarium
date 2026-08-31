# Baseline

Multi-seed numbers from the local engine, measured with the `baseline` example.
Content milestones diff their own runs against this table.

## Command

```text
cargo run --example baseline -- --seeds 10 --days 7
```

Run on 2026-09-02 (release of the baseline example), wall time 12.3s.

## Aggregate (mean | min | max)

```text
Metric                            Mean       Min       Max
Events                          5364.0      5129      5767
Deaths                             0.0         0         0
Unique conversation pairs         27.8        26        28
Talks                            563.8       406       664
Confrontations                     1.2         0         2
Aid given                         10.5         6        16
Rumor max depth                    2.4         2         3
Purchases                        103.9        99       111
Insolvent employers                0.1         0         1
Goals completed                 1232.9      1134      1381
Rejected actions                  10.2         5        20
Waited share %                    7.85      4.41      9.93
Resident balance                  93.5         0       274
Resident health                    1.0         1         1
Resident mood                      1.0         1         1
Resident relationship mean         0.8         0         2
Resident memories                 20.0        20        20
Resident rumors carried           15.5         0        20
```

Counts are per-seed scalars; resident-level metrics are pooled over all
residents of all seeds. LLM metrics are omitted (local engine only).