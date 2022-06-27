use std::env;
use std::time::Duration;

use async_imap::error::{Error, Result};
use async_imap::extensions::idle::IdleResponse::*;
use futures::StreamExt;

#[cfg(feature = "runtime-async-std")]
use async_std::{task, task::sleep};

#[cfg(feature = "runtime-tokio")]
use tokio::{task, time::sleep};

#[cfg_attr(feature = "runtime-tokio", tokio::main)]
#[cfg_attr(feature = "runtime-async-std", async_std::main)]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        eprintln!("need three arguments: imap-server login password");
        Err(Error::Bad("need three arguments".into()))
    } else {
        fetch_and_idle(&args[1], &args[2], &args[3]).await?;
        Ok(())
    }
}

async fn fetch_and_idle(imap_server: &str, login: &str, password: &str) -> Result<()> {
    let tls = async_native_tls::TlsConnector::new();
    let imap_addr = (imap_server, 993);

    // we pass in the imap_server twice to check that the server's TLS
    // certificate is valid for the imap_server we're connecting to.
    let client = async_imap::connect(imap_addr, imap_server, tls).await?;
    println!("-- connected to {}:{}", imap_addr.0, imap_addr.1);

    // the client we have here is unauthenticated.
    // to do anything useful with the e-mails, we need to log in
    let mut session = client.login(login, password).await.map_err(|e| e.0)?;
    println!("-- logged in a {}", login);

    // we want to fetch some messages from the INBOX
    session.select("INBOX").await?;
    println!("-- INBOX selected");

    // fetch flags from all messages
    let msg_stream = session.fetch("1:*", "(FLAGS )").await?;
    let msgs = msg_stream.collect::<Vec<_>>().await;
    println!("-- number of fetched msgs: {:?}", msgs.len());

    // init idle session
    println!("-- initializing idle");
    let mut idle = session.idle();
    idle.init().await?;

    println!("-- idle async wait");
    let (idle_wait, interrupt) = idle.wait();

    /*
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.read_line(&mut line).await?;
    println!("-- read line: {}", line);
    */

    task::spawn(async move {
        println!("-- thread: waiting for 30s");
        sleep(Duration::from_secs(30)).await;
        println!("-- thread: waited 30 secs, now interrupting idle");
        drop(interrupt);
    });

    let idle_result = idle_wait.await?;
    match idle_result {
        ManualInterrupt => {
            println!("-- IDLE manually interrupted");
        }
        Timeout => {
            println!("-- IDLE timed out");
        }
        NewData(data) => {
            let s = String::from_utf8(data.borrow_raw().to_vec()).unwrap();
            println!("-- IDLE data:\n{}", s);
        }
    }

    // return the session after we are done with it
    println!("-- sending DONE");
    let mut session = idle.done().await?;

    // be nice to the server and log out
    println!("-- logging out");
    session.logout().await?;
    Ok(())
}
