# Rakurima System Deep Dive

At a high level, like many distributed systems, Rakurima appears to be an singular monolithic server/API to the clients. It accepts client requests and will respond once the processing completes, however long it may take. However, there might be intermittent failures if a node is down right before it sends the response, or if the network between the server and client is unstable. In those scenarios, it still relies on clients to retry requests to maintain its guarantees. Note that the server will NOT retry the sending of client responses; in other words, responses are fire-and-forget.

## Modes of Operation
Rakurima has 2 modes of operation: single-node and cluster. This is entirely determined by the start sequence. Maelstrom issues an [initialization message](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md#initialization) to each node to boot it up. It contains information on the number of peers in the service; if there is only a single node in the service, Rakurima will function in single-node mode without maintaining a replication state machine; if more than 1 node is in service, Rakurima starts in cluster mode and runs the Raft state machine (explained more below) to replicate information if needed. The number of nodes should be an odd number. The maximum number of nodes supported is 127.

## Server Workflow
When a node starts, it blocks to wait for the initialization message from Maelstrom. Once the init is received, the main server process is initialized and the workflow loop will commence. The workflow loop is administered by a single thread that handles everything related to the state machine as well as performing bookkeeping on requests that cannot immediately finish. The program also uses 2 other threads to control STDIN and STDOUT, and we use channels to pass messages to other threads to process incoming request or communicate with other nodes and clients.

The reason for using a single-threaded worker is due to the fact that every outgoing message has to be written to STDOUT in Maelstrom's simulated network. There is no "real" way to do parallel communication as in a real server networking environment. To keep the system non-blocking, the worker keeps track of a "queue" of tasks that are pending so that it may action on any of them when time is right. Tasks can include incomplete broadcast requests, pending replication requests, and others that are time-consuming.

The worker thread briefly sleeps in between its working cycles, when it wakes up, it does the following:
1. Receives new messages from the STDIN thread and action upon them. Items that are not completed immediately will be queued.
2. Housekeeping for the internal state machine:
   1. If this node is leader, send out heartbeats to other nodes if the configured interval has elapsed.
   2. If not leader, check election timeout to determine if a new election is needed.
3. Go through the work queue and action upon tasks that are ready to make progress.

All outgoing communications are sent via channel to the STDOUT thread to be printed in the order that messages are received. The STDIN thread may directly send messages to STDOUT for simple requests such as echo, bypassing the workflow loop.

## Rakurima State Machine Specifications
The internal state machine for Rakurima is roughly based on the [Raft paper](https://raft.github.io/raft.pdf) for leader election and log replication, and it has the same guarantees in principle. Due to the nature of being inside a Maelstrom cluster, a few adjustments/simplifications have been made. Notably:
* Membership change is not supported. Nodes can be shutdown and brought back up, but no new nodes can be added. In other words, every server only agrees to the configuration given by the init message.
* The first node (`n1`) will be the de-facto leader upon startup. Elections are only held thereafter should `n1` becomes unavailable or sufficiently isolated.
* All writes are handled by the leader only, but reads are served locally from each node's internal state. In other words, we prefer better availability and balanced load over stronger consistency.
* Log compaction and snapshot transfer are not supported. This is largely due to no support for membership change. Every node participates from the start so they are expected to catch up via log replication and execution alone.

## Supported Request/Workload and Implementation
Rakurima supports a large subset of the Maelstrom workloads. And it should easily be able to extend to support additional workloads.

### Echo
The most basic workload, can be used to test setup and as a shallow health check. Each node receiving the echo request responds immediately to the client without any other cross communication.

### Unique ID
This workload requests unique IDs from the server. For simplicity, we don't rely on the internal state machine to build consensus. Instead, each node generates IDs independently using a similar approach to the [Snowflake](https://blog.x.com/engineering/en_us/a/2010/announcing-snowflake) algorithm. The generated ID is a 64-bit string (Hex encoded). In this case, since there are only nodes without data centers, we use 47 bits for timestamp (Epoch milliseconds) + 7 bits for the node ID, and 10 bits for a sequence number stored on each node that increments. The sequence ID wraps once reached maximum. This means we can support over 1 million IDs per node per second on average without creating a conflict.

### Message Broadcast
The workload that announces messages to the server. In cluster mode, the messages need to propagate to every node with eventual-consistency. Because message propagation is idempotent in this scenario (i.e. we persist a set of integers on each server) without side effects, the implementation does not depend on the internal state machine. A simple gossip strategy is used to deliver the broadcast where every node is connected to a few peers.

The way that the peers are chosen is deterministic (Topology messages are ignored). The number of peers will be a fraction of the total number of nodes in the cluster so that it is redundant enough to tolerate a small amount of communication failures. In addition, broadcast will be retried a few times until a response is heard from the peer. A node will only forward messages not already seen to prevent a loop. A node will respond to a peer's broadcast whether it already has the message or not to signal a retry is not needed.

### PN Counters
A workload that increments and decrements an integer value stored on the server. To guarantee correctness, we must enforce once and only-once execution of the update commands. This process will ride on the back of the internal state machine. Each increment/decrement command will be treated as an execution log to be replicated to other server nodes, and they are executed once a consensus is achieved.

### Kafka
A workload that maintains a replicated log stream. This will also rely on the internal state machine. Despite of their similarity, the client-facing Kafka log and commit history are separate from the internal replicated command log so that the server can handle different requests at the same time and maintain an arbitrary number of states.

### Transaction RW Register
A workload that simulates a transaction-based, distributed key-value store. The implementation is still based on executing requested commands on the internal state machine.
