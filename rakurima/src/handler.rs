use std::{
    io::{stdin, stdout, Write},
    sync::mpsc::{Receiver, Sender},
};

use crate::{
    logger::ServerLogger,
    message::{Message, Payload::*},
};

#[derive(Debug)]
pub struct InputHandler {
    #[allow(dead_code)]
    node_id: String,
    logger: &'static ServerLogger,
    in_sender: Sender<Message>,
    out_sender: Sender<Message>,
}

impl InputHandler {
    pub fn new(
        node_id: String,
        logger: &'static ServerLogger,
        in_sender: Sender<Message>,
        out_sender: Sender<Message>,
    ) -> Self {
        Self {
            node_id,
            logger,
            in_sender,
            out_sender,
        }
    }

    /// Continues to read (block if needed) input lines from stdin and handles them based on the message.
    pub fn handle_input(&mut self) -> anyhow::Result<()> {
        let stdin_lines = stdin().lines();

        for line in stdin_lines {
            let line = line?;
            match serde_json::from_str::<Message>(&line) {
                Ok(message) => {
                    self.handle_message(message)?;
                }
                Err(_) => {
                    self.logger
                        .log_debug("Unsupported or malformed input line ignored.");
                }
            }
        }

        Ok(())
    }

    /// Performs message specific action(s).
    /// Most messages are forwarded to the server node.
    fn handle_message(&mut self, message: Message) -> anyhow::Result<()> {
        match &message.body.payload {
            Error { code, text } => {
                // Swallow the error to prevent loop.
                self.logger
                    .log_debug(&format!("Swallowed error message: {code} @ {text}."));
            }
            Echo { echo } => {
                let echo = echo.to_string();
                // Send back the echo response.
                self.out_sender
                    .send(Message::into_response(message, EchoOk { echo }, None))?
            }
            Generate { .. }
            | Topology { .. }
            | Broadcast { .. }
            | BroadcastOk {}
            | Read {}
            | Add { .. }
            | Send { .. }
            | Poll { .. }
            | CommitOffsets { .. }
            | ListCommittedOffsets { .. }
            | Txn { .. } => {
                // Maelstrom client/server messages.
                // Send these messages over to the server for further processing.
                self.in_sender.send(message)?;
            }
            AppendEntries { .. }
            | AppendEntriesResult { .. }
            | RequestVote { .. }
            | RequestVoteResult { .. }
            | RaftRequestForward { .. }
            | RaftRequestAck { .. } => {
                // Raft internal messages.
                self.in_sender.send(message)?;
            }
            _ => {
                // Unexpected, respond with an error.
                self.logger
                    .log_debug(&format!("Received unexpected message: {message:?}."));
                self.out_sender.send(Message::into_response(
                    message,
                    Error {
                        code: 10,
                        text: "Such message is not supported".to_string(),
                    },
                    None,
                ))?
            }
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct OutputHandler {
    #[allow(dead_code)]
    node_id: String,
    #[allow(dead_code)]
    logger: &'static ServerLogger,
    out_receiver: Receiver<Message>,
}

impl OutputHandler {
    pub fn new(
        node_id: String,
        logger: &'static ServerLogger,
        out_receiver: Receiver<Message>,
    ) -> Self {
        Self {
            node_id,
            logger,
            out_receiver,
        }
    }

    /// Continues to receive messages sent from upstream and write them to stdout.
    /// A trailing new line is always appended after each message.
    pub fn handle_output(&mut self) -> anyhow::Result<()> {
        let mut stdout = stdout().lock();

        while let Ok(message) = self.out_receiver.recv() {
            serde_json::to_writer(&mut stdout, &message)?;
            // Write a trailing new line.
            stdout.write_all(b"\n")?;
        }

        Ok(())
    }
}
