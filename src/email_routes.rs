// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// Email related routes.

// TODO: make the email body and subject configurable.

use config::Config;
use database::DatabaseError;
use email::Mailbox;
use errors::*;
use lettre::email::EmailBuilder;
use lettre::transport::smtp::{SUBMISSION_PORT, SecurityLevel, SmtpTransport, SmtpTransportBuilder};
use lettre::transport::smtp::authentication::Mechanism;
use lettre::transport::EmailTransport;
#[cfg(test)]
use lettre::transport::stub::StubEmailTransport;
use iron::prelude::*;
use iron::status::{self, Status};
use params::{FromValue, Params};
use std::str::FromStr;
use uuid::Uuid;

pub struct EmailSender {
    connection: SmtpTransport,
    from: String,
}

impl EmailSender {
    pub fn new(config: &Config) -> Result<EmailSender, ()> {

        let options = &config.options;

        if options.email.server.is_none() || options.email.user.is_none() ||
           options.email.password.is_none() || options.email.sender.is_none() {
            error!("All email fields need to be set.");
            return Err(());
        }

        let builder =
            match SmtpTransportBuilder::new((options.clone().email.server.unwrap().as_str(),
                                             SUBMISSION_PORT)) {
                Ok(builder) => builder,
                Err(error) => {
                    error!("{:?}", error);
                    return Err(());
                }
            };

        let user = options.clone().email.user.unwrap().clone();
        let password = options.clone().email.password.unwrap().clone();
        let connection = builder
            .hello_name("localhost")
            .credentials(&user, &password)
            .security_level(SecurityLevel::AlwaysEncrypt)
            .smtp_utf8(true)
            .authentication_mechanism(Mechanism::Plain)
            .connection_reuse(true)
            .build();

        Ok(EmailSender {
               connection: connection,
               from: options.clone().email.sender.unwrap().clone(),
           })
    }

    pub fn send(&mut self, to: &str, body: &str, subject: &str) -> Result<(), ()> {
        // Stub sender.
        let email = match EmailBuilder::new()
                  .to(to)
                  .from(&*self.from)
                  .body(body)
                  .subject(subject)
                  .build() {
            Ok(email) => email,
            Err(error) => {
                error!("{:?}", error);
                return Err(());
            }
        };

        #[cfg(not(test))]
        match self.connection.send(email.clone()) {
            Ok(_) => Ok(()),
            Err(error) => {
                error!("{:?}", error);
                Err(())
            }
        }

        #[cfg(test)]
        {
            let mut transport = StubEmailTransport;
            match transport.send(email.clone()) {
                Ok(_) => Ok(()),
                Err(error) => {
                    error!("{:?}", error);
                    Err(())
                }
            }
        }
    }
}

pub fn setemail(req: &mut Request, config: &Config) -> IronResult<Response> {
    info!("GET /setemail");

    let map = req.get_ref::<Params>().unwrap(); // TODO: don't unwrap.
    let token = map.find(&["token"]);
    let email = map.find(&["email"]);

    if token.is_none() || email.is_none() {
        return EndpointError::with(status::BadRequest, 400);
    }

    let token = String::from_value(token.unwrap()).unwrap();
    let email = String::from_value(email.unwrap()).unwrap();
    // Check that this is a valid email address.
    if Mailbox::from_str(&email).is_err() {
        return EndpointError::with(status::BadRequest, 400);
    }

    let link = format!("{}", Uuid::new_v4());

    // Check that this is a valid token.
    if config
           .db
           .get_record_by_token(&token)
           .recv()
           .unwrap()
           .is_err() {
        return EndpointError::with(status::BadRequest, 400);
    }

    match config
              .db
              .add_email(&email, &token, &link)
              .recv()
              .unwrap() {
        Ok(_) => {
            match EmailSender::new(config) {
                Ok(mut sender) => {
                    let body = format!("Follow <a href=\"https://{}:4443/confirmemail?s={}\">this link</a> to confirm your email.",
                                       config.options.general.domain,
                                       link);
                    match sender.send(&email, &body, "Welcome to your server!") {
                        Ok(_) => ok_response!(),
                        Err(_) => EndpointError::with(status::InternalServerError, 501),
                    }
                }
                Err(_) => EndpointError::with(status::InternalServerError, 501),
            }
        }
        Err(_) => EndpointError::with(status::InternalServerError, 501),
    }
}

// Process the email confirmation links, that have the link as the "s" parameter.
pub fn verifyemail(req: &mut Request, config: &Config) -> IronResult<Response> {
    info!("GET /setemail");

    let map = req.get_ref::<Params>().unwrap(); // TODO: don't unwrap.
    let link = map.find(&["s"]);

    if link.is_none() {
        return EndpointError::with(status::BadRequest, 400);
    }
    let link = String::from_value(link.unwrap()).unwrap();

    // TODO: return some useful content

    match config.db.get_email_by_link(&link).recv().unwrap() {
        Ok((email, token)) => {
            match config.db.get_record_by_token(&token).recv().unwrap() {
                Ok(mut record) => {
                    // Update the record to set the email address.
                    record.email = Some(email);
                    match config.db.update_record(record).recv().unwrap() {
                        Ok(_) => ok_response!(),
                        Err(DatabaseError::NoRecord) => {
                            EndpointError::with(status::BadRequest, 400)
                        }
                        Err(_) => EndpointError::with(status::InternalServerError, 501),
                    }
                }
                Err(DatabaseError::NoRecord) => EndpointError::with(status::BadRequest, 400),
                Err(_) => EndpointError::with(status::InternalServerError, 501),
            }
        }
        Err(DatabaseError::NoRecord) => EndpointError::with(status::BadRequest, 400),
        Err(_) => EndpointError::with(status::InternalServerError, 501),
    }
}