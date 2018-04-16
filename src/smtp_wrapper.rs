use std::io as std_io;
use std::mem;
use std::sync::Mutex;

use futures::future::{self, Loop, Either};
use futures::{Future, Poll, Async};

use new_tokio_smtp::{
    command,
    Connection,
};
use new_tokio_smtp::io::Socket;
use new_tokio_smtp::chain::{chain, OnError};

use super::error::MailSendError;
use super::common::{EnvelopData, MailResponse};

//FIXME[rust/impl Trait + abstract type]: use abstract type
type SmtpMailSendFuture = Box<Future<
    Item=(Connection, Result<MailResponse, MailSendError>),
    Error=MailSendError>>;

//FIXME[rust/impl Trait]: use impl Trait
pub(crate) fn send_mail(con: Connection, body_bytes: Vec<u8>, envelop: EnvelopData)
    -> SmtpMailSendFuture
{
    let (from, tos) = envelop.split();
    let mut cmds = vec![command::Mail::new(from).boxed()];

    for to in tos.into_iter() {
        cmds.push(command::Recipient::new(to).boxed());
    }

    let fut = chain(con, cmds, OnError::StopAndReset)
        .map_err(|err| MailSendError::Io(err))
        .map(|(con, result)| match result {
            Ok(_) => (con, Ok(MailResponse)),
            Err((_, err)) => (con, Err(MailSendError::Smtp(err)))
        });

    Box::new(fut)
}

pub enum ConnectionState<F> {
    Idle,
    Connecting(F),
    Connected(Connection),
    ConnectionInUse(SmtpMailSendFuture),
    Closing { 
        fut: Box<Future<Item=Socket, Error=std_io::Error>>,
        is_termination: bool
    },
    Terminated,
    Poison
}

pub enum CompletionState {
    Usable(Option<Result<MailResponse, MailSendError>>),
    Idle,
    Terminated
}

impl<F> ConnectionState<F>
    where F: Future<Item=Connection>
{

    pub fn change_into_connecting(&mut self, con_fut: F) {
        let old = mem::replace(self, ConnectionState::Connecting(con_fut));
        if let ConnectionState::Poison = old {
            panic!("reuse of poisoned state in ConnectionState");
        }
    }

    pub fn poll_state_completion(&mut self)
        -> Poll<CompletionState, Either<std_io::Error, F::Error>>
    {
        use self::ConnectionState::*;
        use self::CompletionState::Usable;
        let state = mem::replace(self, Poison);

        let mut new_state = None;
        let (new_state, result) =
            match state {
                Idle => {
                    (Idle, Ok(Async::Ready(CompletionState::Idle)))
                },
                Connected(con) => {
                    (Connected(con), Ok(Async::Ready(Usable(None))))
                },
                Connecting(mut fut) => match fut.poll() {
                    Ok(Async::NotReady) => (Connecting(fut), Ok(Async::NotReady)),
                    Ok(Async::Ready(con)) => {
                        (Connected(con), Ok(Async::Ready(Usable(None))))
                    },
                    Err(err) => (Terminated, Err(Either::B(err)))
                },
                ConnectionInUse(mut fut) => match fut.poll() {
                    Ok(Async::NotReady) => (ConnectionInUse(fut), Ok(Async::NotReady)),
                    Ok(Async::Ready((con, result))) => {
                        (Connected(con), Ok(Async::Ready(Usable(Some(result)))))
                    },
                    Err(err) => (Terminated, Err(Either::A(err)))
                },
                Closing{ mut fut, is_termination } => match fut.poll() {
                    Ok(Async::NotReady) => (Closing {fut, is_termination }, Ok(Async::NotReady)),
                    Ok(Async::Ready(())) => {
                        if is_termination {
                            (Terminated, Ok(Async::Ready(CompletionState::Terminated)))
                        } else {
                            (Idle, Ok(Async::Ready(CompletionState::Idle)))
                        }
                    }
                },
                Terminated => (Terminated, Ok(Async::Ready(CompletionState::Terminated))),
                Poison => panic!("polled ConnectionState after it was poisoned")
            };

        *self = new_state;
        result
    }

    pub fn send_mail(&mut self, body_bytes: Vec<u8>, envelop: EnvelopData)
        -> Result<(), (Vec<u8>, EnvelopData)>
    {
        use self::ConnectionState::*;

        let state = mem::replace(self, Poison);
        let (state, result) =
            match state {
                state @ Idle | Terminated | Connecting(_) | ConnectionInUse(_) | Closing(_) =>
                    (state, Err((body_bytes, envelop))),
                Poison => panic!("used ConnectionState after it was poisoned"),
                Connected(con) => {
                    let in_use_fut = send_mail(con, body_bytes, envelop);
                    (ConnectionInUse(in_use_fut), Ok(()))
                }
            };

        *self = state;
        result
    }

    /// # Panic
    ///
    /// panics if it's either poisoned or if the connections is "in use", i.e.
    /// there is currently in the process of beeing send.
    pub fn close_current(&mut self) -> Result<(), ()> {
        self._close_con(false)
        
    }

    /// # Panic
    ///
    /// panics if it's either poisoned or if the connections is "in use", i.e.
    /// there is currently in the process of beeing send.
    pub fn terminate(&mut self) -> Result<(), ()> {
        self._close_con(true)
    }

    fn _close_con(&mut self, is_termination: bool) -> Result<(), ()> {
        use self::ConnectionState::*;

        let mut result = Ok(());
        let force_termination = is_termination;
        let state = mem::replace(self, Poison);
        *self =
            match state {
                Idle => {
                    if is_termination {
                        Terminated
                    } else {
                        Idle
                    }
                },
                Connecting(fut) => Closing {
                    fut: Box::new(fut.and_then(|con| con.quit())),
                    is_termination
                },
                Connected(con) => Closing {
                    fut: Box::new(con.quit()),
                    is_termination
                },
                ConnectionInUse(fut) => {
                    result = Err(());
                    ConnectionInUse(fut)
                },
                Closing { fut, is_termination } => {
                    // terminating overides quiting but not the other way around
                    if force_termination {
                        Closing{ fut, is_termination: true }
                    } else {
                        Closing{ fut, is_termination }
                    }
                },
                Terminated => Terminated,
                Poison => panic!("used ConnectionState after it was poisoned")
            };

        result
    }
}

