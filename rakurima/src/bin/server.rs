#![allow(unused)]

use std::{
    env,
    io::{stdin, stdout, BufRead, Write},
    sync::mpsc::{self, Receiver, Sender},
};

use anyhow::Context;
use message::Payload::*;
use message::{Body, Message};
use node::{Node, NodeConfig, NodeMode};
use rakurima::*;

const DEF_BASE_PAUSE_TIME_MS: usize = 10;

fn main() -> anyhow::Result<()> {
    eprintln!("Rakurima is starting up...");

    // Retrieve environment variables.
    let base_pause_time_ms =
        get_numeric_environment_variable("BASE_PAUSE_TIME_MS", DEF_BASE_PAUSE_TIME_MS);

    let stdin = stdin().lock();
    let mut stdout = stdout().lock();

    // Wait for the init message on startup.
    eprintln!("Server node is preparing to initialize...");
    let mut stdin_lines = stdin.lines();
    let server_node = loop {
        let line = stdin_lines.next().unwrap()?;
        let message: Message = serde_json::from_str(&line)?;

        if let Init { node_id, node_ids } = message.body.payload {
            // Response to the init message.
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

            // Write trailing new line.
            stdout.write(b"\n")?;
            break Node {
                node_id,
                mode: NodeMode::from_node_ids(node_ids),
                config: NodeConfig { base_pause_time_ms },
            };
        } else {
            eprintln!("Non-init message during startup, ignoring...");
        }
    };

    eprintln!("Server node: {server_node:?} initialized.");

    // Input/Output channels for cross-thread communications.
    let (in_sender, in_receiver) = mpsc::channel::<Message>();
    let (out_sender, out_receiver) = mpsc::channel::<Message>();

    Ok(())
}

fn get_numeric_environment_variable(key: &str, default_val: usize) -> usize {
    match env::var(key) {
        Ok(val) => match val.parse() {
            Ok(v) => {
                eprintln!("{key}: set to value: {v}.");
                v
            }
            Err(_) => {
                eprintln!("{key}: parsing failed for value: {val}. Using default: {default_val}.");
                default_val
            }
        },
        Err(_) => {
            eprintln!("{key}: not found. Using default: {default_val}.");
            default_val
        }
    }
}
