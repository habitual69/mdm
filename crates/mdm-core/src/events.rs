use serde::Serialize;

use crate::model::Job;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum Event {
    JobUpdated { job: Box<Job> },
    JobRemoved { id: String },
    QueueIdle,
}

pub type EventTx = tokio::sync::broadcast::Sender<Event>;
pub type EventRx = tokio::sync::broadcast::Receiver<Event>;
