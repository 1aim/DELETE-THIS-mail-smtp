//! This modules contains some of the data types used, like e.g. Response, Request, Envelop etc.
use std::mem;

use new_tokio_smtp::send_mail::{
    self as smtp,
    MailAddress,
    EnvelopData
};


use mail_common::MailType;
use mail_common::encoder::{EncodingBuffer, EncodableInHeader};
use mail_common::error::EncodingError;
use headers::{Sender, _From, _To};
use headers::components::Mailbox;
use headers::error::BuildInValidationError;
use mail::Mail;
use mail::error::MailError;




// pub type MailSendResult = Result<MailResponse, MailSendError>;
// pub(crate) type Handle2ServiceMsg = (MailRequest, oneshot::Sender<MailSendResult>);

// #[derive(Debug, Clone)]
// pub struct MailResponse;

#[derive(Clone, Debug)]
pub struct MailRequest {
    mail: Mail,
    envelop_data: Option<EnvelopData>
}

impl From<Mail> for MailRequest {
    fn from(mail: Mail) -> Self {
        MailRequest::new(mail)
    }
}



impl MailRequest {

    pub fn new(mail: Mail) -> Self {
        MailRequest { mail, envelop_data: None }
    }

    pub fn new_with_envelop(mail: Mail, envelop: EnvelopData) -> Self {
        MailRequest { mail, envelop_data: Some(envelop) }
    }

    pub fn override_envelop(&mut self, envelop: EnvelopData) -> Option<EnvelopData> {
        mem::replace(&mut self.envelop_data, Some(envelop))
    }

    pub fn into_mail_with_envelop(self) -> Result<(Mail, EnvelopData), MailError> {
        let envelop =
            if let Some(envelop) = self.envelop_data { envelop }
            else { derive_envelop_data_from_mail(&self.mail)? };

        Ok((self.mail, envelop))
    }
}

fn mailaddress_from_mailbox(mailbox: &Mailbox) -> Result<MailAddress, EncodingError> {
    let email = &mailbox.email;
    let needs_smtputf8 = email.check_if_internationalized();
    let mt = if needs_smtputf8 { MailType::Internationalized } else { MailType::Ascii };
    let mut buffer = EncodingBuffer::new(mt);
    {
        email.encode(&mut buffer.writer())?;
    }
    let raw: Vec<u8> = buffer.into();
    let address = String::from_utf8(raw).expect("[BUG] encoding Email produced non utf8 data");
    Ok(MailAddress::new_unchecked(address, needs_smtputf8))
}

pub fn derive_envelop_data_from_mail(mail: &Mail)
    -> Result<smtp::EnvelopData, MailError>
{
    let headers = mail.headers();
    let smtp_from =
        if let Some(sender) = headers.get_single(Sender) {
            let sender = sender?;
            //TODO double check with from field
            mailaddress_from_mailbox(sender)?
        } else {
            let from = headers.get_single(_From)
                .ok_or(BuildInValidationError::NoFrom)??;

            if from.len() > 1 {
                return Err(BuildInValidationError::MultiMailboxFromWithoutSender.into());
            }

            mailaddress_from_mailbox(from.first())?
        };

    let smtp_to =
        if let Some(to) = headers.get_single(_To) {
            let to = to?;
            to.try_mapped_ref(mailaddress_from_mailbox)?
        } else {
            return Err(BuildInValidationError::NoTo.into());
        };

    //TODO Cc, Bcc

    Ok(EnvelopData {
        from: Some(smtp_from),
        to: smtp_to
    })
}