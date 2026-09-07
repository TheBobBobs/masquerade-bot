use once_cell::sync::Lazy;
use regex::Regex;
use validator::Validate;
use volty::{
    http::routes::channels::message_send::SendableMessage, types::channels::message::Message,
};

use crate::{
    Bot, Error,
    models::{Profile, ProfileTag},
};

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

    pub async fn edit_tags(&self, message: &Message, args: &str) -> Result<(), Error> {
        let Some((name, mut rest)) = args.split_once(|c: char| c.is_whitespace()) else {
            let send = SendableMessage::new()
                .content("usage: tags {name} {text}")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        let Some(mut profile) = self.db.get_profile(&message.author_id, name).await else {
            let send = SendableMessage::new()
                .content("Profile not found!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        enum EditType {
            Set,
            Add,
            Remove,
        }
        let mut edit_type = EditType::Set;
        if let Some((sub_command, tag)) = rest.split_once(|c: char| c.is_ascii_whitespace()) {
            match sub_command {
                "set" => edit_type = EditType::Set,
                "add" => edit_type = EditType::Add,
                "remove" => edit_type = EditType::Remove,
                _ => {}
            };
            if ["set", "add", "remove"].contains(&sub_command) {
                rest = tag;
            }
        }

        let Some(tag) = ProfileTag::parse(rest) else {
            let send = SendableMessage::new()
                .content("Invalid tag!\nExamples:\n{text}\nprefix: text\ntext -suffix")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };
        if tag.validate().is_err() {
            let send = SendableMessage::new()
                .content("Tag is too long!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        }

        let index = profile
            .tags
            .iter()
            .enumerate()
            .find(|(_, t)| **t == tag)
            .map(|(i, _)| i);
        match edit_type {
            EditType::Set => profile.tags = vec![tag],
            EditType::Add => {
                if index.is_none() {
                    if profile.tags.len() >= 16 {
                        let send = SendableMessage::new()
                            .content("You already have the max amount of tags!")
                            .reply(message.id.clone());
                        self.http.send_message(&message.channel_id, send).await?;
                        return Ok(());
                    }
                    profile.tags.push(tag);
                }
            }
            EditType::Remove => {
                if let Some(index) = index {
                    profile.tags.remove(index);
                    if profile.tags.is_empty() {
                        profile.tags.push(ProfileTag {
                            prefix: Some(format!("{};", profile.name)),
                            suffix: None,
                        });
                    }
                }
            }
        }
        self.db.save_profile(&message.author_id, profile).await?;

        let send = SendableMessage::new()
            .content("Success!")
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;
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

    pub async fn delete_all_profiles(&self, message: &Message, args: &str) -> Result<(), Error> {
        let Some(profiles) = self.db.get_profiles(&message.author_id, true).await else {
            let send = SendableMessage::new()
                .content("No profiles found!")
                .reply(message.id.clone());
            self.http.send_message(&message.channel_id, send).await?;
            return Ok(());
        };

        self.export_command(message, args).await?;

        let count = profiles.len();
        for profile in profiles {
            self.db
                .delete_profile(&message.author_id, &profile.name)
                .await?;
        }

        let send = SendableMessage::new()
            .content(format!(
                "Deleted {count} profile{}!",
                if count != 1 { "s" } else { "" }
            ))
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;

        Ok(())
    }
}
