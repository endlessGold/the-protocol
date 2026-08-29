//! Task scheduler.
//!
//! **This crate is currently inert.** `Scheduler::new()` has no callers
//! anywhere in the workspace, and `tick()` only does bookkeeping - it never
//! polls or spawns the `BoxFuture` it stores, so a scheduled task would
//! never actually run even if something did drive it.
//!
//! Anything that needs real periodic work today should use `tokio::spawn`
//! with `tokio::time::interval` directly rather than building on this. It's
//! kept because the shape is a reasonable starting point if a real
//! scheduler is wanted later, but treat it as unimplemented, not as
//! infrastructure.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use thiserror::Error;
use tokio::time::Instant;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Task not found: {0}")]
    NotFound(u64),

    #[error("Scheduler closed")]
    Closed,
}

pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

pub struct ScheduledTask {
    pub id: u64,
    pub name: String,
    pub execute_at: Instant,
    pub interval: Option<Duration>,
    pub task: BoxFuture,
}

pub struct Scheduler {
    tasks: Vec<ScheduledTask>,
    next_id: u64,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            tasks: Vec::new(),
            next_id: 1,
        }
    }

    pub fn schedule<F>(&mut self, name: &str, delay: Duration, task: F) -> u64
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;

        let scheduled = ScheduledTask {
            id,
            name: name.to_string(),
            execute_at: Instant::now() + delay,
            interval: None,
            task: Box::pin(task),
        };

        self.tasks.push(scheduled);
        id
    }

    pub fn schedule_interval<F>(&mut self, name: &str, interval: Duration, mut task: F) -> u64
    where
        F: FnMut() + Send + 'static,
    {
        let id = self.next_id;
        self.next_id += 1;

        let fut = async move {
            let mut interval_timer = tokio::time::interval(interval);
            loop {
                interval_timer.tick().await;
                task();
            }
        };

        let scheduled = ScheduledTask {
            id,
            name: name.to_string(),
            execute_at: Instant::now() + interval,
            interval: Some(interval),
            task: Box::pin(fut),
        };

        self.tasks.push(scheduled);
        id
    }

    pub async fn tick(&mut self) {
        let now = Instant::now();
        let mut ready: Vec<usize> = Vec::new();

        for (i, task) in self.tasks.iter().enumerate() {
            if task.execute_at <= now {
                ready.push(i);
            }
        }

        // NOTE: this only does the bookkeeping - it removes one-shot tasks
        // and reschedules interval ones. It does NOT execute `task.task`;
        // nothing polls or spawns those futures, and `Scheduler` has no
        // callers anywhere in the workspace. See the crate docs.
        for i in ready.into_iter().rev() {
            match self.tasks.get_mut(i).and_then(|task| task.interval) {
                Some(interval) => {
                    if let Some(task) = self.tasks.get_mut(i) {
                        task.execute_at = Instant::now() + interval;
                    }
                }
                None => {
                    self.tasks.remove(i);
                }
            }
        }
    }

    pub fn cancel(&mut self, id: u64) -> Result<(), SchedulerError> {
        self.tasks.retain(|t| t.id != id);
        Ok(())
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
