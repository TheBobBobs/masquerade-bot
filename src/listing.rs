use std::{collections::HashMap, fmt::Write};

use once_cell::sync::Lazy;
use regex::Regex;
use volty::{
    http::routes::channels::message_send::SendableMessage,
    types::channels::{
        channel::Channel,
        message::{Interactions, Message},
    },
};

use crate::{Bot, Error, models::Profile};

pub const PER_PAGE: usize = 5;

pub fn get_page(profiles: &[Profile], page: usize, include_hidden: bool) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^(#[a-f0-9]{6}|[a-z]+)$").unwrap());

    let last_page = (profiles.len().max(1) - 1) / PER_PAGE;
    let mut text = format!(
        "[](T:L)[](A:{include_hidden})[](P:{page}){}/{}\n",
        page + 1,
        last_page + 1
    );
    if include_hidden {
        text.push_str("| Name | Display Name | Avatar | Colour | Hidden |\n|-|-|-|-|-|");
    } else {
        text.push_str("| Name | Display Name | Avatar | Colour |\n|-|-|-|-|");
    }

    let start = page * PER_PAGE;
    let end = (start + PER_PAGE).min(profiles.len());
    if start >= profiles.len() {
        return text;
    }

    for p in &profiles[start..end] {
        // The name stays plain text so it can be read and copied as-is.
        // A colour that KaTeX can understand gets a swatch next to its value.
        let colour = p.colour.as_deref().unwrap_or("");
        let swatch = if RE.is_match(colour) {
            format!("$$\\color{{{colour}}}\\blacksquare$$ ")
        } else {
            String::new()
        };
        write!(
            &mut text,
            "\n|{}|{}|{}|{}{}|",
            p.name,
            p.display_name.as_deref().unwrap_or(""),
            p.avatar
                .as_ref()
                .map(|u| format!("[Link](<{u}>)"))
                .unwrap_or_default(),
            swatch,
            colour
        )
        .unwrap();
        if include_hidden {
            write!(&mut text, "{}|", if p.hidden { "yes" } else { "" }).unwrap();
        }
    }

    text
}

impl Bot {
    pub async fn list_profiles(&self, message: &Message, args: &str) -> Result<(), Error> {
        let include_hidden = args.split_whitespace().next() == Some("all");
        if include_hidden {
            let channel = self.cache.get_channel(&message.channel_id).await.unwrap();
            if !matches!(channel, Channel::DirectMessage { .. }) {
                let username = self.cache.user().await.username;
                let send = SendableMessage::new()
                    .content(format!(
                        "`list all` only works in a DM, so hidden profiles never end up in a channel.\n\
                         Open my profile, choose Message, and send `@{username} list all` there."
                    ))
                    .reply(message.id.clone());
                self.http.send_message(&message.channel_id, send).await?;
                return Ok(());
            }
        }
        let profiles = self
            .db
            .get_profiles(&message.author_id, include_hidden)
            .await
            .unwrap_or_default();
        let page = get_page(&profiles, 0, include_hidden);
        let send = SendableMessage::new()
            .content(page)
            .interactions(Interactions::new(["👈", "👉"]).restrict())
            .reply(message.id.clone());
        self.http.send_message(&message.channel_id, send).await?;
        Ok(())
    }

    pub async fn on_listing_react(
        &self,
        message: &Message,
        reply: &Message,
        data: HashMap<&str, &str>,
        emoji_id: &str,
    ) -> Result<(), Error> {
        let include_hidden: bool = data
            .get("A")
            .copied()
            .and_then(|a| a.parse().ok())
            .unwrap_or(false);
        let profiles = self
            .db
            .get_profiles(&reply.author_id, include_hidden)
            .await
            .unwrap_or_default();
        let last_page = (profiles.len().max(1) - 1) / PER_PAGE;
        let current_page: usize = data
            .get("P")
            .copied()
            .and_then(|p| p.parse().ok())
            .unwrap_or(0);
        let page = match emoji_id {
            "👈" => {
                if current_page == 0 {
                    last_page
                } else {
                    current_page - 1
                }
            }
            "👉" => {
                if current_page >= last_page {
                    0
                } else {
                    current_page + 1
                }
            }
            _ => unreachable!(),
        };
        let page = get_page(&profiles, page, include_hidden);
        if Some(&page) == message.content.as_ref() {
            return Ok(());
        }
        self.http
            .edit_message(&message.channel_id, &message.id, page)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, colour: Option<&str>) -> Profile {
        let mut p = Profile::new("user", name);
        p.colour = colour.map(str::to_string);
        p
    }

    #[test]
    fn list_keeps_names_plain_and_swatches_colours() {
        let profiles = [
            profile("J", Some("#f5a9b8")),
            profile("some_name", Some("blue")),
            profile("gradient", Some("linear-gradient(to right,#f00,#00f)")),
            profile("plain", None),
        ];
        let page = get_page(&profiles, 0, false);
        let rows: Vec<&str> = page.lines().skip(3).collect();
        assert_eq!(rows[0], "|J|||$$\\color{#f5a9b8}\\blacksquare$$ #f5a9b8|");
        assert_eq!(rows[1], "|some_name|||$$\\color{blue}\\blacksquare$$ blue|");
        assert_eq!(rows[2], "|gradient|||linear-gradient(to right,#f00,#00f)|");
        assert_eq!(rows[3], "|plain||||");
    }
}
