//! Tasks Module - Backend-agnostic task management
//!
//! Architecture:
//! - Tasks are stored in Xavier's own backend (TaskStore)
//! - Planka is just a sync target (optional)
//! - Can work fully offline without Planka

pub mod models;
pub mod scoring;
pub mod session_sync_task;
pub mod store;
pub mod sync;

use std::collections::VecDeque;

use tokio::sync::Mutex;

pub use store::{InMemoryTaskStore, TaskService, TaskStore};

// Re-exports

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskPriority {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub name: String,
    pub description: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub error_message: Option<String>,
}

impl Task {
    /// New.
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description,
            status: TaskStatus::Pending,
            priority: TaskPriority::Medium,
            error_message: None,
        }
    }

    /// With priority.
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Start.
    pub fn start(&mut self) {
        self.status = TaskStatus::InProgress;
    }

    /// Complete.
    pub fn complete(&mut self) {
        self.status = TaskStatus::Completed;
        self.error_message = None;
    }

    /// Fail.
    pub fn fail(&mut self, error: String) {
        self.status = TaskStatus::Failed;
        self.error_message = Some(error);
    }

    /// Cancel.
    pub fn cancel(&mut self) {
        self.status = TaskStatus::Cancelled;
    }

    /// Can execute.
    pub fn can_execute(&self) -> bool {
        matches!(self.status, TaskStatus::Pending | TaskStatus::Failed)
    }
}

#[derive(Default)]
pub struct TaskQueue {
    tasks: Mutex<VecDeque<Task>>,
}

impl TaskQueue {
    /// New.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enqueue.
    pub async fn enqueue(&self, task: Task) {
        let mut tasks = self.tasks.lock().await;
        tasks.push_back(task);
        let mut ordered: Vec<_> = tasks.drain(..).collect();
        ordered.sort_by_key(|task| std::cmp::Reverse(task.priority));
        *tasks = ordered.into();
    }

    /// Dequeue.
    pub async fn dequeue(&self) -> Option<Task> {
        self.tasks.lock().await.pop_front()
    }

    /// Is empty.
    pub fn is_empty(&self) -> bool {
        self.tasks
            .try_lock()
            .map(|tasks| tasks.is_empty())
            .unwrap_or(false)
    }

    /// Len.
    pub fn len(&self) -> usize {
        self.tasks.try_lock().map(|tasks| tasks.len()).unwrap_or(0)
    }
}

/// Default projects for SouthLabs
pub fn default_projects() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Xavier", "Sistema de memoria y cognitive"),
        ("ZeroClaw", "Runtime Rust para agentes"),
        ("Trading Bot", "Automatizacion de trading"),
        ("ManteniApp", "SaaS mantenimiento industrial"),
        ("Research", "Investigacion y experimentos"),
        ("Ops", "Infraestructura y DevOps"),
    ]
}
