use async_imap::error::Result;
use async_std::prelude::*;
use async_std::task;

fn main() -> Result<()> {
    task::block_on(async {
        // To connect to the gmail IMAP server with this you will need to allow unsecure apps access.
        // See: https://support.google.com/accounts/answer/6010255?hl=en
        // Look at the gmail_oauth2.rs example on how to connect to a gmail server securely.
        fetch_inbox_top().await?;
        Ok(())
    })
}

async fn fetch_inbox_top() -> Result<Option<String>> {
    let domain = "imap.example.com";
    let tls = async_tls::TlsConnector::new();

    // we pass in the domain twice to check that the server's TLS
    // certificate is valid for the domain we're connecting to.
    let client = async_imap::connect((domain, 993), domain, &tls).await?;

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client
        .login("me@example.com", "password")
        .await
        .map_err(|e| e.0)?;

    // we want to fetch the first email in the INBOX mailbox
    imap_session.select("INBOX").await?;

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

    // be nice to the server and log out
    imap_session.logout().await?;

    Ok(Some(body))
}
