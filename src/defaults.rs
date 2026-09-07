use volty::prelude::*;

use crate::{Bot, Error, database::DefaultProfileDocId};

impl Bot {
    pub async fn default_command(
        &self,
        message: &Message,
        command: &str,
        args: &str,
    ) -> Result<(), Error> {
        let user_id = message.author_id.clone();

        let id = match command {
            "default" => DefaultProfileDocId::Global { user_id },
            "server_default" | "sdefault" => {
                let channel = self.cache.get_channel(&message.channel_id).await.unwrap();
                let Some(server_id) = channel.server_id() else {
                    let send = SendableMessage::new()
                        .content("Not in a server!")
                        .reply(message.id.clone());
                    self.http.send_message(&message.channel_id, send).await?;
                    return Ok(());
                };
                DefaultProfileDocId::Server {
                    user_id,
                    server_id: server_id.to_string(),
                }
            }
            "channel_default" | "cdefault" => DefaultProfileDocId::Channel {
                user_id,
                channel_id: message.channel_id.clone(),
            },
            _ => unreachable!(),
        };

        let name = args.split_whitespace().next();
        let Some(name) = name else {
            self.db.set_default(id, name).await?;
            let send = SendableMessage::new()
                .content("Success!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        let Some(mut profile) = self.db.get_profile(&message.author_id, name).await else {
            let send = SendableMessage::new()
                .content("Profile doesn't exist!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        self.db.set_default(id, Some(name)).await?;

        self.check_profile(&message.channel_id, &message.author_id, &mut profile)
            .await?;
        let send = SendableMessage::new()
            .content("Success!")
            .masquerade(profile)
            .reply(message.id.clone());
        self.send_masq(&message.author_id, &message.channel_id, send)
            .await?;

        Ok(())
    }
}
