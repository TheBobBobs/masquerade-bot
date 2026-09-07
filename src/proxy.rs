use volty::prelude::*;

use crate::{Bot, Error, database::ProxyOffDocId};

impl Bot {
    pub async fn proxy_command(&self, message: &Message, args: &str) -> Result<(), Error> {
        let channel = self.cache.get_channel(&message.channel_id).await.unwrap();
        let Some(server_id) = channel.server_id() else {
            let send = SendableMessage::new()
                .content("Not in a server!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        let id = ProxyOffDocId {
            user_id: message.author_id.clone(),
            server_id: server_id.to_string(),
        };

        let off = match args.split_whitespace().next() {
            Some("on") => {
                self.db.set_proxy_off(id, false).await?;
                false
            }
            Some("off") => {
                self.db.set_proxy_off(id, true).await?;
                true
            }
            _ => self.db.is_proxy_off(&id.user_id, &id.server_id).await,
        };

        let content = if off {
            "Proxying is off for you in this server."
        } else {
            "Proxying is on for you in this server."
        };
        let send = SendableMessage::new()
            .content(content)
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;
        Ok(())
    }
}
