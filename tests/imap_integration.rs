use std::time::Duration;

use async_imap::Session;
use async_std::net::TcpStream;
use async_std::prelude::*;
use async_std::sync::Arc;
use async_std::task;
use lettre::Transport;

fn native_tls() -> native_tls::TlsConnector {
    native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .unwrap()
}

pub struct NoCertificateVerification {}

impl rustls::ServerCertVerifier for NoCertificateVerification {
    fn verify_server_cert(
        &self,
        _roots: &rustls::RootCertStore,
        _presented_certs: &[rustls::Certificate],
        _dns_name: webpki::DNSNameRef<'_>,
        _ocsp: &[u8],
    ) -> Result<rustls::ServerCertVerified, rustls::TLSError> {
        Ok(rustls::ServerCertVerified::assertion())
    }
}

fn tls() -> async_tls::TlsConnector {
    let mut config = rustls::ClientConfig::new();
    config
        .dangerous()
        .set_certificate_verifier(Arc::new(NoCertificateVerification {}));
    Arc::new(config).into()
}

async fn session(user: &str) -> Session<async_tls::client::TlsStream<TcpStream>> {
    let mut s = async_imap::connect(
        &format!(
            "{}:3993",
            std::env::var("TEST_HOST").unwrap_or("127.0.0.1".to_string())
        ),
        "imap.example.com",
        &tls(),
    )
    .await
    .unwrap()
    .login(user, user)
    .await
    .ok()
    .unwrap();
    s.debug = true;
    s
}

fn smtp(user: &str) -> lettre::SmtpTransport {
    let creds = lettre::smtp::authentication::Credentials::new(user.to_string(), user.to_string());
    lettre::SmtpClient::new(
        &format!(
            "{}:3465",
            std::env::var("TEST_HOST").unwrap_or("127.0.0.1".to_string())
        ),
        lettre::ClientSecurity::Wrapper(lettre::ClientTlsParameters {
            connector: native_tls(),
            domain: "smpt.example.com".to_string(),
        }),
    )
    .unwrap()
    .credentials(creds)
    .transport()
}

#[ignore]
#[test]
fn connect_insecure_then_secure() {
    task::block_on(async {
        let host = std::env::var("TEST_HOST").unwrap_or("127.0.0.1".to_string());
        let stream = TcpStream::connect((host.as_ref(), 3143)).await.unwrap();

        // ignored because of https://github.com/greenmail-mail-test/greenmail/issues/135
        async_imap::Client::new(stream)
            .secure("imap.example.com", &tls())
            .await
            .unwrap();
    });
}

#[test]
fn connect_secure() {
    task::block_on(async {
        async_imap::connect(
            &format!(
                "{}:3993",
                std::env::var("TEST_HOST").unwrap_or("127.0.0.1".to_string())
            ),
            "imap.example.com",
            &tls(),
        )
        .await
        .unwrap();
    });
}

#[test]
fn login() {
    task::block_on(async {
        session("readonly-test@localhost").await;
    });
}

#[test]
fn logout() {
    task::block_on(async {
        let mut s = session("readonly-test@localhost").await;
        s.logout().await.unwrap();
    });
}

#[test]
fn inbox_zero() {
    task::block_on(async {
        // https://github.com/greenmail-mail-test/greenmail/issues/265
        let mut s = session("readonly-test@localhost").await;
        s.select("INBOX").await.unwrap();
        let inbox = s.search("ALL").await.unwrap();
        assert_eq!(inbox.len(), 0);
    });
}

#[test]
fn inbox() {
    task::block_on(async {
        let to = "inbox@localhost";

        // first log in so we'll see the unsolicited e-mails
        let mut c = session(to).await;
        c.select("INBOX").await.unwrap();

        println!("sending");
        // then send the e-mail
        let mut s = smtp(to);
        let e = lettre_email::Email::builder()
            .from("sender@localhost")
            .to(to)
            .subject("My first e-mail")
            .text("Hello world from SMTP")
            .build()
            .unwrap();
        s.send(e.into()).unwrap();

        println!("searching");

        // now we should see the e-mail!
        let inbox = c.search("ALL").await.unwrap();
        // and the one message should have the first message sequence number
        assert_eq!(inbox.len(), 1);
        assert!(inbox.contains(&1));

        // we should also get two unsolicited responses: Exists and Recent
        c.noop().await.unwrap();
        println!("noop done");
        let mut unsolicited = Vec::new();
        while !c.unsolicited_responses.is_empty() {
            unsolicited.push(c.unsolicited_responses.recv().await.unwrap());
        }

        assert_eq!(unsolicited.len(), 2);
        assert!(unsolicited
            .iter()
            .any(|m| m == &async_imap::types::UnsolicitedResponse::Exists(1)));
        assert!(unsolicited
            .iter()
            .any(|m| m == &async_imap::types::UnsolicitedResponse::Recent(1)));

        println!("fetching");

        // let's see that we can also fetch the e-mail
        let fetch: Vec<_> = c
            .fetch("1", "(ALL UID)")
            .await
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .await
            .unwrap();
        assert_eq!(fetch.len(), 1);
        let fetch = &fetch[0];
        assert_eq!(fetch.message, 1);
        assert_ne!(fetch.uid, None);
        assert_eq!(fetch.size, Some(138));
        let e = fetch.envelope().unwrap();
        assert_eq!(e.subject, Some(&b"My first e-mail"[..]));
        assert_ne!(e.from, None);
        assert_eq!(e.from.as_ref().unwrap().len(), 1);
        let from = &e.from.as_ref().unwrap()[0];
        assert_eq!(from.mailbox, Some(&b"sender"[..]));
        assert_eq!(from.host, Some(&b"localhost"[..]));
        assert_ne!(e.to, None);
        assert_eq!(e.to.as_ref().unwrap().len(), 1);
        let to = &e.to.as_ref().unwrap()[0];
        assert_eq!(to.mailbox, Some(&b"inbox"[..]));
        assert_eq!(to.host, Some(&b"localhost"[..]));
        let date_opt = fetch.internal_date();
        assert!(date_opt.is_some());

        // and let's delete it to clean up
        c.store("1", "+FLAGS (\\Deleted)")
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        c.expunge().await.unwrap().collect::<Vec<_>>().await;

        // the e-mail should be gone now
        let inbox = c.search("ALL").await.unwrap();
        assert_eq!(inbox.len(), 0);
    });
}

#[test]
fn inbox_uid() {
    task::block_on(async {
        let to = "inbox-uid@localhost";

        // first log in so we'll see the unsolicited e-mails
        let mut c = session(to).await;
        c.select("INBOX").await.unwrap();

        // then send the e-mail
        let mut s = smtp(to);
        let e = lettre_email::Email::builder()
            .from("sender@localhost")
            .to(to)
            .subject("My first e-mail")
            .text("Hello world from SMTP")
            .build()
            .unwrap();
        s.send(e.into()).unwrap();

        // now we should see the e-mail!
        let inbox = c.uid_search("ALL").await.unwrap();
        // and the one message should have the first message sequence number
        assert_eq!(inbox.len(), 1);
        let uid = inbox.into_iter().next().unwrap();

        // we should also get two unsolicited responses: Exists and Recent
        c.noop().await.unwrap();
        let mut unsolicited = Vec::new();
        while !c.unsolicited_responses.is_empty() {
            unsolicited.push(c.unsolicited_responses.recv().await.unwrap());
        }

        assert_eq!(unsolicited.len(), 2);
        assert!(unsolicited
            .iter()
            .any(|m| m == &async_imap::types::UnsolicitedResponse::Exists(1)));
        assert!(unsolicited
            .iter()
            .any(|m| m == &async_imap::types::UnsolicitedResponse::Recent(1)));

        // let's see that we can also fetch the e-mail
        let fetch: Vec<_> = c
            .uid_fetch(format!("{}", uid), "(ALL UID)")
            .await
            .unwrap()
            .collect::<Result<_, _>>()
            .await
            .unwrap();
        assert_eq!(fetch.len(), 1);
        let fetch = &fetch[0];
        assert_eq!(fetch.uid, Some(uid));
        let e = fetch.envelope().unwrap();
        assert_eq!(e.subject, Some(&b"My first e-mail"[..]));
        let date_opt = fetch.internal_date();
        assert!(date_opt.is_some());

        // and let's delete it to clean up
        c.uid_store(format!("{}", uid), "+FLAGS (\\Deleted)")
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;
        c.expunge().await.unwrap().collect::<Vec<_>>().await;

        // the e-mail should be gone now
        let inbox = c.search("ALL").await.unwrap();
        assert_eq!(inbox.len(), 0);
    });
}

#[test]
#[ignore]
fn list() {
    task::block_on(async {
        let mut s = session("readonly-test@localhost").await;
        s.select("INBOX").await.unwrap();
        let subdirs: Vec<_> = s.list(None, Some("%")).await.unwrap().collect().await;
        assert_eq!(subdirs.len(), 0);

        // TODO: make a subdir
    });
}

#[test]
#[ignore] // Greenmail does not support IDLE :(
fn idle() -> async_imap::error::Result<()> {
    task::block_on(async {
        let mut session = session("idle-test@localhost").await;

        // get that inbox
        let res = session.select("INBOX").await?;
        println!("selected: {:#?}", res);

        // fetchy fetch
        let msg_stream = session.fetch("1:3", "(FLAGS BODY.PEEK[])").await?;
        let msgs = msg_stream.collect::<Vec<_>>().await;
        println!("msgs: {:?}", msgs.len());

        // Idle session
        println!("starting idle");
        let mut idle = session.idle();
        idle.init().await?;

        let (idle_wait, interrupt) = idle.wait_with_timeout(std::time::Duration::from_secs(30));
        println!("idle wait");

        task::spawn(async move {
            println!("waiting for 1s");
            task::sleep(Duration::from_secs(2)).await;
            println!("interrupting idle");
            drop(interrupt);
        });

        let idle_result = idle_wait.await;
        println!("idle result: {:#?}", &idle_result);

        // return the session after we are done with it
        let mut session = idle.done().await?;

        println!("logging out");
        session.logout().await?;

        Ok(())
    })
}
