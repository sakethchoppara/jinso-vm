# v3 baseline

Measured at `09d1d94` (end of v2), before any v3 change, with `./scripts/bench.sh 10`
on an AMD Ryzen 7 5800HS, Linux. The optimizer's goal is a reduction in the last column,
which is stable across runs and builds; wall-clock time moves about ±5% from code layout.

| Benchmark | lumen | CPython 3.13.5 | lumen vs CPython | lumen instructions |
|---|---|---|---|---|
| fib | 0.111s | 0.116s | 1.05x faster | 1135M |
| loop | 0.479s | 0.644s | 1.34x faster | 5012M |
| strings | 0.061s | 0.072s | 1.18x faster | 705M |
