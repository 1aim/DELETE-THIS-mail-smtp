use futures::sync::mpsc;
use futures::sync::oneshot;
use futures::{sink, Sink, Future, Poll, Async};

use super::common::{MailRequest, MailResponse};
use super::error::MailSendError;

type InnerChannel = mpsc::Sender<(MailRequest, oneshot::Sender<Result<MailResponse, MailSendError>>)>;

#[derive(Debug, Clone)]
pub struct MailServiceHandle {
    channel: InnerChannel
}


impl MailServiceHandle {

    pub(crate) fn new(sender: InnerChannel) -> Self {
        MailServiceHandle { channel: sender }
    }

    pub fn send_mail(self, mail_request: MailRequest) -> MailEnqueueFuture {
        let (sender, rx) = oneshot::channel();

        let send = self.channel.send((mail_request, sender));

        MailEnqueueFuture { send, rx: Some(rx) }
    }

//    pub fn map_request_stream<S>(self, stream: S, max_buffer: Option<usize>) -> SmtpMailStream<RQ, S>
//        where S: Stream<Item = RQ>,
//              S::Error: Into<MailSendError>
//    {
//        SmtpMailStream::new(self.channel, stream, max_buffer)
//    }

    pub fn into_inner(self) -> InnerChannel {
        self.channel
    }

}

pub struct MailEnqueueFuture {
    send: sink::Send<InnerChannel>,
    rx: Option<oneshot::Receiver<Result<MailResponse, MailSendError>>>
}

impl Future for MailEnqueueFuture {

    type Item = (MailServiceHandle, MailResponseFuture);
    type Error = MailSendError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let channel = match self.send.poll() {
            Ok(Async::Ready(channel)) => channel,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(_cancel_err) => return Err(MailSendError::DriverDropped),
        };

        let rx = self.rx.take().expect("called poll after polling completed");
        Ok(Async::Ready((MailServiceHandle { channel }, MailResponseFuture { rx })))
    }
}


pub struct MailResponseFuture {
    rx: oneshot::Receiver<Result<MailResponse, MailSendError>>
}

impl Future for MailResponseFuture  {
    type Item = MailResponse;
    type Error = MailSendError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let res = match self.rx.poll() {
            Ok(Async::Ready(res)) => res,
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Err(_cancel_err) => return Err(MailSendError::CanceledByDriver),
        };

        match res {
            Ok(resp) => Ok(Async::Ready(resp)),
            Err(err) => Err(err)
        }
    }
}


// use a closure to setup smtp Clone + FnMut() -> R, R: Future<Item=...,> or similar
//fn setup_smtp(smtp_connect SmtpSetup) -> (Sender, impl Future<(), SendError>) {
//    let (send, resc) = mpcs::channel(XX);
//    let shared_composition_base = smtp_setup.composition_base;
//    let driver = smtp_connect().and_then(|service| {
//        let service = shared_composition_base.wrap_smtp_service(service);
//
//        let pipe_in = resc;;
//
//        pipe_in.for_each(|(cmd, pipe_out)| {
//            service.call(cmd).and_then(|res| {
//                let _ = pipe_out.send(res);
//                Ok(())
//            })
//        })
//    });
//    (send, driver)
//}
//
//fn setup_smtp_with_retry(setup: SmtpSetup) -> impl Future<(),RetryFail> {
//    let driver = setup_smtp(setup.clone());
//
//    future::loop_fn(driver, move |driver| {
//        driver.or_else(|err| {
//            if err.was_canceled() {
//                Ok(Loop::Break(()))
//            } else if err.can_recover() {
//                let new_driver = setup_smtp(setup.clone());
//                Ok(Loop::Continue(new_driver))
//            } else {
//                Err(err.into())
//            }
//        })
//    })
//}
//



// TODO: This links into a stream mapping a stream of requests to a stream of responses,
// but it needs more though into it, i.e. it decouples the sending of a mail, with
// the response of getting one. So we need to at last add some mail Id to Req (we
// might want to see if we can generally do so using MessageId).
//
//pub struct SmtpMailStream<RQ, S>
//    where RQ: MailSendRequest,
//          S: Stream<Item = RQ>,
//          S::Error: Into<MailSendError>
//{
//    channel: Option<InnerChannel<RQ>>,
//    stream: Option<S>,
//    res_buffer: FuturesUnordered<oneshot::Receiver<Result<MailResponse, MailSendError>>>,
//    send_buffer: Option<(RQ, oneshot::Sender<Result<MailResponse, MailSendError>>)>,
//    max_buffer: Option<usize>
//}
//
//
//impl<RQ, S> SmtpMailStream<RQ, S>
//    where RQ: MailSendRequest,
//          S: Stream<Item = RQ>,
//          S::Error: Into<MailSendError>
//{
//    pub(crate) fn new(channel: InnerChannel<RQ>, stream: S, max_buffer: Option<usize>) -> Self {
//        SmtpMailStream {
//            channel, stream, max_buffer,
//            send_buffer: None,
//            res_buffer: FuturesUnordered::new(),
//        }
//    }
//
//    fn channel_mut(&mut self) -> &mut InnerChannel<RQ> {
//        self.channel.as_mut().unwrap()
//    }
//
//    fn stream_mut(&mut self) -> &mut S {
//        self.stream.as_mut().unwrap()
//    }
//
//    fn try_enqueue_mail(
//        &mut self,
//        msg: (RQ, oneshot::Sender<Result<MailResponse, MailSendError>>)
//    ) -> Poll<(), DriverDropped<S>>
//    {
//        debug_assert!(self.send_buffer.is_none());
//        debug_assert!(self.stream.is_some() && self.channel.is_some());
//        match self.channel_mut().start_send(msg) {
//            Ok(AsyncSink::Ready) => Ok(Async::Ready(())),
//            Ok(AsyncSink::NotReady(msg)) => {
//                self.send_buffer = Some(msg);
//                Ok(Async::NotReady)
//            },
//            Err(_) => {
//                mem::drop(self.channel.take());
//                Err(DriverDropped::Stream(self.stream.take()))
//            }
//        }
//    }
//
//    fn close_channel(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
//        try_ready!(self.channel_mut().close());
//        return Ok(Async::Ready(None));
//    }
//
//    fn poll_stream(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
//        match self.stream_mut().poll()? {
//            Async::Ready(Some(item)) => Async::Ready(Some(item)),
//            Async::Ready(None) => {
//                self.stream.take();
//                return self.close_channel();
//            }
//            Async::NotReady => {
//                try_ready!(self.channel_mut().poll_complete());
//                return Ok(Async::NotRead);
//            }
//        }
//    }
//}
//
//pub enum DriverDropped<S> {
//    Stream(S),
//    StreamTakenOnPreviousError
//}
//
//impl<RQ, S> Stream for SmtpMailStream<RQ, S>
//    where RQ: MailSendRequest,
//          S: Stream<Item = RQ>,
//          //FIXME[tokio 0.2] use error Never??
//          S::Error: Into<MailSendError>
//{
//    type Item = Result<MailResponse, MailSendError>;
//    type Error = DriverDropped<S>;
//
//    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
//        if self.channel.is_none() {
//            return Err(DriverDropped::StreamTakenOnPreviousError);
//        }
//
//        if self.stream.is_none() {
//            return self.close_channel();
//        }
//
//        if let Some(msg) = self.send_buffer {
//            try_ready!(self.try_enqueue_mail(msg));
//        }
//
//        loop {
//
//            match self.res_buffer.poll() {
//                Ok(Async::NotReady) => {},
//                Ok(Async::Ready(res)) => return Ok(AsyncReady(Ok(res))),
//                Err(err) => return Ok(AsyncReady(Err(e)))
//            }
//
//            if self.max_buffer.map(|max| self.res_buffer.len() >= max).unwrap_or_false() {
//                return Ok(Async::NotReady);
//            }
//
//
//            let item = try_some_ready!(self.poll_stream());
//
//            let (tx, rx) = oneshot::channel();
//
//            self.res_buffer.push(rx);
//
//            try_ready!(self.try_enqueue_mail((item, tx)));
//        }
//    }
//}