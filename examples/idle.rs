use async_imap::error::{Error, Result};
use async_std::task;
use std::env;

fn main() -> Result<()> {
    task::block_on(async {
        let args: Vec<String> = env::args().collect();
        if args.len() != 4 {
            eprintln!("need three arguments: imap-server login password");
            Err(Error::Bad("need three arguments".into()))
        } else {
            perform_idle(&args[1], &args[2], &args[3]).await?;
            Ok(())
        }
    })
}

async fn perform_idle(imap_server: &str, login: &str, password: &str) -> Result<()> {
    let tls = async_tls::TlsConnector::new();

    // we pass in the imap_server twice to check that the server's TLS
    // certificate is valid for the imap_server we're connecting to.
    let client = async_imap::connect((imap_server, 993), imap_server, &tls).await?;
    println!("** connected to {}:{}", imap_server, 993);

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut imap_session = client.login(login, password).await.map_err(|e| e.0)?;
    println!("** logged in a {}", login);

    // we want to fetch the first email in the INBOX mailbox
    imap_session.select("INBOX").await?;
    println!("** INBOX selected");

    println!("** XXX TODO: implement idle call + readline from stdin to interrupt");

    // be nice to the server and log out
    println!("** Logging out");
    imap_session.logout().await?;

    Ok(())
}
