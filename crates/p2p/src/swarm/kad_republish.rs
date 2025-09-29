//! This is essentially a coroutine written in terms of `Stream`.
//! The task is to republish our own record.
//! The logic for when to do is fully in this file.
//!
//! Here's a diagram of logic, without some details of implementation:
//!
//! ┌───────────────────────────────┐                                   
//! │                               │                                   
//! │                            ┌──┴──┐                                
//! │┌───────────────────────────┼─────┼──────────────┐                 
//! ││         Kademlia's Events │     │              │                 
//! ││┌──────────────┐ ┌─────────▼─┐ ┌─▼────────────┐ │                 
//! │││RoutingUpdated│ │PutRecordOk│ │PutRecordError│ │                 
//! ││└──────────────┘ └─────┬─────┘ └──────┬───────┘ │                 
//! │└───────┬───────────────┼──────────────┼─────────┘                 
//! │        │               │              │                           
//! │        │               │              │                           
//! │        │               │              │                           
//! │        └────────┐      │              │                           
//! │                 │      │              │                           
//! │  ┌──────────────│──────│──────────────│──────────────────────────┐
//! │  │Internal      │      │              │                          │
//! │  │states        │      └──────────────│─┐                        │
//! │  │              │                     │ │                        │
//! │  │┌─────────┐   ▼     ┌──────────────┐│ ▼   ┌───────────┐        │
//! │  ││FirstTime├────────►│RoutingUpdated┼─────►│PutRecordOk├──┐     │
//! │  │└─────────┘         └──────┬─────┬─┘│     └───┬───────┘  │     │
//! │  │                           │     │◄─┘         │          │     │
//! │  │                           │ ┌───▼──────────┐ │          │     │
//! │  │                           │ │PutRecordError│ │          │     │
//! │  └───────────────────────┐   │ └───────┬──────┘ │          │     │
//! │ ┌──────────────────┐     │   │         │  │     │          │     │
//! │ │flag:is_first_time┼─────│──►│         │  │     │          │     │
//! │ └──────────────────┘     │   │         │  │     │          │     │
//! │ ┌────────┐               │   │         │  │     │          │     │
//! │ │interval│ set to i.e. 1h│   │         │  │     │          │     │
//! │ └────────┘◄──────────────│───│─────────│──│─────┘          │     │
//! │         ▲  set to i.e. 5m│   │         │  │                │     │
//! │         └────────────────│───│─────────┘  │                │     │
//! │                          │   │            │                │     │
//! │                          │   │            │                │     │
//! │ if interval triggered    │   │ ┌──────────▼────┐           │     │
//! │    ┌─────────────────────│───│─│IntervalUpdated◄───────────┘     │
//! │    │                     │   │ └───────────────┘                 │
//! │    ▼                     └───┼───────────────────────────────────┘
//! │  ┌──────────────────┐        │                                    
//! └──┼action: put record│  ◄─────┘                                    
//!    └──────────────────┘                                             

use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{Stream, lock::Mutex, task::AtomicWaker};
use tokio::time::{Instant, Interval, interval_at};
use tracing::{trace, warn};

#[derive(Debug, Clone)]
pub(crate) enum WhyThisIsPolled {
    FirstTime,

    RoutingUpdated,

    PutRecordOk,

    PutRecordError,

    IntervalTriggered,
}

// Stream because it's the closest to coroutines.
pub(crate) struct KadRepublishStream {
    waker: Arc<AtomicWaker>,
    interval: Option<Interval>,
    dur: Duration,
    dur_fail: Duration,
    is_routing_updated_first_time: bool,
    previous_event: Option<WhyThisIsPolled>,
    pub(crate) why_this_has_been_polled: Arc<Mutex<Option<WhyThisIsPolled>>>,
}

impl KadRepublishStream {
    /// dur      - duration of timer in case of PutRecordOk
    /// dur_fail - duration of timer in case of PutRecordError
    pub(crate) fn new(dur: Duration, dur_fail: Duration) -> Self {
        KadRepublishStream {
            waker: Arc::new(AtomicWaker::new()),
            interval: None,
            dur,
            dur_fail,
            is_routing_updated_first_time: true,
            previous_event: None,
            why_this_has_been_polled: Arc::new(Mutex::new(Some(WhyThisIsPolled::FirstTime))),
        }
    }

    pub(crate) fn send_event(&mut self, event: WhyThisIsPolled) {
        match event {
            WhyThisIsPolled::RoutingUpdated => {
                if self.is_routing_updated_first_time {
                    trace!("RoutingUpdated first time");
                    self.waker.wake();
                    let state = Arc::get_mut(&mut self.why_this_has_been_polled).unwrap();
                    *state.get_mut() = Some(WhyThisIsPolled::RoutingUpdated);
                } else {
                    trace!("RoutingUpdated not first time");
                }
            }
            WhyThisIsPolled::PutRecordOk => {
                self.waker.wake();
                let state = Arc::get_mut(&mut self.why_this_has_been_polled).unwrap();
                *state.get_mut() = Some(WhyThisIsPolled::PutRecordOk);
            }
            WhyThisIsPolled::PutRecordError => {
                self.waker.wake();
                let state = Arc::get_mut(&mut self.why_this_has_been_polled).unwrap();
                *state.get_mut() = Some(WhyThisIsPolled::PutRecordError);
            }
            _ => {}
        }
    }
}

// This is essentially a state machine...
impl Stream for KadRepublishStream {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();
        let state = Arc::get_mut(&mut self_mut.why_this_has_been_polled).unwrap();
        match state.get_mut() {
            Some(WhyThisIsPolled::FirstTime) => {
                trace!("FirstTime");
                self_mut.waker.register(cx.waker());
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());
                Poll::Pending
            }
            Some(WhyThisIsPolled::RoutingUpdated) => {
                if self_mut.is_routing_updated_first_time {
                    trace!("RoutingUpdated first time");
                    self_mut.is_routing_updated_first_time = false;
                    Poll::Ready(Some(()))
                } else {
                    trace!("RoutingUpdated not first time");
                    Poll::Pending
                }
            }
            Some(WhyThisIsPolled::PutRecordOk) => {
                trace!("PutRecordOk");
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());
                *state.get_mut() = Some(WhyThisIsPolled::IntervalTriggered);

                self_mut.interval = Some(interval_at(Instant::now() + self_mut.dur, self_mut.dur));
                let is_interval_says_it_should_be_triggered =
                    self_mut.interval.as_mut().unwrap().poll_tick(cx);

                match is_interval_says_it_should_be_triggered {
                    Poll::Ready(_) => Poll::Ready(Some(())),
                    Poll::Pending => Poll::Pending,
                }
            }
            Some(WhyThisIsPolled::PutRecordError) => {
                trace!("PutRecordError");
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());
                *state.get_mut() = Some(WhyThisIsPolled::IntervalTriggered);

                self_mut.interval = Some(interval_at(
                    Instant::now() + self_mut.dur_fail,
                    self_mut.dur_fail,
                ));
                let is_interval_says_it_should_be_triggered =
                    self_mut.interval.as_mut().unwrap().poll_tick(cx);

                match is_interval_says_it_should_be_triggered {
                    Poll::Ready(_) => Poll::Ready(Some(())),
                    Poll::Pending => Poll::Pending,
                }
            }
            Some(WhyThisIsPolled::IntervalTriggered) => {
                trace!("IntervalTriggered");
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());

                let is_interval_says_it_should_be_triggered =
                    self_mut.interval.as_mut().unwrap().poll_tick(cx);

                match is_interval_says_it_should_be_triggered {
                    Poll::Ready(_) => Poll::Ready(Some(())),
                    Poll::Pending => Poll::Pending,
                }
            }
            None => {
                warn!(
                    "Something is wrong: poll_next got None event. Try investigate this mystery..."
                );
                Poll::Pending
            }
        }
    }
}
