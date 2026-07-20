use std::time::Duration;
use tokio::task::JoinHandle;

#[derive(Default)]
pub struct ConnectionTasks(Vec<JoinHandle<()>>);

impl ConnectionTasks {
    pub fn push(&mut self, task: JoinHandle<()>) {
        self.0.retain(|task| !task.is_finished());
        self.0.push(task);
    }

    pub async fn finish(&mut self) {
        let deadline = tokio::time::Instant::now() + Duration::from_millis(500);
        while let Some(mut task) = self.0.pop() {
            if tokio::time::timeout_at(deadline, &mut task).await.is_err() {
                task.abort();
                let _ = task.await;
                break;
            }
        }
        self.abort_all();
        for task in self.0.drain(..) {
            let _ = task.await;
        }
    }

    fn abort_all(&self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

impl Drop for ConnectionTasks {
    fn drop(&mut self) {
        self.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shutdown_aborts_stalled_connections_at_one_shared_deadline() {
        let mut connections = ConnectionTasks::default();
        for _ in 0..3 {
            connections.push(tokio::spawn(std::future::pending()));
        }

        tokio::time::timeout(Duration::from_secs(1), connections.finish())
            .await
            .expect("connection shutdown must be bounded");
        assert!(connections.0.is_empty());
    }
}
