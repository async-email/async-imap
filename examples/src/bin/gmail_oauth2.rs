use async_imap::{error::Result, types::TlsConnector};
use futures::StreamExt;

struct GmailOAuth2 {
    user: String,
    access_token: String,
}

impl async_imap::Authenticator for &GmailOAuth2 {
    type Response = String;

    fn process(&mut self, _data: &[u8]) -> Self::Response {
        format!(
            "user={}\x01auth=Bearer {}\x01\x01",
            self.user, self.access_token
        )
    }
}

#[cfg_attr(feature = "runtime-tokio", tokio::main)]
#[cfg_attr(feature = "runtime-async-std", async_std::main)]
async fn main() -> Result<()> {
    let gmail_auth = GmailOAuth2 {
        user: String::from("sombody@gmail.com"),
        access_token: String::from("<access_token>"),
    };
    let domain = "imap.gmail.com";
    let port = 993;
    let socket_addr = (domain, port);
    let tls = TlsConnector::new();
    let client = async_imap::connect(socket_addr, domain, tls).await?;

    let mut imap_session = match client.authenticate("XOAUTH2", &gmail_auth).await {
        Ok(c) => c,
        Err((e, _unauth_client)) => {
            println!("error authenticating: {}", e);
            return Err(e);
        }
    };

    match imap_session.select("INBOX").await {
        Ok(mailbox) => println!("{}", mailbox),
        Err(e) => println!("Error selecting INBOX: {}", e),
    };

    {
        let mut msgs = imap_session.fetch("2", "body[text]").await.map_err(|e| {
            eprintln!("Error Fetching email 2: {}", e);
            e
        })?;

        while let Some(msg) = msgs.next().await {
            print!("{:?}", msg?);
        }
    }

    imap_session.logout().await?;
    Ok(())
}
