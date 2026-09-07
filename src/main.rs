use std::{collections::HashMap, sync::Arc};
use tokio::join;

use volty::{http::routes::users::user_edit::UserEdit, prelude::*};

mod constants;
mod database;
mod defaults;
mod error;
mod import;
mod listing;
mod models;
mod profiles;
mod proxy;

use constants::HELP_MESSAGE;
use database::DB;
pub use error::Error;
use models::{Author, Profile};
use profiles::EditCommand;

struct Bot {
    http: Http,
    cache: Cache,

    db: DB,
    requests: reqwest::Client,
}

impl Bot {
    async fn check_profile(
        &self,
        channel_id: &str,
        user_id: &str,
        profile: &mut Profile,
    ) -> Result<(), Error> {
        let bot_permissions = self
            .cache
            .fetch_channel_permissions(&self.http, channel_id, self.cache.user_id())
            .await?;
        if !bot_permissions.has(Permission::Masquerade) {
            return Err(Error::BotMissing(Permission::Masquerade));
        }
        if !bot_permissions.has(Permission::ManageRole) {
            profile.colour = None;
        }

        let user_permissions = self
            .cache
            .fetch_channel_permissions(&self.http, channel_id, user_id)
            .await?;
        if !user_permissions.has(Permission::Masquerade) {
            return Err(Error::UserMissing(Permission::Masquerade));
        }
        Ok(())
    }

    async fn send_masq(
        &self,
        author_id: &str,
        channel_id: &str,
        sendable: SendableMessage,
    ) -> Result<Message, Error> {
        let message = self.http.send_message(channel_id, sendable).await?;
        self.db
            .set_author(Author {
                message_id: message.id.clone(),
                user_id: author_id.to_string(),
            })
            .await?;
        Ok(message)
    }

    async fn extract_masq_messages(
        &self,
        message: &Message,
    ) -> Result<Vec<SendableMessage>, Error> {
        let Some(content) = &message.content else {
            return Ok(Vec::new());
        };
        if content.starts_with(self.cache.user_mention()) {
            return Ok(Vec::new());
        }

        let user_id = &message.author_id;
        let channel_id = &message.channel_id;
        let channel = self.cache.get_channel(channel_id).await.unwrap();
        let server_id = channel.server_id();
        if let Some(server_id) = server_id {
            if self.db.is_proxy_off(user_id, server_id).await {
                return Ok(Vec::new());
            }
        }
        let mut default = self.db.get_default(user_id, server_id, channel_id).await;

        let mut sendables = Vec::new();
        let mut push = |c: (Profile, String)| {
            let mut send = SendableMessage::new().content(c.1).masquerade(c.0);
            if message.replies.is_some() && sendables.is_empty() {
                send = send.replies(message.replies.clone().unwrap_or_default());
            }
            sendables.push(send);
        };
        let mut current: Option<(Profile, String)> = None;
        for line in content.lines() {
            if let Some((name, rest)) = line.split_once(';').map(|(n, r)| (n, r.trim_start())) {
                if let Some(mut profile) = self.db.get_profile(&message.author_id, name).await {
                    self.check_profile(&message.channel_id, &message.author_id, &mut profile)
                        .await?;
                    if let Some(c) = current {
                        push(c);
                    }
                    current = Some((profile, rest.to_string()));
                    continue;
                }
            }
            if let Some(c) = &mut current {
                c.1.push('\n');
                c.1.push_str(line);
            } else if let Some(default) = default.take() {
                current = Some((default, line.to_string()));
            } else {
                return Ok(Vec::new());
            }
        }
        if let Some(c) = current {
            push(c);
        }

        Ok(sendables)
    }

    async fn on_message(&self, message: &Message) -> Result<(), Error> {
        if message.author_id == self.cache.user_id() {
            return Ok(());
        }

        let sendables = self.extract_masq_messages(message).await?;
        if !sendables.is_empty() {
            let mut delete = Some(async {
                let channel_id = &message.channel_id;
                let user_id = self.cache.user_id();
                if self
                    .cache
                    .fetch_channel_permissions(&self.http, channel_id, user_id)
                    .await
                    .is_ok_and(|p| p.has(Permission::ManageMessages))
                {
                    let _ = self
                        .http
                        .delete_message(&message.channel_id, &message.id)
                        .await;
                }
            });

            for send in sendables.into_iter().take(10) {
                let send = self.send_masq(&message.author_id, &message.channel_id, send);
                if let Some(delete) = delete.take() {
                    let (result, _) = join!(send, delete);
                    result?;
                } else {
                    send.await?;
                }
            }
            return Ok(());
        }

        let Some(stripped) = message
            .content
            .as_ref()
            .and_then(|c| c.strip_prefix(self.cache.user_mention()))
            .map(|s| s.trim())
        else {
            return Ok(());
        };
        let user = self
            .cache
            .fetch_user(&self.http, &message.author_id)
            .await?;
        if user.bot.is_some() {
            return Ok(());
        }

        let (command, rest) = stripped
            .split_once(|c: char| c.is_whitespace())
            .map(|(c, r)| (c, r.trim_start()))
            .unwrap_or((stripped, ""));
        match command {
            "create" => {
                self.create_profile(message, rest).await?;
            }
            "name" | "n" => {
                self.edit_profile(EditCommand::Name, message, rest).await?;
            }
            "display_name" | "display" | "d" => {
                self.edit_profile(EditCommand::DisplayName, message, rest)
                    .await?;
            }
            "avatar" | "pfp" | "a" => {
                self.edit_profile(EditCommand::Avatar, message, rest)
                    .await?;
            }
            "colour" | "color" | "c" => {
                self.edit_profile(EditCommand::Colour, message, rest)
                    .await?;
            }
            "delete" => {
                self.delete_profile(message, rest).await?;
            }
            "list" => {
                self.list_profiles(message, rest).await?;
            }
            "hide" => {
                self.set_hidden(message, rest, true).await?;
            }
            "unhide" => {
                self.set_hidden(message, rest, false).await?;
            }
            "author" => {
                let Some(reply_id) = message.replies.as_ref().and_then(|r| r.first()) else {
                    return Ok(());
                };
                let content = match self.db.get_author(reply_id).await? {
                    Some(author) => {
                        format!("<\\@{}>", author.user_id)
                    }
                    None => "Unknown".to_string(),
                };
                let send = SendableMessage::new()
                    .content(content)
                    .reply(message.id.clone());
                self.http.send_message(&message.channel_id, send).await?;
            }
            "default" | "server_default" | "sdefault" | "channel_default" | "cdefault" => {
                self.default_command(message, command, rest).await?;
            }
            "import" => {
                self.import_command(message, rest).await?;
            }
            "proxy" => {
                self.proxy_command(message, rest).await?;
            }
            _ => {
                let bot_user = self.cache.user().await;
                let send = SendableMessage::new()
                    .content(HELP_MESSAGE.replace("%DISPLAY_NAME%", &bot_user.username))
                    .reply(message.id.clone());
                self.http.send_message(&message.channel_id, send).await?;
            }
        };

        Ok(())
    }

    async fn on_message_error(&self, message: &Message, error: Error) {
        let send = match error {
            Error::BotMissing(permission)
            | Error::Http(HttpError::Api(ApiError::MissingPermission { permission })) => {
                let content = format!("I don't have `{permission}` permission.");
                if permission == Permission::SendMessage {
                    let dm = match self.cache.fetch_dm(&self.http, &message.author_id).await {
                        Ok(dm) => dm,
                        Err(e) => {
                            log::error!("Opening DM for {}\n{e:?}", &message.author_id);
                            return;
                        }
                    };
                    if let Err(e) = self.http.send_message(dm.id(), content).await {
                        log::error!("Sending DM to {}\n{e:?}", &message.author_id);
                    }
                    return;
                }
                content
            }
            Error::UserMissing(perm) => format!("You don't have `{perm}` permission."),
            Error::UserMaxProfiles(max) => format!("Max profiles reached ({max})"),
            Error::Http(e) => {
                log::error!("on_message_error:\n{message:?}\n{e:?}");
                return;
            }
            Error::Mongo(e) => {
                log::error!("on_message_error:\n{message:?}\n{e:?}");
                return;
            }
            Error::Validate(e) => {
                log::debug!("on_message_error:validate:\n{message:?}\n{e:?}");
                let mut send = String::new();
                for (field, errors) in e.field_errors() {
                    for error in errors {
                        send.push_str(field);
                        send.push(' ');
                        send.push_str(error.message.as_ref().unwrap_or(&error.code));
                        send.push('\n');
                    }
                }
                send
            }
        };
        if let Err(e) = self.http.send_message(&message.channel_id, send).await {
            log::error!("on_message_error:send_message:\n{message:?}\n{e:?}");
        }
    }

    async fn on_react(
        &self,
        channel_id: &str,
        message_id: &str,
        user_id: &str,
        emoji_id: &str,
    ) -> Result<(), Error> {
        let message = self
            .cache
            .fetch_message(&self.http, channel_id, message_id)
            .await?;
        if message.author_id != self.cache.user_id() {
            return Ok(());
        }
        if message.interactions.is_none() {
            return Ok(());
        }

        let Some(reply_id) = message.replies.as_ref().and_then(|r| r.first()) else {
            return Ok(());
        };
        let reply = self
            .cache
            .fetch_message(&self.http, channel_id, reply_id)
            .await?;
        if reply.author_id != user_id {
            return Ok(());
        }

        let Some(content) = &message.content else {
            return Ok(());
        };

        let data = get_data(content);
        if let Some("L") = data.get("T").copied() {
            self.on_listing_react(&message, &reply, data, emoji_id)
                .await?;
        }

        Ok(())
    }

    async fn on_react_error(&self, error: Error) {
        log::error!("on_react_error:\n{error:?}");
    }
}

#[async_trait]
impl RawHandler for Bot {
    async fn on_ready(
        &self,
        _users: Vec<User>,
        _servers: Vec<Server>,
        _channels: Vec<Channel>,
        _members: Vec<Member>,
        _emojis: Vec<Emoji>,
    ) {
        println!("Ready as {}", self.cache.user().await.username);

        let user = self.cache.user().await;
        if user
            .status
            .is_none_or(|s| s.text.as_deref() != Some("Mention Me!"))
        {
            let edit = UserEdit::new().status_text("Mention Me!");
            if let Err(e) = self.http.edit_user(self.cache.user_id(), edit).await {
                log::error!("on_ready:edit_user:\n{e:?}");
            }
        }
    }

    async fn on_message(&self, message: Message) {
        if let Err(e) = self.on_message(&message).await {
            self.on_message_error(&message, e).await;
        }
    }

    async fn on_message_react(
        &self,
        id: String,
        channel_id: String,
        user_id: String,
        emoji_id: String,
    ) {
        if let Err(e) = self.on_react(&channel_id, &id, &user_id, &emoji_id).await {
            self.on_react_error(e).await;
        }
    }

    async fn on_message_unreact(
        &self,
        id: String,
        channel_id: String,
        user_id: String,
        emoji_id: String,
    ) {
        if let Err(e) = self.on_react(&channel_id, &id, &user_id, &emoji_id).await {
            self.on_react_error(e).await;
        }
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().unwrap();
    env_logger::init();
    let db = {
        let uri = std::env::var("MONGO_URI").expect("Missing Env Variable: MONGO_URI");
        let db_name = std::env::var("MONGO_DB_NAME").expect("Missing Env Variable: MONGO_DB_NAME");
        let authors_col =
            std::env::var("MONGO_AUTHORS_COL").expect("Missing Env Variable: MONGO_AUTHORS_COL");
        let profiles_col =
            std::env::var("MONGO_PROFILES_COL").expect("Missing Env Variable: MONGO_PROFILES_COL");
        let defaults_col =
            std::env::var("MONGO_DEFAULTS_COL").expect("Missing Env Variable: MONGO_DEFAULTS_COL");
        let proxy_off_col = std::env::var("MONGO_PROXY_OFF_COL")
            .expect("Missing Env Variable: MONGO_PROXY_OFF_COL");
        DB::new(
            &uri,
            &db_name,
            &authors_col,
            &profiles_col,
            &defaults_col,
            &proxy_off_col,
        )
        .await
        .unwrap()
    };
    let requests = reqwest::Client::new();

    let token = std::env::var("BOT_TOKEN").expect("Missing Env Variable: BOT_TOKEN");
    let http = Http::new(&token, true);
    let ws = WebSocket::connect(&token).await;
    let cache = Cache::new();

    let bot = Bot {
        http,
        cache: cache.clone(),
        db,
        requests,
    };
    let handler = Arc::new(bot);

    loop {
        let event = ws.next().await;
        cache.update(event.clone()).await;
        let h = handler.clone();
        tokio::spawn(async move {
            h.on_event(event).await;
        });
    }
}

fn get_data(mut text: &str) -> HashMap<&str, &str> {
    let mut data = HashMap::new();
    while let Some(stripped) = text.strip_prefix("[](") {
        let Some((kv, rest)) = stripped.split_once(')') else {
            break;
        };
        let Some((key, value)) = kv.split_once(':') else {
            break;
        };
        data.insert(key, value);
        text = rest;
    }
    data
}
