use crate::{models::Profile, Bot, Error};
use serde::{Deserialize, Serialize};
use validator::Validate;
use volty::{
    http::routes::autumn::upload_file::{Tag, UploadFile, UploadResponse},
    prelude::*,
    types::util::regex::RE_USERNAME,
};

#[derive(Deserialize)]
#[serde(untagged)]
enum Export {
    Masq(MasqExport),
    PluralKit(PluralKitExport),
    Tupper(TupperBoxExport),
}

impl Export {
    fn into_profiles(
        self,
        user_id: &str,
    ) -> Result<Vec<Profile>, (Profile, validator::ValidationErrors)> {
        let mut profiles: Vec<Profile> = match self {
            Export::Masq(export) => export
                .profiles
                .into_iter()
                .map(|p| Profile {
                    user_id: user_id.to_string(),
                    name: p.name.trim().to_string(),
                    display_name: p.display_name.map(|d| d.trim().to_string()),
                    avatar: p.avatar_url,
                    colour: p.color.map(|c| c.to_string()),
                    hidden: p.hidden.unwrap_or(false),
                })
                .collect(),
            Export::PluralKit(export) => export
                .members
                .into_iter()
                .map(|m| Profile {
                    user_id: user_id.to_string(),
                    name: m.name.trim().to_string(),
                    display_name: m.display_name.map(|d| d.trim().to_string()),
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
                    name: m.name.trim().to_string(),
                    display_name: m.nick.map(|n| n.trim().to_string()),
                    avatar: m.avatar_url,
                    colour: None,
                    hidden: false,
                })
                .collect(),
        };

        for profile in &mut profiles {
            if !RE_USERNAME.is_match(&profile.name) {
                if profile.display_name.is_none() {
                    profile.display_name = Some(profile.name.clone());
                }
                profile.name = profile.name.replace(' ', "_").replace(
                    |c: char| !(c.is_alphanumeric() || ['_', '-'].contains(&c)),
                    "",
                );
            }
        }

        if let Some((p, e)) = profiles
            .iter()
            .find_map(|p| p.validate().err().map(|e| (p, e)))
        {
            return Err((p.clone(), e));
        }

        Ok(profiles)
    }
}

#[derive(Deserialize, Serialize)]
struct MasqExport {
    profiles: Vec<MasqMember>,
}

#[derive(Deserialize, Serialize)]
struct MasqMember {
    name: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    color: Option<String>,
    hidden: Option<bool>,
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
    pub async fn export_command(&self, message: &Message, _args: &str) -> Result<(), Error> {
        let Some(profiles) = self.db.get_profiles(&message.author_id, true).await else {
            self.http
                .send_message(&message.channel_id, "No profiles found!")
                .await?;
            return Ok(());
        };
        let count = profiles.len();
        let export = MasqExport {
            profiles: profiles
                .into_iter()
                .map(|p| MasqMember {
                    name: p.name,
                    display_name: p.display_name,
                    avatar_url: p.avatar,
                    color: p.colour,
                    hidden: Some(p.hidden),
                })
                .collect(),
        };

        let bytes = serde_json::to_string_pretty(&export).unwrap().into_bytes();
        let file = UploadFile::new(bytes, Some("export.json"));
        let UploadResponse { id } = self.http.upload_file(Tag::Attachments, file).await?;

        let send = SendableMessage::new()
            .content(format!(
                "Exported {count} profile{}!",
                if count > 1 { "s" } else { "" }
            ))
            .attachment(id);
        let dm = self.cache.fetch_dm(&self.http, &message.author_id).await?;
        self.http.send_message(dm.id(), send).await?;
        Ok(())
    }

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
            let mut text = text.trim_start_matches(|c| c != '{');
            text = text.trim_end_matches(|c| c != '}');
            match serde_json::from_str(text) {
                Ok(export) => export,
                Err(e) => {
                    self.http
                        .send_message(&message.channel_id, format!("Failed to parse file!\n{e}"))
                        .await?;
                    return Ok(());
                }
            }
        };
        let profiles = match export.into_profiles(&message.author_id) {
            Ok(p) => p,
            Err((p, e)) => {
                self.http
                    .send_message(&message.channel_id, format!("Invalid profile\n{p}"))
                    .await?;
                return Err(e.into());
            }
        };
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
