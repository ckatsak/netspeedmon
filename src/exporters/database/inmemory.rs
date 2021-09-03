use std::{
    fmt,
    sync::{Arc, Mutex},
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Local};
use tracing::trace;

use super::{Store, DEFAULT_HISTORY_SIZE};
use crate::measure::Measurement;

pub struct InMemory {
    inner: Arc<Mutex<Inner>>,
    history_size: usize,
}

#[derive(Debug)]
struct Inner {
    buffer: Vec<(DateTime<Local>, Measurement)>,
    curr: usize,
}

impl InMemory {
    #[tracing::instrument]
    pub fn new(history_size: usize) -> Self {
        assert!(history_size > 0, "InMemory Store with history_size == 0");
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buffer: Vec::with_capacity(history_size),
                curr: usize::MAX,
            })),
            history_size,
        }
    }
}

#[async_trait]
impl Store for InMemory {
    #[tracing::instrument(skip(self))]
    async fn retrieve_most_recent(&mut self) -> Result<Option<(DateTime<Local>, Measurement)>> {
        self.inner
            .lock()
            .map_err(|e| anyhow!("failed to acquire in-memory database lock: {}", e))
            .map(|inner| inner.buffer.get(inner.curr).cloned())
    }

    #[tracing::instrument(skip(self))]
    async fn retrieve_history(&mut self) -> Result<Vec<(DateTime<Local>, Measurement)>> {
        self.inner
            .lock()
            .map_err(|e| anyhow!("failed to acquire in-memory database lock: {}", e))
            .map(|inner| {
                let mut ret = inner.buffer.clone();
                ret.sort_unstable_by_key(|(dt, _)| *dt);
                ret
            })
    }

    #[tracing::instrument(skip(self))]
    async fn store(&mut self, timestamp: DateTime<Local>, measurement: Measurement) -> Result<()> {
        let mut im = self
            .inner
            .lock()
            .map_err(|e| anyhow!("failed to acquire in-memory database lock: {}", e))?;

        if im.buffer.len() < self.history_size {
            debug_assert!(
                (im.buffer.is_empty() && im.curr == usize::MAX)
                    || (im.buffer.len() == im.curr + 1 && im.curr < self.history_size)
            );
            im.buffer.push((timestamp, measurement));
            im.curr = im.buffer.len() - 1;
        } else {
            debug_assert!(im.buffer.len() == self.history_size && im.curr < self.history_size);
            im.curr = (im.curr + 1) % self.history_size;
            let pos = im.curr;
            trace!("Removing oldest entry: '{:?}'", im.buffer[pos]);
            im.buffer[pos] = (timestamp, measurement);
        }

        Ok(())
    }
}

impl Default for InMemory {
    #[tracing::instrument]
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                buffer: Vec::with_capacity(DEFAULT_HISTORY_SIZE),
                curr: usize::MAX,
            })),
            history_size: DEFAULT_HISTORY_SIZE,
        }
    }
}

impl fmt::Debug for InMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let im = self
            .inner
            .lock()
            .expect("failed to acquire in-memory database lock in std::fmt::Debug::fmt !");
        writeln!(f, "InMemory(history size = {}){{", self.history_size)?;
        for (timestamp, measurement) in im.buffer.iter() {
            write!(f, "{:?}: {:?}", timestamp, measurement)?;
        }
        writeln!(f, "}}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic]
    fn new_empty() {
        let _ = InMemory::new(0);
    }

    #[tokio::test]
    async fn full() -> Result<()> {
        let mut db = InMemory::new(5);
        assert!(db.retrieve_most_recent().await?.is_none());

        // First fill it up

        let m1 = (1., 1., 1.).into();
        db.store(Local::now(), m1).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m1))));

        let m2 = (2., 2., 2.).into();
        db.store(Local::now(), m2).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m2))));

        let m3 = (3., 3., 3.).into();
        db.store(Local::now(), m3).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m3))));

        let m4 = (4., 4., 4.).into();
        db.store(Local::now(), m4).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m4))));

        let m5 = (5., 5., 5.).into();
        db.store(Local::now(), m5).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m5))));

        // Now start overflowing it and make sure the ring buffer works as expected

        let m9 = (9., 9., 9.).into();
        db.store(Local::now(), m9).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m9))));

        let m8 = (8., 8., 8.).into();
        db.store(Local::now(), m8).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m8))));

        let history = db.retrieve_history().await?;
        let history = history.iter().map(|(_, m)| m).collect::<Vec<_>>();
        assert_eq!(history, vec![&m3, &m4, &m5, &m9, &m8]);

        let m7 = (7., 7., 7.).into();
        db.store(Local::now(), m7).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m7))));

        let m6 = (6., 6., 6.).into();
        db.store(Local::now(), m6).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m6))));

        let m5 = (5., 5., 5.).into();
        db.store(Local::now(), m5).await?;
        assert!(matches!(db.retrieve_most_recent().await?, Some((_, m5))));

        let history = db.retrieve_history().await?;
        let history = history.iter().map(|(_, m)| m).collect::<Vec<_>>();
        assert_eq!(history, vec![&m9, &m8, &m7, &m6, &m5]);

        Ok(())
    }
}
