#![allow(unused)]

use std::{
    env,
    io::{stdin, stdout, BufRead, Write},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use anyhow::Context;
use handler::{InputHandler, OutputHandler};
use logger::Logger;
use message::Payload::*;
use message::{Body, Message};
use node::{Node, NodeConfig, NodeMode};
use rakurima::*;

const DEF_BASE_PAUSE_TIME_MS: usize = 10;

fn main() -> anyhow::Result<()> {
    let logger: &'static Logger = Box::leak(Box::new(Logger {}));
    logger.log_debug("Rakurima is starting up...");

    // Retrieve environment variables.
    let base_pause_time_ms =
        get_numeric_environment_variable(logger, "BASE_PAUSE_TIME_MS", DEF_BASE_PAUSE_TIME_MS);

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

            break Node::new(
                node_id,
                NodeMode::from_node_ids(node_ids),
                NodeConfig::new(base_pause_time_ms),
                logger,
                in_receiver,
                out_sender.clone(),
            );
        } else {
            logger.log_debug("Non-init message during startup, ignoring...");
        }
    };

    logger.log_debug(&format!("Server node: {server_node:?} initialized."));

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

/// Reads and parses out a numeric value from an environment variable key.
/// The given default will be returned if the key is missing or the value is invalid.
fn get_numeric_environment_variable(
    logger: &'static Logger,
    key: &str,
    default_val: usize,
) -> usize {
    match env::var(key) {
        Ok(val) => match val.parse() {
            Ok(v) => {
                logger.log_debug(&format!("{key}: set to value: {v}."));
                v
            }
            Err(_) => {
                logger.log_debug(&format!(
                    "{key}: parsing failed for value: {val}. Using default: {default_val}."
                ));
                default_val
            }
        },
        Err(_) => {
            logger.log_debug(&format!("{key}: not found. Using default: {default_val}."));
            default_val
        }
    }
}
