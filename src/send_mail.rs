//! Module implementing mail sending using `new-tokio-smtp::send_mail`.

use std::iter::{once as one};

use futures::{
    stream::{self, Stream},
    future::{self, Future, Either}
};

use mail_common::{
    MailType,
    encoder::EncodingBuffer
};
use mail::Context;

use new_tokio_smtp::{
    ConnectionConfig,
    Cmd,
    SetupTls,
    send_mail::MailEnvelop,
    Connection,
    send_mail as smtp
};

use ::{
    error::MailSendError,
    request::MailRequest
};

pub fn send<A, S>(mail: MailRequest, conconf: ConnectionConfig<A, S>, ctx: impl Context)
    -> impl Future<Item=(), Error=MailSendError>
    where A: Cmd, S: SetupTls
{
    let fut = encode(mail, ctx)
        .then(move |envelop_res| Connection
            ::connect_send_quit(conconf, one(envelop_res))
            .collect())
        .map(|mut results| results.pop().expect("[BUG] sending one mail expects one result"));

    fut
}

pub fn send_batch<A, S, C>(
    mails: Vec<MailRequest>,
    conconf: ConnectionConfig<A, S>,
    ctx: C
) -> impl Stream<Item=(), Error=MailSendError>
    where A: Cmd, S: SetupTls, C: Context
{
    let iter = mails.into_iter().map(move |mail| encode(mail, ctx.clone()));

    let fut = collect_res(stream::futures_ordered(iter))
        .map(move |vec_of_res| Connection::connect_send_quit(conconf, vec_of_res))
        .flatten_stream();

    fut
}

//FIXME[futures/v>=0.2] use Error=Never
fn collect_res<S, E>(stream: S) -> impl Future<Item=Vec<Result<S::Item, S::Error>>, Error=E>
    where S: Stream
{
    stream.then(|res| Ok(res)).collect()
}


pub fn encode<C>(request: MailRequest, ctx: C)
    -> impl Future<Item=MailEnvelop, Error=MailSendError>
    where C: Context
{
    let (mail, envelop_data) =
        match request.into_mail_with_envelop() {
            Ok(pair) => pair,
            Err(e) => return Either::A(future::err(e.into()))
        };

    let fut = mail
        .into_encodeable_mail(ctx.clone())
        .and_then(move |enc_mail| ctx.offload_fn(move || {
            let (mail_type, requirement) =
                if envelop_data.needs_smtputf8() {
                    (MailType::Internationalized, smtp::EncodingRequirement::Smtputf8)
                } else {
                    (MailType::Ascii, smtp::EncodingRequirement::None)
                };

            let mut buffer = EncodingBuffer::new(mail_type);
            enc_mail.encode(&mut buffer)?;

            let vec_buffer: Vec<_> = buffer.into();
            let smtp_mail = smtp::Mail::new(requirement, vec_buffer);

            Ok(smtp::MailEnvelop::from((smtp_mail, envelop_data)))
        }))
        .map_err(MailSendError::from);

    Either::B(fut)
}