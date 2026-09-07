use once_cell::sync::Lazy;
use regex::Regex;
use volty::{
    http::routes::channels::message_send::SendableMessage, types::channels::message::Message,
};

use crate::{Bot, Error, models::Profile};

fn parse_colours(colours: &str) -> String {
    let colours = colours.trim();
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(#?[a-z0-9]+)$").unwrap());
    if colours.split_whitespace().all(|a| RE.is_match(a)) {
        let colours: Vec<&str> = colours.split_whitespace().collect();
        if colours.len() > 1 {
            return format!("linear-gradient(to right,{})", colours.join(","));
        }
    }
    colours.to_string()
}

pub enum EditCommand {
    Name,
    DisplayName,
    Avatar,
    Colour,
}

impl Bot {
    pub async fn create_profile(&self, message: &Message, args: &str) -> Result<(), Error> {
        let (name, display_name) = args
            .split_once(|c: char| c.is_whitespace())
            .map(|(n, d)| (n, Some(d)))
            .unwrap_or((args, None));
        let mut profile = Profile::new(&message.author_id, name);
        profile.display_name = display_name.map(|s| s.to_string());
        if let Some(attachment) = message.attachments.as_ref().and_then(|a| a.first()) {
            let api_info = self.cache.api_info(&self.http).await?;
            profile.avatar = Some(attachment.autumn_url(&api_info.features.autumn.url));
        }
        self.db
            .save_profile(&message.author_id, profile.clone())
            .await?;

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

    pub async fn edit_profile(
        &self,
        command: EditCommand,
        message: &Message,
        args: &str,
    ) -> Result<(), Error> {
        let (name, mut value) = args
            .split_once(|c: char| c.is_whitespace())
            .map(|(n, d)| (n, Some(d.to_string())))
            .unwrap_or((args, None));

        if matches!(command, EditCommand::Avatar)
            && value.is_none()
            && let Some(attachment) = message.attachments.as_ref().and_then(|a| a.first())
        {
            let api_info = self.cache.api_info(&self.http).await?;
            value = Some(attachment.autumn_url(&api_info.features.autumn.url));
        }
        if value.is_none() {
            let content = match self.db.get_profile(&message.author_id, name).await {
                Some(profile) => {
                    let value = match command {
                        EditCommand::Name => Some(profile.name),
                        EditCommand::DisplayName => profile.display_name,
                        EditCommand::Avatar => profile.avatar,
                        EditCommand::Colour => profile.colour,
                    };
                    value.unwrap_or("None".to_string())
                }
                None => format!("Profile not found!\n{name}"),
            };
            let send = SendableMessage::new()
                .content(content)
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        }

        let value = value.and_then(|s| (s != "clear").then_some(s.to_string()));
        let mut profile = self
            .db
            .get_profile(&message.author_id, name)
            .await
            .unwrap_or_else(|| Profile::new(&message.author_id, name));
        match command {
            EditCommand::Name => profile.name = value.unwrap_or(name.to_string()),
            EditCommand::DisplayName => profile.display_name = value,
            EditCommand::Avatar => profile.avatar = value,
            EditCommand::Colour => {
                let colour = value.map(|v| parse_colours(&v));
                profile.colour = colour;
            }
        };
        self.db
            .save_profile(&message.author_id, profile.clone())
            .await?;
        if profile.name != name {
            self.db.delete_profile(&message.author_id, name).await?;
        }

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

    pub async fn set_hidden(
        &self,
        message: &Message,
        args: &str,
        hidden: bool,
    ) -> Result<(), Error> {
        let name = args;
        let content = match self.db.get_profile(&message.author_id, name).await {
            Some(mut profile) => {
                profile.hidden = hidden;
                self.db.save_profile(&message.author_id, profile).await?;
                "Success!".to_string()
            }
            None => format!("Profile not found!\n{name}"),
        };
        let send = SendableMessage::new()
            .content(content)
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;
        Ok(())
    }

    pub async fn delete_profile(&self, message: &Message, args: &str) -> Result<(), Error> {
        let name = args;
        let profile = self.db.delete_profile(&message.author_id, name).await?;
        let content = if profile.is_some() {
            "Success!".to_string()
        } else {
            format!("Profile not found!\n{name}")
        };
        let send = SendableMessage::new()
            .content(content)
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;
        Ok(())
    }
}
