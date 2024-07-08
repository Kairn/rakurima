# Rakurima System Deep Dive

At a high level, like many distributed systems, Rakurima appears to be a singular monolithic server/API to the clients. It accepts client requests and will respond once the processing completes, however long it may take. However, there might be intermittent failures if a node is down right before it sends the response, or if the network between the server and client is unstable. In those scenarios, it still relies on the clients to retry requests to maintain its guarantees. Note that the server will NOT retry the sending of client responses; in other words, responses are fire-and-forget.

## Modes of Operation
Rakurima has 2 modes of operation: single-node and cluster. This is entirely determined by the startup sequence. Maelstrom issues an [initialization message](https://github.com/jepsen-io/maelstrom/blob/main/doc/protocol.md#initialization) to each node to boot it up. It contains information on the number of peers in the cluster; if there is only a single node, Rakurima will function in single-node mode without data replication and consensus; if more than 1 node is in service, Rakurima starts in cluster mode and runs the Raft state machine (explained more below) to replicate information and requires consensus from majority to commit and apply changes. The number of nodes should be an odd number.

## Server Workflow
When a node starts, it blocks to wait for the initialization message from Maelstrom. Once the init is received, the main server process is initialized and the workflow loop will commence. The workflow loop is administered by a single thread that handles everything related to the state machine as well as performing bookkeeping on requests that cannot immediately finish. The program also uses 2 other threads to control STDIN and STDOUT, and we use channels to pass messages to other threads to process incoming request or communicate with other nodes and clients.

The reason for using a single-threaded worker is due to the fact that every outgoing message has to be written to STDOUT in Maelstrom's simulated network. There is no "real" way to do parallel communication as in a real server networking environment. To keep the system non-blocking, the worker stores "tasks" in its internal memory to keep track of requests that are time consuming, and it schedules event loops to action on pending tasks when time is right. Tasks can contain broadcast requests or voting requests (for leader election), for example.

The worker thread briefly sleeps (configurable interval) in between event loops, when it wakes up, it does the following:
1. Receives new messages from the STDIN thread and action upon them. Items that are not completable in a stateless way will be sent to the internal state machine.
2. Process incoming events in the state machine:
   1. Followers receive and replicate logs sent from leader.
   2. All nodes will vote for candidates that are up to date upon request when leader is isolated for some time, on a first come first serve basis.
   3. All nodes will update its state and role based on responses from other nodes. For example, a leader can convert to a follower if a new election has been held while it is being isolated.
3. Housekeeping for the state machine:
   1. For leader, send out replication RPCs (functions like heartbeat too) to other nodes if the configured interval has elapsed.
   2. For leader, commit entries that have been replicated to the majority of servers.
   3. If not leader, check election timeout to determine if a new election is needed.
   4. All nodes will apply committed entries; leader will respond to client after application.
4. Go through other tasks (broadcast only as of now) and action upon those that are ready to make progress.

All outgoing communications are sent via channel to the STDOUT thread to be printed in the order that messages are received. The STDIN thread may directly send messages to STDOUT for simple requests such as echo, bypassing the state machine event loop.

## Rakurima State Machine Specifications
The internal state machine for Rakurima is "roughly" based on the [Raft paper](https://raft.github.io/raft.pdf), and it has the same guarantees in principle. Due to the nature of being inside a Maelstrom cluster, a few adjustments/simplifications have been made. Notably:
* Membership change is not supported. Nodes can be shutdown and brought back up, but no new nodes can be added. In other words, every server only agrees to the configuration given by the init message.
* The first node (`n0`) will be the de-facto leader upon startup. Elections are only held thereafter should `n0` becomes unavailable or sufficiently isolated. If `n0` crashed and came back later, it will quickly recognize the new leader and abandon its presumed authority.
* All writes are handled by the leader node (followers will forward messages on behalf of the client), but reads are served locally from each node's internal state. In other words, we prefer better availability and balanced load over stronger consistency.
* Heartbeat and log replication is combined into the same `AppendEntries` RPC that is sent out on a schedule. New client updates will be instantly scheduled for better convergence time, but internal log realignment may be a bit slower, an acceptable trade-off in practice given how much it reduces code complexity.
* All RPC responses also include a "leader_id" in addition to "term" for nodes to get the information faster upon converting to follower.
* Log compaction and snapshot transfer are not supported. This is largely due to no support for membership change. Every node participates from the start so they are expected to catch up via log replication and execution alone.
* `AppendEntries` RPC responses will include a "suggested_next_index" for leader to more efficiently learn how far behind the node is rather than retrying all indices until match. This optimization is mentioned in the paper but rarely beneficial in real production, however, it may help nodes catch up much faster in many simulated failure cases performed by Maelstrom.

## Supported Workload and Implementation
Rakurima supports a large subset of the Maelstrom workloads. And it should be easily extendable to support additional workloads. It may handle multiple workloads concurrently even though this is not natively built into Maelstrom. The only exception to concurrent support is when "different" RPCs have the same message type; e.g. Broadcast and G-Counter workloads cannot be conducted simultaneously because they both have the same `read` RPC which are indistinguishable from each other.

### Echo
The most basic workload, can be used to test setup and as a shallow health check. Each node receiving the echo request responds immediately to the client without any other cross communication.

### Unique ID
This workload requests unique IDs from the server. For simplicity, we don't rely on the internal state machine to build consensus. Instead, each node generates IDs independently using a similar approach to the [Snowflake](https://blog.x.com/engineering/en_us/a/2010/announcing-snowflake) algorithm. The generated ID is a 64-bit string (Hex encoded). In this case, since there are only nodes without data centers, we use 47 bits for timestamp (Epoch milliseconds) + 7 bits for the node ID, and 10 bits for a sequence number stored on each node that increments. The sequence ID wraps once reached maximum. This means we can support over 1 million IDs per node per second on average without creating a conflict.

### Message Broadcast
The workload that announces messages to the server. In cluster mode, the messages need to propagate to every node with eventual-consistency guarantee. Because message propagation is idempotent in this scenario (i.e. we persist a set of integers on each server) without side effects, the implementation does NOT depend on the internal state machine. A simple gossip strategy is used to deliver the broadcast to peers.

The way that the peers are chosen is deterministic based on the "Topology" message from the Maelstrom client. Each broadcast is retried indefinitely until a response is heard from the peer. A node will only forward messages NOT already seen to prevent a loop. A node will respond to a peer's broadcast whether it already has the message or not to signal a retry is not needed.

### PN Counter
A workload that increments and decrements an integer value stored on the server. To guarantee correctness, we must enforce once and only-once execution of the update commands. This process will ride on the back of the internal state machine. Each increment/decrement command will be treated as an execution log to be replicated to other server nodes, and they are executed once consensus is achieved.

Due to that the "read" message in this workload has an identical type as the Broadcast workload, we rely on the presence of the "Topology" message to determine whether the server will respond with the broadcast set or the PN counter value (default without a topology message).

### Kafka
A workload that maintains a replicated log stream. This will also rely on the internal state machine. Despite of their similarity, the client-facing Kafka log and commit history are separate from the internally replicated command log so that the server can handle different requests at the same time and maintain an arbitrary number of states.

### Transaction RW Register
A workload that simulates a transaction-based, distributed key-value store. The implementation is still based on executing requested commands on the internal state machine. Note that a response is only generated after the leader commits and applies the change, so we guarantee that each write is durable. But since reads can be served from any node, there is a tiny chance for getting stale data after leader commit. Everything is still eventually consistent (typically within milliseconds) in any case.
