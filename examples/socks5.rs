use async_imap::error::{Error, Result};
use async_std::prelude::*;
use async_std::task;
use std::env;

fn main() -> Result<()> {
    task::block_on(async {
        let res = fetch_inbox_top().await?;
        println!("**result:\n{}", res.unwrap());
        Ok(())
    })
}

async fn fetch_inbox_top() -> Result<Option<String>> {

    let imap_host= "g77kjrad6bafzzyldqvffq6kxlsgphcygptxhnn4xlnktfgaqshilmyd.onion".to_string();
    let imap_port: u16 = 143;
    let socks5_host = "127.0.0.1".to_string();
    let socks5_port = 9150;

    let user = "user";
    let password = "xxx";

    
    let client = async_imap::connect_with_socks5(imap_host.clone(), imap_port, socks5_host, socks5_port).await?;

    println!("-- connected to {}:{}", imap_host, imap_port);

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client.login(user, password).await.map_err(|e| e.0)?;
    println!("-- logged in a {}", user);

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
