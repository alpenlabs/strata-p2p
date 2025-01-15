//! Module for timeouts manager.
use std::{
    collections::HashMap,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use bitcoin::hashes::sha256;
use futures::{FutureExt, Stream, StreamExt};
use strata_p2p_db::OperatorPubkey;
use tokio::time::{sleep, Sleep};

/// Kind of timeout which can be omitted by [`TimeoutsManager`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum TimeoutEvent {
    /// Timeout related to genesis stage for some operator.
    Genesis { operator_pk: OperatorPubkey },
    /// Timeout related to deposit processing stage for some operator and deposit.
    Deposit {
        operator_pk: OperatorPubkey,
        scope: sha256::Hash,
    },
}

impl TimeoutEvent {
    #[allow(unused)]
    pub fn operator_pk(self) -> OperatorPubkey {
        match self {
            TimeoutEvent::Genesis { operator_pk } | TimeoutEvent::Deposit { operator_pk, .. } => {
                operator_pk
            }
        }
    }
}

/// Manager for timeouts by operator id and deposit transactions id.
///
/// Implements endless stream, that returns execution if one of the timeouts have ended.
///
/// The list of timeouts can be updated.
pub(crate) struct TimeoutsManager {
    // TODO(Velnbur): make this persistent
    timeouts: HashMap<TimeoutEvent, Pin<Box<Sleep>>>,
}

impl TimeoutsManager {
    pub fn new() -> Self {
        Self {
            timeouts: Default::default(),
        }
    }

    pub fn set_deposit_timeout(
        &mut self,
        operator_pk: OperatorPubkey,
        scope: sha256::Hash,
        timeout: Duration,
    ) {
        let sleep = sleep(timeout);
        self.timeouts.insert(
            TimeoutEvent::Deposit { operator_pk, scope },
            Box::pin(sleep),
        );
    }

    pub fn set_genesis_timeout(&mut self, operator_pk: OperatorPubkey, timeout: Duration) {
        let sleep = sleep(timeout);
        self.timeouts
            .insert(TimeoutEvent::Genesis { operator_pk }, Box::pin(sleep));
    }

    pub async fn next_timeout(&mut self) -> TimeoutEvent {
        self.next().await.unwrap()
    }
}

impl Stream for TimeoutsManager {
    type Item = TimeoutEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        if self.timeouts.is_empty() {
            return Poll::Pending;
        }

        // find first interval that is ready
        let mut event_to_remove = None;
        for (event, timeout) in self.timeouts.iter_mut() {
            if timeout.poll_unpin(cx).is_ready() {
                event_to_remove = Some(event.clone());
                break;
            }
        }

        if let Some(event) = event_to_remove {
            // do now await this future, as we already checked that it's finished.
            let _ = self.timeouts.remove(&event);
            Poll::Ready(Some(event))
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bitcoin::hashes::{sha256, Hash};
    use futures::StreamExt;
    use strata_p2p_db::OperatorPubkey;

    use crate::timeouts::TimeoutsManager;

    #[tokio::test]
    async fn test_reversed_order_timeouts_returned_in_forward_order() {
        const TIMEOUTS_NUM: usize = 5;
        let mut mng = TimeoutsManager::new();

        let operators = vec![OperatorPubkey(vec![1]); TIMEOUTS_NUM];

        for (idx, operator_pk) in operators.iter().enumerate() {
            mng.set_deposit_timeout(
                operator_pk.clone(),
                sha256::Hash::hash(&idx.to_le_bytes()),
                Duration::from_millis(500 - idx as u64 * 100),
            );
        }

        let mut collected: Vec<OperatorPubkey> = mng
            .map(|event| event.operator_pk())
            .take(TIMEOUTS_NUM)
            .collect()
            .await;
        collected.reverse();

        assert_eq!(operators, collected);
    }

    #[tokio::test]
    async fn test_next_timeout_works() {
        let mut mng = TimeoutsManager::new();

        let operators = vec![OperatorPubkey(vec![1]); 3];

        for (idx, operator_pk) in operators.iter().enumerate() {
            mng.set_deposit_timeout(
                operator_pk.clone(),
                sha256::Hash::hash(&idx.to_le_bytes()),
                Duration::from_millis(100 * idx as u64),
            );
            assert_eq!(mng.timeouts.len(), idx + 1);
        }

        assert_eq!(mng.next_timeout().await.operator_pk(), operators[0]);
        println!("hello1");
        assert_eq!(mng.next_timeout().await.operator_pk(), operators[1]);
        println!("hello2");
        assert_eq!(mng.next_timeout().await.operator_pk(), operators[2]);
    }

    #[tokio::test]
    async fn test_push_after_next_timeout() {
        let mut mng = TimeoutsManager::new();

        let operators = vec![OperatorPubkey(vec![1]); 4];

        for (idx, operator_pk) in operators.iter().take(3).enumerate() {
            mng.set_deposit_timeout(
                operator_pk.clone(),
                sha256::Hash::hash(&idx.to_le_bytes()),
                Duration::from_millis(100 * idx as u64),
            );
            assert_eq!(mng.timeouts.len(), idx + 1);
        }

        assert_eq!(mng.next_timeout().await.operator_pk(), operators[0]);
        mng.set_genesis_timeout(operators[3].clone(), Duration::from_millis(50));
        assert_eq!(mng.next_timeout().await.operator_pk(), operators[3]);
        assert_eq!(mng.next_timeout().await.operator_pk(), operators[1]);
        assert_eq!(mng.next_timeout().await.operator_pk(), operators[2]);
    }

    #[tokio::test]
    async fn test_timeouts_mng_cancelling() {}
}
