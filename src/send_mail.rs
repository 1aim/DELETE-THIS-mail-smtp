use std::iter::FromIterator;
use std::vec;

use futures::future::{self, Either, Loop, Future};
use new_tokio_smtp::{Cmd, Connection, ConnectionConfig, SetupTls};
use new_tokio_smtp::send_mail::{self as smtp, ConSendMailExt};

use mail_common::MailType;
use mail_common::encoder::EncodingBuffer;
use mail::context::BuilderContext;
use mail::error::MailError;

use ::resolve_all::ResolveAll;
use ::common::MailRequest;
use ::error::{MailSendError, TransportError};


/// Result of encoding a mail
pub type EncodeMailResult = Result<smtp::MailEnvelop, MailError>;

/// creates a futures which encodes all mails
///
/// To encode the mails this functions turns
/// mail every requests into mails with envelop data,
/// then creates a future resolving when the mail is
/// ready to be encoded and chain this result with
/// offloading the actual encoding of each mail
/// to a thread pool. Lastly all fo this futures
/// are polled in parallel by the returned future.
///
/// # Error
///
/// The futures will never error, but it will
/// resolve to a vector of results, representing
/// the encoding result for each mail in the input
/// separately
///
pub fn encode_mails<I, C>(requests: I, ctx: &C)
    //TODO[futures/v>=0.2 | rust/! type]: use Never or !
    -> impl Future<Item=Vec<EncodeMailResult>, Error=()> + Send
    where I: IntoIterator<Item=MailRequest>, C: BuilderContext
{
    let pending = requests
        .into_iter()
        .map(|request| {
            let (mail, envelop_data) =
                match request.into_mail_with_envelop() {
                    Ok(pair) => pair,
                    Err(e) => return Either::A(future::err(e.into()))
                };

            let _ctx = ctx.clone();
            let fut = mail
                .into_encodeable_mail(ctx)
                .and_then(move |enc_mail| _ctx.offload_fn(move || {
                    let (mail_type, requirement) =
                        if envelop_data.needs_smtputf8() {
                            (MailType::Internationalized, smtp::EncodingRequirement::Smtputf8)
                        } else {
                            (MailType::Ascii, smtp::EncodingRequirement::None)
                        };

                    let mut buffer = EncodingBuffer::new(mail_type);
                    enc_mail.encode(&mut buffer)?;

                    let smtp_mail = smtp::Mail::new(requirement, buffer.into());

                    Ok(smtp::MailEnvelop::from((smtp_mail, envelop_data)))
                }));

            Either::B(fut)
        });

    ResolveAll::from_iter(pending)
}

/// results of sending an encoded mail
pub type SendMailResult = Result<(), MailSendError>;

/// Sends all encoded mails through the given connection
///
/// This methods accepts a iterator of `EncodedMailResult`'s as it's
/// meant to be chained with `encode_mails`.
///
/// # Error
///
/// The returned future resolves to a vector of results, one for each mail
/// send.
///
/// If a transport error happens (e.g. an I/O-Error) a tuple consisting of
/// the Error, the already send mails and and iterator of the remaining mails is
/// returned.
pub fn send_encoded_mails<I>(con: Connection, mails: I)
    -> impl Future<
        Item=(Connection, Vec<SendMailResult>),
        Error=(TransportError, Vec<SendMailResult>, I::IntoIter)>
    where I: IntoIterator<Item=EncodeMailResult>, I::IntoIter: 'static
{
    let iter = mails.into_iter();
    let results = Vec::new();
    let fut = future::loop_fn((con, iter, results), |(con, mut iter, mut results)| match iter.next() {
        None => Either::A(future::ok(Loop::Break((con, results)))),
        Some(Err(err)) => {
            results.push(Err(MailSendError::from(err)));
            Either::A(future::ok(Loop::Continue((con, iter, results))))
        },
        Some(Ok(envelop)) => Either::B(con
            .send_mail(envelop)
            .then(move |res| match res {
                Ok((con, logic_result)) => {
                    results.push(logic_result.map_err(|(_idx, err)| MailSendError::from(err)));
                    Ok(Loop::Continue((con, iter, results)))
                },
                Err(err) => {
                    Err((TransportError::Io(err), results, iter))
                }
            }))
    });

    fut
}

/// Send mails _to a specific mail server_
///
/// This encodes the mails, opens a connection, sends the mails over and
/// closes the connection again.
///
/// While this uses the `To` field of a mail to determine the smtp reveiver
/// it does not resolve the server based on the mail address domain. This
/// means it's best suite for sending to a Mail Submission Agent (MSA), but
/// less for sending to a Mail Exchanger (MX).
///
/// Automatically handling Bcc/Cc is _not yet_ implemented.
///
pub fn send_mails<S, A, I, C>(config: ConnectionConfig<A, S>, requests: I, ctx: &C)
    -> impl Future<
        Item=Vec<SendMailResult>,
        Error=(TransportError, Vec<SendMailResult>, vec::IntoIter<EncodeMailResult>)>
    where I: IntoIterator<Item=MailRequest>,
          C: BuilderContext,
          S: SetupTls,
          A: Cmd
{

    let fut = encode_mails(requests, ctx)
        .map_err(|_| unreachable!())
        .and_then(|mails| {
            if mails.iter().all(|r| r.is_err()) {
                let send_skipped = mails
                    .into_iter()
                    .map(|result| match result {
                        Ok(_) => unreachable!(),
                        Err(err) => Err(MailSendError::Mail(err))
                    })
                    .collect();

                Either::A(future::ok(send_skipped))
            } else {
                let fut = Connection
                    ::connect(config)
                    .then(|result| match result {
                        Err(err) => Either::A(future::err((TransportError::Connecting(err), Vec::new(), mails.into_iter()))),
                        Ok(con) => Either::B(send_encoded_mails(con, mails))
                    })
                    .and_then(|(con, results)| con.quit().then(|_| Ok(results)));

                Either::B(fut)
            }
        });

    fut
}
