use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use thiserror::Error;
use tokio::sync::mpsc;
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
    tx: mpsc::Sender<u64>,
    rx: mpsc::Receiver<u64>,
}

impl Scheduler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(256);
        Self {
            tasks: Vec::new(),
            next_id: 1,
            tx,
            rx,
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

        // Execute ready tasks (in production, spawn them)
        for i in ready.into_iter().rev() {
            if let Some(task) = self.tasks.get_mut(i) {
                if task.interval.is_none() {
                    self.tasks.remove(i);
                } else {
                    task.execute_at = Instant::now() + task.interval.unwrap();
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
