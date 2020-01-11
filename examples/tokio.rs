use async_imap::error::{Error, Result};
use async_std::prelude::*;
use std::env;
use tokio::runtime::Runtime;

fn main() -> Result<()> {
    let mut rt = Runtime::new().expect("unable to create runtime");

    rt.block_on(async {
        let args: Vec<String> = env::args().collect();
        if args.len() != 4 {
            eprintln!("need three arguments: imap-server login password");
            Err(Error::Bad("need three arguments".into()))
        } else {
            let res = fetch_inbox_top(&args[1], &args[2], &args[3]).await?;
            println!("**result:\n{}", res.unwrap());
            Ok(())
        }
    })
}

async fn fetch_inbox_top(imap_server: &str, login: &str, password: &str) -> Result<Option<String>> {
    let tls = async_native_tls::TlsConnector::new();

    // we pass in the imap_server twice to check that the server's TLS
    // certificate is valid for the imap_server we're connecting to.
    let client = async_imap::connect((imap_server, 993), imap_server, tls).await?;
    println!("-- connected to {}:{}", imap_server, 993);

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client.login(login, password).await.map_err(|e| e.0)?;
    println!("-- logged in a {}", login);

    // we want to fetch the first email in the INBOX mailbox
    imap_session.select("INBOX").await?;
    println!("-- INBOX selected");

    // fetch message number 1 in this mailbox, along with its RFC822 field.
    // RFC 822 dictates the format of the body of e-mails
    let messages_stream = imap_session.fetch("1", "RFC822").await?;
    let messages: Vec<_> = messages_stream.collect::<Result<_>>().await?;
    let message = if let Some(m) = messages.first() {
        m
    } else {
        return Ok(None);
    };

    // extract the message's body
    let body = message.body().expect("message did not have a body!");
    let body = std::str::from_utf8(body)
        .expect("message was not valid utf-8")
        .to_string();
    println!("-- 1 message received, logging out");

    // be nice to the server and log out
    imap_session.logout().await?;

    Ok(Some(body))
}
