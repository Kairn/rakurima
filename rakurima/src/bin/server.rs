#![allow(unused)]

use std::{
    env,
    io::{stdin, stdout, BufRead, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::Context;
use handler::{InputHandler, OutputHandler};
use logger::ServerLogger;
use message::Payload::*;
use message::{Body, Message};
use node::{Node, NodeConfig, NodeMode};
use raft::{RaftConfig, RaftCore};
use rakurima::*;
use util::get_numeric_environment_variable;

const DEF_BASE_PAUSE_TIME_MS: usize = 10;
const DEF_BASE_BROADCAST_RETRY_MS: usize = 200;
const DEF_BASE_ELECTION_TIMEOUT_MS: usize = 3000;
const DEF_BASE_REPLICATE_INTERVAL_MS: usize = 150;

fn main() -> anyhow::Result<()> {
    let logger: &'static ServerLogger = Box::leak(Box::new(ServerLogger {}));
    logger.log_debug("Rakurima is starting up...");

    // Retrieve environment variables.
    let base_pause_time_ms =
        get_numeric_environment_variable(logger, "BASE_PAUSE_TIME_MS", DEF_BASE_PAUSE_TIME_MS);
    let base_broadcast_retry_ms = get_numeric_environment_variable(
        logger,
        "BASE_BROADCAST_RETRY_MS",
        DEF_BASE_BROADCAST_RETRY_MS,
    );
    let base_election_timeout_ms = get_numeric_environment_variable(
        logger,
        "BASE_ELECTION_TIMEOUT_MS",
        DEF_BASE_ELECTION_TIMEOUT_MS,
    );
    let base_replicate_interval_ms = get_numeric_environment_variable(
        logger,
        "BASE_REPLICATE_INTERVAL_MS",
        DEF_BASE_REPLICATE_INTERVAL_MS,
    );

    let stdin = stdin().lock();
    let mut stdout = stdout().lock();

    // Input/Output channels for cross-thread communications.
    let (in_sender, in_receiver) = mpsc::channel::<Message>();
    let (out_sender, out_receiver) = mpsc::channel::<Message>();

    // Wait for the init message on startup.
    logger.log_debug("Server node is preparing to initialize...");
    let mut stdin_lines = stdin.lines();
    let mut server_node = loop {
        let line = stdin_lines.next().unwrap()?;
        let message: Message = serde_json::from_str(&line)?;

        if let Init { node_id, node_ids } = message.body.payload {
            // Respond to the init message.
            serde_json::to_writer(
                &mut stdout,
                &Message {
                    src: message.dst,
                    dst: message.src,
                    body: Body {
                        msg_id: None,
                        in_reply_to: message.body.msg_id,
                        payload: InitOk {},
                    },
                },
            )
            .context("Writing init_ok response")?;
            // Write a trailing new line.
            stdout.write_all(b"\n")?;

            let raft_core = RaftCore::new(
                RaftConfig::new(base_election_timeout_ms, base_replicate_interval_ms),
                node_id.as_str(),
                node_ids.len(),
                out_sender.clone(),
            );

            break Node::new(
                node_id,
                NodeMode::from_node_ids(node_ids),
                NodeConfig::new(base_pause_time_ms, base_broadcast_retry_ms),
                logger,
                in_receiver,
                out_sender.clone(),
                raft_core,
            );
        } else {
            logger.log_debug("Non-init message during startup, ignoring...");
        }
    };

    logger.log_debug(&format!("Server node: {server_node:?} initialized."));
    if server_node.is_singleton() {
        logger.log_debug("Node is in singleton mode.");
    }

    // Drop the current I/O locks as the handler threads will be using them.
    drop(stdin_lines);
    drop(stdout);

    // Start the I/O handler threads and the server orchestration.
    let node_id = server_node.get_node_id().to_string();
    thread::spawn(move || {
        let mut input_handler = InputHandler::new(node_id, logger, in_sender, out_sender);
        input_handler
            .handle_input()
            .context("Stdin handler error")
            .unwrap();
    });
    let node_id = server_node.get_node_id().to_string();
    thread::spawn(move || {
        let mut output_handler = OutputHandler::new(node_id.to_string(), logger, out_receiver);
        output_handler
            .handle_output()
            .context("Stdout handler error")
            .unwrap();
    });

    server_node.orchestrate()?;

    Ok(())
}
