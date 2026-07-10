## Node stdlib legacy → real benchmark delta

Samples: 5 measured + 1 warmup, same sidecar and host.

| row | legacy p50 (ms) | real p50 (ms) | real/legacy | real guest RSS |
| --- | ---: | ---: | ---: | ---: |
| fs/stat_storm | 0.29 | 0.28 | 0.9655 | 47382528 |
| fs/fs_read_small | 0.53 | 0.4 | 0.7547 | 29331456 |
| fs/fs_read_big | 21.19 | 20.59 | 0.9717 | 37793792 |
| fs/readdir_big | 1.47 | 1.42 | 0.966 | 30060544 |
| fs/stream_copy_big | 33.16 | 33.47 | 1.0093 | 34639872 |
| modules/require_100_small | 46.5 | 43.54 | 0.9363 | 30425088 |
| modules/import_npm_package | 174.49 | 539.53 | 3.092 | 81809408 |
| pipes/pass_through_big | 0.18 | 0.2 | 1.1111 | 29294592 |
| control/cpu_loop | 7.79 | 7.92 | 1.0167 | 29429760 |

Native Node codec floor:

- utf8EncodeDecode: p50 0.34ms, p99 0.8231ms, IQR 0.0428ms (9 samples)
- base64EncodeDecode: p50 0.2664ms, p99 0.6515ms, IQR 0.0208ms (9 samples)
