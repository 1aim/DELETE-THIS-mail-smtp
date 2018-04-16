use std::io::{Error as IoError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::mem;

use futures::sync::{mpsc, oneshot};
use futures::{future, Future, Stream, Poll, Async};
use futures::stream::Peekable;

use new_tokio_smtp::Connection;

use mail::prelude::{Encoder, Encodable, MailType};
use mail::utils::SendBoxFuture;
use mail::context::BuilderContext;

use super::smtp_wrapper::{send_mail, ConnectionState, CompletionState};
use super::encode::{MailEncodingResult, stream_encode_mail};
use super::common::{MailResponse, MailRequest, EnvelopData, MailSendResult};
use super::handle::MailServiceHandle;
use super::error::MailSendError;
use super::stop_handle::StopServiceHandle;
use super::mpsc_ext::AutoClose;


pub trait SmtpSetup {

    /// The future returned which returns a Smtp connection,
    ///
    /// as this smtp mail bindings are writting for `tokio_smtp`
    /// the Item is fixed to `ClientProxy`, (This might change
    /// in future versions)
    type ConnectFuture: 'static + Future<
        Item=Connection,
        Error=Self::NotConnectingError>;

    /// The error returned if it is not possible to connect,
    /// this might represent a direct connection failure or
    /// one involfing multiple retries or similar aspects.
    type NotConnectingError;

    type BuilderContext: BuilderContext;

    // this future can contain all kind of retry connection handling etc.
    /// This method is called to connect with an SMTP server.
    ///
    /// It is called whenever connecting to a SMTP server is necessary,
    /// this includes the initial connection as well as reconnecting after
    /// the connection might no longer be usable.
    ///
    /// As it returns a future with it's own error it can be used to
    /// handle automatically retrying failed connections and limiting
    /// the amount of retries or having a timeout before retrying to
    /// connect.
    ///
    //TODO
    /// Currently it is not implemented to retry sending failed mails, even
    /// if it reconnects after e.g. an IO error
    fn connect(&mut self) -> Self::ConnectFuture;

    fn context(&self) -> Self::BuilderContext;

    /// return how many mail should be encoded at the same time
    ///
    /// encoding a `Mail` includes transforming it into an `EncodableMail` which means
    /// loading all resources associated with the `Mail`
    fn mail_encoding_buffer_size(&self) -> usize { 16 }

    /// return the buffer size for the mpsc channel between the service and it's handles
    ///
    /// By default each handle has one and the loading buffer is directly connected to the
    /// receiver, but the difference between the buffers is that sender can write into the
    /// mpsc channels buffer _in their thread_ while moving the data buffered in the mpsc
    /// channel to the `BufferUnordered` buffer is done _while polling the service driver_.
    fn mail_enqueuing_buffer_size(&self) -> usize { 16 }
}



pub struct MailService<SUP>
    where SUP: SmtpSetup
{
    setup: SUP,
    //FIXME[future >= 0.2]: use Never
    //FIXME[rust/impl Trait+abstract type]: use impl Trait/abstract type for rx
    rx: Peekable<Box<Stream<Item=(MailEncodingResult, oneshot::Sender<MailSendResult>), Error=()>>>,
    connection: ConnectionState<SUP::ConnectFuture>,
    tx_of_pending: Option<oneshot::Sender<MailSendResult>>,
    stop_handle: StopServiceHandle
}


impl<SUP> MailService<SUP>
    where SUP: SmtpSetup
{

    pub fn new(setup: SUP) -> (Self, MailServiceHandle) {
        let ctx = setup.context();
        let stop_handle = StopServiceHandle::new();

        let (tx, raw_rx) = mpsc::channel(setup.mail_enqueuing_buffer_size());
        let auto_close_rx = AutoClose::new(raw_rx, stop_handle.clone());
        let enc_rx = stream_encode_mail(raw_rx, ctx, setup.mail_encoding_buffer_size());
        let rx = enc_rx.peekable();

        let driver = MailService {
            setup, rx,
            connection: ConnectionState::Idle,
            tx_of_pending: None,
            stop_handle: StopServiceHandle::new()
        };

        let handle = MailServiceHandle::new(tx);
        (driver, handle)
    }

    pub fn stop_handle(&self) -> StopServiceHandle {
        self.stop_handle.clone()
    }

    /// # Error
    ///
    /// returns an error if the connection is "in use"
    fn start_stopping_now(&mut self) -> Result<(), ()> {
        self.stop_handle.stop();
        self.connection.terminate()
    }

    fn poll_next_request(&mut self) -> Async<()> {
        // 2. try to get a new request
        let (enc_result, req_tx) = match self.rx.poll() {
            Ok(Async::Ready(Some(item))) => {
                item
            },
            Ok(Async::Ready(None)) => {
                //stop ourself, all senders where closed
                self.start_stopping_now()
                    .expect("[BUG] we try_ready early return on \"in use\" connections");
                return Async::Ready(());
            },
            Ok(Async::NotReady) => {
                return Ok(Async::NotReady)
            },
            Err(_) => unreachable!("mpsc::Receiver.poll does not error")
        };

        // 3. start transmitting new request / send error back
        match enc_result {
            Ok((body, envelop)) =>  {
                self.connection.send_mail(data, envelop)
                    .expect("[BUG] we can only reach here if the connection is \"Usable\"" );
                self.tx_of_pending = Some(req_tx);
            },
            Err(err) => {
                let _ = req_tx.send(err);
            }
        }

        Async::Ready(())
    }
}

impl<SUP> Future for MailService<SUP>
    where SUP: SmtpSetup
{
    type Item = ();
    type Error = SUP::NotConnectingError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        //TODO[futures+tokio/new timout api]: use timeout and then close open connection
        loop {
            // 1. poll the connections state
            match try_ready!(self.connection.poll_state_completion().map(|err| TODO)) {
                CompletionState::Usable(opt_mail_send_result) => {
                    if let Some(mail_result) = opt_mail_send_result {
                        let tx = self.tx_of_pending.take().expect("[BUG] pending result but no tx");
                        // we do not care if the receiver is now dropped
                        let _ = tx.send(mail_result);
                    }

                    match self.poll_next_request() {
                        Async::Ready(()) => (),
                        Async::NotReady => return Ok(Async::NotReady)
                    }

                },
                CompletionState::Idle => {
                    let peek = try_ready!(self.rx.peek());
                    // only open connection if a mail request is pending
                    if peek.is_some() {
                        self.connection.change_into_connecting(self.setup.connect());
                    } else {
                        self.start_stopping_now()
                            .expect("[BUG] we try_ready early return on \"in use\" connections");
                    }
                }
                CompletionState::Terminated => {
                    // be robust and make sure the flag is set anyway, through
                    // it should not be possible to end up here without stop_handle
                    // being set
                    self.stop_handle.stop();
                    return Ok(Async::Ready(()));
                }
            }
        }
    }
}






#[cfg(test)]
mod test {
    use std::io;
    use futures::{Future, IntoFuture};
    use mail::prelude::*;
    use chrono::{Utc, TimeZone};
    use super::super::test::*;
    use super::*;


    fn _test<F, R, S, D>(setup: S, fail_connect: bool, func: F, other_driver: D)
        where S: SmtpSetup,
              F: FnOnce(MailServiceHandle) -> R,
              R: IntoFuture<Item=(), Error=TestError>,
              D: Future<Item=(), Error=TestError>
    {
        let (driver, handle) = MailService::new(setup);
        let stop_handle = driver.stop_handle();

        let test_fut = func(handle)
            .into_future()
            .then(|res| {
                stop_handle.stop();
                res
            });

        let driver = driver.then(|res| match res {
            Ok(_) if fail_connect =>
                Err(TestError("[test] did not fail to connect".to_owned())),
            Err(_) if !fail_connect =>
                Err(TestError("[test] did unexpected fail to connect".to_owned())),
            _ => Ok(())
        });

        // we want all futures to complete independent of errors
        // so there errors get "lifted" into their item
        let driver = driver.then(|res| Ok(res));
        let test_fut = test_fut.then(|res| Ok(res));
        let other_driver = other_driver.then(|res| Ok(res));

        let res: Result<_, ()> = driver.join3(test_fut, other_driver).wait();
        let (rd, rt, rod) = res.unwrap();
        match rd { Ok(_) => {}, Err(TestError(msg)) => panic!(msg) }
        match rt { Ok(_) => {}, Err(TestError(msg)) => panic!(msg) }
        match rod { Ok(_) => {}, Err(TestError(msg)) => panic!(msg) }
    }

    fn example_io_error() -> IoError {
        IoError::new(io::ErrorKind::Other, "it broke")
    }

    fn example_mail() -> (MailRequest, &'static str) {
        let headers = headers! {
            From: ["djinns@are.magic"],
            To: ["lord.of@the.bottle"],
            Subject: "empty bottle, no djinn",
            Date: Utc.ymd(2023, 1, 1).and_hms(1, 1, 1)
        }.unwrap();

        let mail = Builder
        ::singlepart(text_resource("<--body-->"))
            .headers(headers).unwrap()
            .build().unwrap();

        let req = MailRequest::new(mail);

        let expected_body = concat!(
            "MIME-Version: 1.0\r\n",
            "From: <djinns@are.magic>\r\n",
            "To: <lord.of@the.bottle>\r\n",
            "Subject: empty bottle, no djinn\r\n",
            "Date: Sun,  1 Jan 2023 01:01:01 +0000\r\n",
            "Content-Type: text/plain\r\n",
            "Content-Transfer-Encoding: 7bit\r\n",
            "\r\n",
            "<--body-->\r\n"
        );

        (req, expected_body)

    }
    #[test]
    fn send_simple_mail() {
        use self::RequestMock::*;
        let (req, expected_body) = example_mail();
        let (setup, fake_server) = TestSetup::new(1,
            vec![
                Normal(SmtpRequest::Mail {
                    from: "djinns@are.magic".parse().unwrap(), params: Vec::new() }),
                Normal(SmtpRequest::Rcpt {
                    to: "lord.of@the.bottle".parse().unwrap(), params: Vec::new() }),
                Body(SmtpRequest::Data, expected_body.to_owned().into_bytes()),
                Normal(SmtpRequest::Quit)
            ],
            vec![
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
            ]
        );

        _test(setup, false, |handle| {
            handle.send_mail(req)
                .and_then(|(_handle, resp_fut)| resp_fut)
                .and_then(|_resp: MailResponse| {
                    //MailResponse is currently zero sized, so nothing to do here
                    Ok(())
                })
                .map_err(|mse| TestError(format!("unexpected error: {:?}", mse)))
        }, fake_server)
    }

    #[test]
    fn reset_connection_on_io_error() {
        use self::RequestMock::*;
        let (req, expected_body) = example_mail();
        let (setup, fake_server) = TestSetup::new(2,
            vec![
                Normal(SmtpRequest::Mail {
                    from: "djinns@are.magic".parse().unwrap(), params: Vec::new() }),
                // currently we only check for errs after sending all non Data parts
                Normal(SmtpRequest::Rcpt {
                    to: "lord.of@the.bottle".parse().unwrap(), params: Vec::new() }),
                Normal(SmtpRequest::Mail {
                    from: "djinns@are.magic".parse().unwrap(), params: Vec::new() }),
                Normal(SmtpRequest::Rcpt {
                    to: "lord.of@the.bottle".parse().unwrap(), params: Vec::new() }),
                Body(SmtpRequest::Data, expected_body.to_owned().into_bytes()),
                Normal(SmtpRequest::Quit)
            ],
            vec![
                Err(example_io_error()),
                Err(example_io_error()),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
                Ok(SmtpResponse::parse(b"250 Ok\r\n").unwrap().1),
            ]
        );

        _test(setup, false, |handle| {
            handle.send_mail(req)
                .map_err(|err| TestError(format!("unexpected enque error {:?}", err)))
                .and_then(|(handle, resp_fut)| resp_fut.then(|res| match res {
                    Ok(MailResponse) => Err(TestError("[test] unexpected no error".to_owned())),
                    Err(err) => {
                        if let MailSendError::Io(_) = err {
                            Ok(handle)
                        } else {
                            Err(TestError(format!("unexpected error kind {:?}", err)))
                        }
                    }
                }))
                .and_then(|handle| {
                    let (req, _) = example_mail();
                    handle.send_mail(req)
                        .map_err(|err| TestError(format!("unexpected enque error {:?}", err)))
                })
                .and_then(|(_handle, res_fut)| {
                    res_fut.map_err(|err| TestError(format!("unexpected error {:?}", err)))
                })
                .map(|_| ())
        }, fake_server)
    }

    #[test]
    fn failed_reset_connection() {
        use self::RequestMock::*;
        let (req, _) = example_mail();
        let (setup, fake_server) = TestSetup::new(1,
            vec![
                Normal(SmtpRequest::Mail {
                    from: "djinns@are.magic".parse().unwrap(), params: Vec::new() }),
                // currently we only check for errs after sending all non Data parts
                Normal(SmtpRequest::Rcpt {
                    to: "lord.of@the.bottle".parse().unwrap(), params: Vec::new() }),
            ],
            vec![
                Err(example_io_error()),
                Err(example_io_error()),
            ]
        );

        _test(setup, true, |handle| {
            handle.send_mail(req)
                .and_then(|(_handle, res_fut)| res_fut)
                .then(|res| match res {
                    Ok(_) => Err(TestError("unexpected no error".to_owned())),
                    Err(err) => {
                        if let MailSendError::Io(_) = err {
                            Ok(())
                        } else {
                            Err(TestError(format!("unexpected error kind: {:?}", err)))
                        }
                    }
                })
        }, fake_server)

    }
}