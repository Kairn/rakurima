use std::{
    any::Any,
    io::{stdin, stdout, StdinLock, Write},
    sync::mpsc::{Receiver, Sender},
};

use crate::{
    logger::{self, Logger},
    message::{self, Body, Message, Payload::*},
};

#[derive(Debug)]
pub struct InputHandler {
    node_id: String,
    logger: &'static Logger,
    in_sender: Sender<Message>,
    out_sender: Sender<Message>,
}

impl InputHandler {
    pub fn new(
        node_id: String,
        logger: &'static Logger,
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

    pub fn handle_input(&mut self) -> anyhow::Result<()> {
        let stdin_lines = stdin().lines();

        for line in stdin_lines {
            let line = line?;
            match serde_json::from_str::<Message>(&line) {
                Ok(message) => {
                    self.handle_message(message)?;
                }
                Err(e) => {
                    self.logger
                        .log_debug("Unsupported or malformed input line ignored.");
                }
            }
        }

        Ok(())
    }

    fn handle_message(&mut self, message: Message) -> anyhow::Result<()> {
        match &message.body.payload {
            Echo { echo } => {
                let echo = echo.to_string();
                // Send back the echo response.
                self.out_sender
                    .send(Message::into_response(message, EchoOk { echo }, None))?
            }
            Error { code, text } => {
                // Swallow the error to prevent loop.
                self.logger
                    .log_debug(&format!("Swallowed error message: {code} @ {text}."));
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
    node_id: String,
    logger: &'static Logger,
    out_receiver: Receiver<Message>,
}

impl OutputHandler {
    pub fn new(node_id: String, logger: &'static Logger, out_receiver: Receiver<Message>) -> Self {
        Self {
            node_id,
            logger,
            out_receiver,
        }
    }

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
