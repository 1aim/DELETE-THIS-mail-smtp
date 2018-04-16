use super::stop_handle::StopServiceHandle;

use futures::sync::mpsc;
use futures::{Poll, Async, Stream};

pub struct AutoClose<I> {
    inner:  mpsc::Receiver<I>,
    stop_handle: StopServiceHandle,
}

impl<I> AutoClose<I>
    //FIXME[tokio >= 0.2]: use Never
    where I: Stream<Error=()>
{
    pub fn new(inner: mpsc::Receiver<I>, stop_handle: StopServiceHandle) -> Self {
        AutoClose { inner, stop_handle }
    }
}

impl<I> Stream for AutoClose<I>
    where I: Stream<Error=()>
{

    type Item = I::Item;
    //FIXME[tokio >= 0.2]: use Never
    type Error = ();

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if stop_handle.should_stop() {
            self.inner.close()
        }
        self.inner.poll()
    }
}