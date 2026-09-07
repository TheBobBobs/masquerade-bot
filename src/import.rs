use crate::{models::Profile, Bot, Error};
use serde::Deserialize;
use validator::Validate;
use volty::prelude::*;

#[derive(Deserialize)]
struct PluralKitExport {
    members: Vec<PluralKitMember>,
}

#[derive(Deserialize)]
struct PluralKitMember {
    name: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    color: Option<String>,
}

impl PluralKitExport {
    fn into_profiles(self, user_id: &str) -> Result<Vec<Profile>, validator::ValidationErrors> {
        let profiles: Vec<_> = self
            .members
            .into_iter()
            .map(|m| Profile {
                user_id: user_id.to_string(),
                name: m.name,
                display_name: m.display_name,
                avatar: m.avatar_url,
                colour: m.color.map(|c| format!("#{c}")),
            })
            .collect();
        if let Some(e) = profiles.iter().find_map(|p| p.validate().err()) {
            return Err(e);
        }
        Ok(profiles)
    }
}

impl Bot {
    pub async fn import_command(&self, message: &Message, _args: &str) -> Result<(), Error> {
        let Some([attatchment, ..]) = message.attachments.as_deref() else {
            self.http
                .send_message(
                    &message.channel_id,
                    "Command requires a json file from running pk;export",
                )
                .await?;
            return Ok(());
        };
        if attatchment.size > (256 * 1024) {
            self.http
                .send_message(&message.channel_id, "File too large!")
                .await?;
            return Ok(());
        }
        let export: PluralKitExport = {
            let api_info = self.cache.api_info(&self.http).await?;
            let url = attatchment.autumn_url(&api_info.features.autumn.url);
            let Ok(response) = self.requests.get(url).send().await else {
                self.http
                    .send_message(&message.channel_id, "Failed to download file!")
                    .await?;
                return Ok(());
            };
            let Ok(text) = response.text().await else {
                self.http
                    .send_message(&message.channel_id, "Failed to download file!")
                    .await?;
                return Ok(());
            };
            match serde_json::from_str(&text) {
                Ok(export) => export,
                Err(e) => {
                    self.http
                        .send_message(&message.channel_id, format!("Failed to parse file!\n{e}"))
                        .await?;
                    return Ok(());
                }
            }
        };
        let profiles = export.into_profiles(&message.author_id)?;
        let count = profiles.len();

        for profile in profiles {
            self.db.save_profile(&message.author_id, profile).await?;
        }

        self.http
            .send_message(
                &message.channel_id,
                format!(
                    "Imported {count} Profile{}!",
                    if count != 1 { "s" } else { "" }
                ),
            )
            .await?;

        Ok(())
    }
}
