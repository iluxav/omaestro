//! Clipboard, key injection and the model.

use std::sync::{Mutex, MutexGuard, PoisonError};

use super::Journal;
use crate::backend::{
    BackendError, BoxFuture, ChatRequest, ClipContent, Clipboard, Injector, Llm, Result, Shell,
    ShellOutput,
};
use crate::chord::Chord;

#[derive(Default)]
struct ClipState {
    primary: String,
    content: Option<ClipContent>,
}

#[derive(Default)]
pub struct FakeClipboard {
    journal: Journal,
    state: Mutex<ClipState>,
}

impl FakeClipboard {
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, ClipState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The user selects text.
    pub fn select(&self, text: &str) {
        self.state().primary = text.to_string();
    }

    /// The user copies something.
    pub fn copy(&self, content: ClipContent) {
        self.state().content = Some(content);
    }

    pub fn content(&self) -> Option<ClipContent> {
        self.state().content.clone()
    }
}

impl Clipboard for FakeClipboard {
    fn selection(&self) -> BoxFuture<'_, Result<String>> {
        Box::pin(async move { Ok(self.state().primary.clone()) })
    }

    fn get(&self) -> BoxFuture<'_, Result<Option<ClipContent>>> {
        Box::pin(async move { Ok(self.state().content.clone()) })
    }

    fn set<'a>(&'a self, content: &'a ClipContent) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.push(format!(
                "clipboard = {} {:?}",
                content.mime,
                String::from_utf8_lossy(&content.data)
            ));
            self.state().content = Some(content.clone());
            Ok(())
        })
    }

    fn clear(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move {
            self.journal.push("clipboard cleared".to_string());
            self.state().content = None;
            Ok(())
        })
    }
}

#[derive(Default)]
pub struct FakeInjector {
    journal: Journal,
}

impl FakeInjector {
    pub fn new(journal: Journal) -> Self {
        Self { journal }
    }
}

impl Injector for FakeInjector {
    fn key<'a>(&'a self, chord: &'a Chord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.push(format!("key {}", chord.id()));
            Ok(())
        })
    }

    fn type_text<'a>(&'a self, text: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.journal.push(format!("type {text:?}"));
            Ok(())
        })
    }
}

pub struct FakeLlm {
    requests: Mutex<Vec<ChatRequest>>,
    answer: Mutex<std::result::Result<String, String>>,
}

impl Default for FakeLlm {
    fn default() -> Self {
        Self {
            requests: Mutex::default(),
            answer: Mutex::new(Ok("model says hi".to_string())),
        }
    }
}

impl FakeLlm {
    pub fn answer(&self, text: &str) {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Ok(text.to_string());
    }

    pub fn fail(&self, message: &str) {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = Err(message.to_string());
    }

    pub fn requests(&self) -> Vec<ChatRequest> {
        self.requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Llm for FakeLlm {
    fn chat<'a>(&'a self, request: &'a ChatRequest) -> BoxFuture<'a, Result<String>> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(request.clone());
            self.answer
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
                .map_err(BackendError::Other)
        })
    }
}

/// Answers commands from a queue; with the queue empty, every command
/// succeeds with no output.
pub struct FakeShell {
    journal: Journal,
    answers: Mutex<std::collections::VecDeque<ShellOutput>>,
}

impl FakeShell {
    pub fn new(journal: Journal) -> Self {
        Self {
            journal,
            answers: Mutex::default(),
        }
    }

    fn queue(&self, output: ShellOutput) {
        self.answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(output);
    }

    /// The next command prints `stdout` and succeeds.
    pub fn answer(&self, stdout: &str) {
        self.queue(ShellOutput {
            status: Some(0),
            stdout: stdout.to_string(),
            stderr: String::new(),
        });
    }

    /// The next command fails with `status` and `stderr`.
    pub fn fail(&self, status: Option<i32>, stderr: &str) {
        self.queue(ShellOutput {
            status,
            stdout: String::new(),
            stderr: stderr.to_string(),
        });
    }
}

impl Shell for FakeShell {
    fn run<'a>(&'a self, command: &'a str) -> BoxFuture<'a, Result<ShellOutput>> {
        Box::pin(async move {
            self.journal.push(format!("sh {command}"));
            let answer = self
                .answers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            Ok(answer.unwrap_or(ShellOutput {
                status: Some(0),
                ..ShellOutput::default()
            }))
        })
    }
}
