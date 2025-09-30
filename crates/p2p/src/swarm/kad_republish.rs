//! This is essentially a coroutine written in terms of `Stream`.
//! The task is to republish our own record.
//! The logic for when to do that is entirely in this file.
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

// Apparently it seems we need P2P to be Send, because its task can
// run on different threads, therefore some Arcs and Mutexes are used.
pub(crate) struct KadRepublishStream {
    waker: Arc<AtomicWaker>,
    why_this_has_been_polled: Arc<Mutex<Option<WhyThisIsPolled>>>,
    interval: Option<Interval>,
    dur: Duration,
    dur_fail: Duration,
    is_routing_updated_first_time: bool,
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

// This is a state machine.
//
// Logic behind implementation: either waker.wake() will trigger tokio to poll this, or interval's
// waker via "forwarding" context inside of `interval.poll_tick(cx)`.
impl Stream for KadRepublishStream {
    type Item = ();

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let self_mut = self.get_mut();
        let state = Arc::get_mut(&mut self_mut.why_this_has_been_polled).unwrap();
        match state.get_mut() {
            Some(WhyThisIsPolled::FirstTime) => {
                trace!("FirstTime");
                self_mut.waker.register(cx.waker());
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
