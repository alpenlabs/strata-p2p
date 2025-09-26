use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use futures::{Stream, lock::Mutex, task::AtomicWaker};
use tokio::time::{Instant, Interval, interval_at};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub(crate) enum WhyThisIsPolled {
    FirstTime,

    #[cfg_attr(not(feature = "kad"), expect(dead_code))]
    RoutingUpdated,

    #[cfg_attr(not(feature = "kad"), expect(dead_code))]
    PutRecordOk,

    #[cfg_attr(not(feature = "kad"), expect(dead_code))]
    PutRecordError,

    IntervalTriggered,
}

// Stream because it's the closest to coroutines.
pub(crate) struct KadRepublishStream {
    pub(crate) waker: Arc<AtomicWaker>,
    interval: Option<Interval>,
    dur: Duration,
    is_routing_updated_first_time: bool,
    previous_event: Option<WhyThisIsPolled>,
    pub(crate) why_this_has_been_polled: Arc<Mutex<Option<WhyThisIsPolled>>>,
}

impl KadRepublishStream {
    pub(crate) fn new(dur: Duration) -> Self {
        KadRepublishStream {
            waker: Arc::new(AtomicWaker::new()),
            interval: None,
            dur,
            is_routing_updated_first_time: true,
            previous_event: None,
            why_this_has_been_polled: Arc::new(Mutex::new(Some(WhyThisIsPolled::FirstTime))),
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
                debug!("FirstTime");
                self_mut.waker.register(cx.waker());
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());
                Poll::Pending
            }
            Some(WhyThisIsPolled::RoutingUpdated) => {
                debug!("RoutingUpdated");
                if self_mut.is_routing_updated_first_time {
                    debug!("RoutingUpdated first time");
                    self_mut.waker.register(cx.waker());
                    self_mut.is_routing_updated_first_time = false;
                    Poll::Ready(Some(()))
                } else {
                    debug!("RoutingUpdated not first time");
                    Poll::Pending
                }
            }
            Some(WhyThisIsPolled::PutRecordOk) => {
                debug!("PutRecordOk");
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
                debug!("PutRecordError");
                self_mut.previous_event = Some(state.get_mut().as_ref().unwrap().clone());
                *state.get_mut() = Some(WhyThisIsPolled::IntervalTriggered);

                self_mut.interval = Some(interval_at(
                    Instant::now() + Duration::from_secs(60),
                    self_mut.dur,
                ));
                let is_interval_says_it_should_be_triggered =
                    self_mut.interval.as_mut().unwrap().poll_tick(cx);

                match is_interval_says_it_should_be_triggered {
                    Poll::Ready(_) => Poll::Ready(Some(())),
                    Poll::Pending => Poll::Pending,
                }
            }
            Some(WhyThisIsPolled::IntervalTriggered) => {
                debug!("IntervalTriggered");
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
