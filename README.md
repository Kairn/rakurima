# Rakurima

This is a Rust based implementation of solving distributed system challenges conducted using the [Maelstrom](https://github.com/jepsen-io/maelstrom/tree/main) workbench.

## Setup
* Make sure to have [JDK 11+](https://docs.aws.amazon.com/corretto/latest/corretto-17-ug/downloads-list.html) on the system, runtime for Clojure (language for Maelstrom).
* Install other dependencies (`apt install graphviz gnuplot`).
* Get [Rust](https://www.rust-lang.org/tools/install).
* Run `./bootstrap.sh` to extract the Maelstrom binary (included) and compile the test program.

Reference the [prerequisite page](https://github.com/jepsen-io/maelstrom/blob/main/doc/01-getting-ready/index.md#prerequisites) on Maelstrom for more details.

## Testing
*Make sure to review the setup steps and complete them correctly before proceeding.*

Use the following command template to run a Maelstrom workload against the server in the repository root.
```
./maelstrom/maelstrom test -w <WORKLOAD> --bin ./rakurima/target/debug/server --node-count <NODE_COUNT> --time-limit <TIME_LIMIT> [OPTIONS]
```

To debug, use the following command after a run, then open http://localhost:8080/.
```
./maelstrom/maelstrom serve
```
Server logs (printed to stderr) can be found under `./store/latest/node-logs`.

The following environment variables can be supplied to the test program.
* `BASE_PAUSE_TIME_MS` - The (base) sleep time of the Rakurima worker thread in milliseconds, default to 10.
* `BASE_BROADCAST_RETRY_MS` - The (base) timeout in milliseconds before a broadcast is retried, default to 200.
* `BASE_ELECTION_TIMEOUT_MS` - The (base) timeout in milliseconds before a raft node holds a new election without receiving replication call, default to 3000.
* `BASE_REPLICATE_INTERVAL_MS` - The time in milliseconds between replication RPCs for leader, default to 150. No jitter is applied currently.
* `RAFT_REQUEST_RETRY_INTERVAL_MS` - The time in milliseconds between Raft request retries, default to 125.
* `RAFT_REQUEST_TTL_MS` - The time in milliseconds for how long can a queued Raft request live before being dropped, default to 6000.

The following workloads are currently supported. (More in progress)

### Echo
```
./maelstrom/maelstrom test -w echo --bin ./rakurima/target/debug/server --node-count 1 --time-limit 10
```

### Broadcast (simple)
```
./maelstrom/maelstrom test -w broadcast --bin ./rakurima/target/debug/server --node-count 1 --time-limit 10 --rate 10
```

### Broadcast (complex)
```
./maelstrom/maelstrom test -w broadcast --bin ./rakurima/target/debug/server --node-count 25 --time-limit 30 --rate 100 --latency 100 --nemesis partition
```

### PN Counter (simple)
```
./maelstrom/maelstrom test -w pn-counter --bin ./rakurima/target/debug/server --node-count 1 --time-limit 10
```

### PN Counter (complex)
```
./maelstrom/maelstrom test -w pn-counter --bin ./rakurima/target/debug/server --node-count 3 --rate 500 --time-limit 70 --latency 20 --nemesis partition --nemesis-interval 20
```

### Kafka
```
./maelstrom/maelstrom test -w kafka --bin ./rakurima/target/debug/server --node-count 3 --concurrency 2n --time-limit 70 --rate 500 --latency 20 --nemesis partition --nemesis-interval 20
```
**Note:** Currently unstable under network partition. Removing `--nemesis partition` will produce stable results.

### Transaction RW Register
```
./maelstrom/maelstrom test -w txn-rw-register --bin ./rakurima/target/debug/server --node-count 3 --concurrency 2n --time-limit 70 --rate 1000 --consistency-models read-committed --availability total --nemesis partition --nemesis-interval 20
```

## Design
Rakurima is designed to be an all-encompassing and non-blocking server that handles Maelstrom workloads with partition tolerance.
* **All-encompassing** - There is only a single server binary that contains logic to process different types of requests, even concurrently without being incorrect.
* **Non-blocking** - A "slow" request will not block the servicing of other requests. For example, while the server is waiting for the replication of an update request, it can still respond to new echo requests with minimal delay.
* **Partition tolerance** - It implements an adapted version of the [Raft](https://raft.github.io/raft.pdf) consensus algorithm for distributed log replication to ensure that the system can function correctly as long as a majority of nodes are up and can communicate with each other.

More detailed design specifications can be found in the [doc](https://github.com/Kairn/rakurima/tree/master/doc) subdirectory.

## References
* [Maelstrom protocol specifications](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md)
* [Official challenges](https://github.com/jepsen-io/maelstrom/tree/main/doc)
* [Fly.io notes](https://fly.io/dist-sys/1/)
* [Jon's implementation on YouTube](https://www.youtube.com/watch?v=gboGyccRVXI)
