use std::io::{Error as IoError};

use futures::future;
use futures::sync::oneshot;
use futures::{Future, Stream, Poll, Async};

use tokio_proto::streaming::{Body, Message};
use tokio_proto::util::client_proxy::{self, Receiver};

use tokio_smtp::response::{Response as SmtpResponse};
use tokio_smtp::request::{Request as SmtpRequest};

use super::service::{TokioSmtpService, SmtpSetup, StopServiceHandle};
use mail::default_impl::simple_context;
use mail::prelude::*;
use std::convert::From;

pub(crate) struct FakeSmtpServer {
    rx: Receiver<
        Message<SmtpRequest, Body<Vec<u8>, IoError>>,
        Message<SmtpResponse, Body<(), IoError>>,
        IoError>,
    expected_requests: Vec<RequestMock>,
    use_responses: Vec<ResponseMock>,
    stop_flag: StopServiceHandle
}

pub(crate) type ResponseMock = Result<SmtpResponse, IoError>;

pub(crate) enum RequestMock {
    Normal(SmtpRequest),
    Body(SmtpRequest, Vec<u8>)
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TestError(pub(crate) String);

impl From<String> for TestError {
    fn from(inp: String) -> Self {
        TestError(inp)
    }
}

impl<'a> From<&'a str> for TestError {
    fn from(inp: &'a str) -> Self {
        TestError(inp.to_owned())
    }
}


impl FakeSmtpServer {

    pub(crate) fn new(
        expected_requests: Vec<RequestMock>,
        use_responses: Vec<Result<SmtpResponse, IoError>>
    ) -> (TokioSmtpService, Self)
    {
        let (proxy, rx) = client_proxy::pair();
        let mut expected_requests = expected_requests;
        expected_requests.reverse();
        let mut use_responses = use_responses;
        use_responses.reverse();
        let _self = FakeSmtpServer {
            rx, expected_requests, use_responses,
            stop_flag: StopServiceHandle::new()
        };
        (proxy, _self)
    }

    pub(crate) fn get_stop_flag(&self) -> StopServiceHandle {
        self.stop_flag.clone()
    }

    fn get_next_expected(&mut self, got: &SmtpRequest) -> Result<RequestMock, TestError> {
        if let Some(expected) = self.expected_requests.pop() {
            Ok(expected)
        } else {
            Err(TestError(
                format!("[test] got request but expected no more requests, got: {:?}", got)))
        }
    }
    fn check_next_request(&mut self, req: SmtpRequest) -> Result<(), TestError> {
        let expected = self.get_next_expected(&req)?;

        let (expected, expected_body) = match expected {
            RequestMock::Normal(req) => (req, false),
            RequestMock::Body(req, _) => (req, true)
        };

        if req != expected {
            return Err(
                TestError(format!("[test] expected req and received req differ: {:?} != {:?}",
                                  expected, req)));
        }

        if expected_body {
            Err(TestError(format!("[test] expected with body, got: {:?}", req)))
        } else {
            Ok(())
        }
    }

    //NOTE: only call during a poll or it will panic
    fn check_next_request_with_body(
        &mut self,
        req: SmtpRequest, mut body: Body<Vec<u8>, IoError>
    ) -> Result<(), TestError>
    {
        let expected = self.get_next_expected(&req)?;

        let (expected, expected_body) = match expected {
            RequestMock::Normal(_exp) => {
                return Err(TestError(format!("[test] expected req without body, got: {:?}", req)));
            },
            RequestMock::Body(exp, body) => {
                (exp, body)
            }
        };

        if req != expected {
            return Err(
                TestError(format!("[test] expected req and received req differ: {:?} != {:?}",
                                  expected, req)));
        }


        let body_bytes =
            match body.poll() {
                Ok(Async::Ready(Some(data))) => data,
                Ok(Async::Ready(None)) =>
                    panic!("[TEST_BUG] Body::from(data) is used so data should be ready"),
                Ok(Async::NotReady) =>
                    panic!("[TEST_BUG] Body::from(data) is used so data should be ready"),
                Err(_) =>
                    panic!("[TEST_BUG] Body::from(data) is used so no error can occure")
            };

        if expected_body != body_bytes {
            let readable_exp_body = String::from_utf8_lossy(&expected_body);
            let readable_body = String::from_utf8_lossy(&body_bytes);
            Err(TestError(
                format!("unexpected body, got {:?} expected {:?}",
                        readable_body, readable_exp_body)))
        } else {
            Ok(())
        }
    }

    fn send_next_response(
        &mut self,
        tx: oneshot::Sender<Result<Message<SmtpResponse, Body<(), IoError>>, IoError>>
    ) -> Result<(), TestError>
    {
        let next_response = match self.use_responses.pop() {
            Some(resp) => resp,
            None => return Err(TestError("[test] run out of responses".to_owned()))
        };

        tx.send(next_response.map(|res| Message::WithoutBody(res)))
            .map_err(|_|
                TestError("[test] Smtp ClientProxy call response future dropped early".to_owned()))
    }
}

impl Future for FakeSmtpServer {
    type Item = ();
    type Error = TestError;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {

            if self.stop_flag.should_stop() {
                return Ok(Async::Ready(()));
            }

            let item = try_ready! {
                self.rx.poll()
                    .map_err(|_| TestError::from("[test] smtp mock service channel closed in poll"))
            };

            let io_res = match item {
                Some(item) => item,
                None => {
                    if !self.expected_requests.is_empty() {
                        return Err(TestError(
                            "[test] receiver 'closed' but expected more requests".to_owned()));
                    } else {
                        return Ok(Async::Ready(()));
                    }
                }
            };

            let (req, tx) = io_res
                .expect("[TEST_BUG] unexpected io error send form ClinetProxy");

            match req {
                Message::WithoutBody(req) => {
                    self.check_next_request(req)?;
                },
                Message::WithBody(req, body) => {
                    self.check_next_request_with_body(req, body)?;
                }
            }
            self.send_next_response(tx)?;
        }
    }

}

pub(crate) struct TestSetup {
    nr_of_ok_connection_attemps: usize,
    client_proxy: TokioSmtpService,
    context: simple_context::Context
}

impl TestSetup {

    pub(crate)  fn new(
        nr_of_ok_connection_attemps: usize,
        expected_requests: Vec<RequestMock>,
        use_responses: Vec<Result<SmtpResponse, IoError>>
    ) -> (Self, FakeSmtpServer)
    {
        let (client_proxy, fake_driver) = FakeSmtpServer::new(expected_requests, use_responses);

        let _self = TestSetup {
            nr_of_ok_connection_attemps, client_proxy,
            context: simple_context::new().unwrap()
        };
        (_self, fake_driver)
    }

}




#[derive(Debug, PartialEq)]
pub(crate) enum ConnectionError {
    RunOutOfConnection
}


impl SmtpSetup for TestSetup {

    type ConnectFuture = future::FutureResult<TokioSmtpService, Self::NotConnectingError>;
    type NotConnectingError = ConnectionError;
    type BuilderContext = simple_context::Context;

    fn connect(&mut self) -> Self::ConnectFuture {
        let left_attemps = self.nr_of_ok_connection_attemps;
        if left_attemps < 1 {
            future::err(ConnectionError::RunOutOfConnection)
        } else {
            self.nr_of_ok_connection_attemps = left_attemps - 1;
            future::ok(self.client_proxy.clone())
        }
    }

    fn context(&self) -> Self::BuilderContext {
        self.context.clone()
    }

}


pub(crate) fn text_resource<I: Into<String>>(text: I) -> Resource {
    let fb = FileBuffer::new(MediaType::new("text", "plain").unwrap(), text.into().into_bytes());
    Resource::sourceless_from_buffer(fb)
}