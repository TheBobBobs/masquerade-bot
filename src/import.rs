use crate::{models::Profile, Bot, Error};
use serde::Deserialize;
use validator::Validate;
use volty::prelude::*;

#[derive(Deserialize)]
#[serde(untagged)]
enum Export {
    PluralKit(PluralKitExport),
    Tupper(TupperBoxExport),
}

impl Export {
    fn into_profiles(self, user_id: &str) -> Result<Vec<Profile>, validator::ValidationErrors> {
        let profiles: Vec<Profile> = match self {
            Export::PluralKit(export) => export
                .members
                .into_iter()
                .map(|m| Profile {
                    user_id: user_id.to_string(),
                    name: m.name,
                    display_name: m.display_name,
                    avatar: m.avatar_url,
                    colour: m.color.map(|c| format!("#{c}")),
                    hidden: m
                        .privacy
                        .and_then(|p| p.visibility)
                        .is_some_and(|v| v == "private"),
                })
                .collect(),
            Export::Tupper(export) => export
                .tuppers
                .into_iter()
                .map(|m| Profile {
                    user_id: user_id.to_string(),
                    name: m.name,
                    display_name: m.nick,
                    avatar: m.avatar_url,
                    colour: None,
                    hidden: false,
                })
                .collect(),
        };
        if let Some(e) = profiles.iter().find_map(|p| p.validate().err()) {
            return Err(e);
        }
        Ok(profiles)
    }
}

#[derive(Deserialize)]
struct PluralKitExport {
    members: Vec<PluralKitMember>,
}

#[derive(Deserialize)]
struct PluralKitPrivacy {
    visibility: Option<String>,
}

#[derive(Deserialize)]
struct PluralKitMember {
    name: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    color: Option<String>,
    privacy: Option<PluralKitPrivacy>,
}

#[derive(Deserialize)]
struct TupperBoxExport {
    tuppers: Vec<TupperBoxMember>,
}

#[derive(Deserialize)]
struct TupperBoxMember {
    name: String,
    nick: Option<String>,
    avatar_url: Option<String>,
}

impl Bot {
    pub async fn import_command(&self, message: &Message, _args: &str) -> Result<(), Error> {
        let Some([attatchment, ..]) = message.attachments.as_deref() else {
            self.http
                .send_message(
                    &message.channel_id,
                    "Command requires a json file from running pk;export or tul!export",
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
        let export: Export = {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_hides_members_with_private_visibility() {
        let json = r#"{"members": [
            {"name": "alice", "privacy": {"visibility": "public"}},
            {"name": "bob", "privacy": {"visibility": "private"}},
            {"name": "carol", "privacy": null},
            {"name": "dave"}
        ]}"#;
        let export: PluralKitExport = serde_json::from_str(json).unwrap();
        let profiles = export.into_profiles("user").unwrap();
        let hidden: Vec<_> = profiles
            .iter()
            .map(|p| (p.name.as_str(), p.hidden))
            .collect();
        assert_eq!(
            hidden,
            [
                ("alice", false),
                ("bob", true),
                ("carol", false),
                ("dave", false)
            ]
        );
    }
}
