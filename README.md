# Rakurima

This is a Rust based implementation of distributed system challenges conducted using the [Maelstrom](https://github.com/jepsen-io/maelstrom/tree/main) workbench.

## Setup
* [JDK 11+](https://docs.aws.amazon.com/corretto/latest/corretto-17-ug/downloads-list.html), runtime for Clojure (language for Maelstrom)
* Install other dependencies (`apt install graphviz gnuplot`)
* [Rust](https://www.rust-lang.org/tools/install)
* Run `./bootstrap.sh` to extract the Maelstrom library and compile the test program.

Reference the [prerequisite page](https://github.com/jepsen-io/maelstrom/blob/main/doc/01-getting-ready/index.md#prerequisites) on Maelstrom for more details.

## Testing
*Make sure to review the setup steps and complete them correctly before proceeding.*

## Design

## References
* [Maelstrom protocol specifications](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md)
* [Challenge specifications](https://fly.io/dist-sys/1/)
* [Jon's implementation on YouTube](https://www.youtube.com/watch?v=gboGyccRVXI)
