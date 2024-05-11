# Rakurima

This is a Rust based implementation of distributed system challenges conducted using the [Maelstrom](https://github.com/jepsen-io/maelstrom/tree/main) workbench.

## Setup
* [JDK 11+](https://docs.aws.amazon.com/corretto/latest/corretto-17-ug/downloads-list.html), runtime for Clojure (language for Maelstrom)
* Install other dependencies (`apt install graphviz gnuplot`)
* [Rust](https://www.rust-lang.org/tools/install)
* Download (included in the repository root as `maelstrom.tar.bz2`) the maelstrom release and extract it with `tar xvf maelstrom.tar.bz2`
* Test maelstrom setup by running a demo in the `maelstrom/demo` directory

Reference the [prerequisite page](https://github.com/jepsen-io/maelstrom/blob/main/doc/01-getting-ready/index.md#prerequisites) on Maelstrom for more details.

## Testing

## References
* [Challenge specifications with Go scaffolding](https://fly.io/dist-sys/1/)
* [Maelstrom protocol specifications](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md)
* [Jon's implementation on YouTube](https://www.youtube.com/watch?v=gboGyccRVXI)
