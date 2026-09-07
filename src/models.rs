use validator::Validate;
use volty::types::{
    channels::message::Masquerade,
    util::regex::{RE_COLOUR, RE_DISPLAY_NAME, RE_USERNAME},
};

#[derive(Clone, Debug)]
pub struct Author {
    pub message_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug, Validate, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProfileTag {
    #[validate(length(min = 1, max = 64, message = "must be <= 64 characters"))]
    pub prefix: Option<String>,
    #[validate(length(min = 1, max = 64, message = "must be <= 64 characters"))]
    pub suffix: Option<String>,
}

impl std::fmt::Display for ProfileTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = self.prefix.as_deref().unwrap_or_default();
        let suffix = self.suffix.as_deref().unwrap_or_default();
        write!(f, "{prefix}text{suffix}")
    }
}

impl ProfileTag {
    pub fn parse(mut tag: &str) -> Option<Self> {
        tag = tag.trim();
        let index = tag.find("text")?;
        let prefix = &tag[..index];
        let suffix = &tag[index + 4..];
        if prefix.contains("text") || suffix.contains("text") {
            return None;
        }
        let prefix = (!prefix.is_empty()).then(|| prefix.to_string());
        let suffix = (!suffix.is_empty()).then(|| suffix.to_string());
        Some(Self { prefix, suffix })
    }

    pub fn len(&self) -> usize {
        self.prefix.as_ref().map(String::len).unwrap_or(0)
            + self.suffix.as_ref().map(String::len).unwrap_or(0)
    }

    pub fn get_match<'t>(&self, text: &'t str) -> Option<&'t str> {
        let has_prefix = self.prefix.as_ref().is_none_or(|p| text.starts_with(p));
        let has_suffix = self.suffix.as_ref().is_none_or(|s| text.ends_with(s));
        if !(has_prefix && has_suffix) {
            return None;
        }
        let prefix_len = self.prefix.as_ref().map(String::len).unwrap_or(0);
        let suffix_len = self.suffix.as_ref().map(String::len).unwrap_or(0);
        let matched = &text[prefix_len..text.len() - suffix_len];
        if matched.is_empty() {
            return None;
        }
        Some(matched)
    }
}

#[derive(Clone, Debug, Validate)]
pub struct Profile {
    pub user_id: String,
    #[validate(
        length(min = 1, max = 32, message = "must be <= 32 characters"),
        regex(path = *RE_USERNAME, message = "contains invalid characters")
    )]
    pub name: String,
    #[validate(nested)]
    pub tags: Vec<ProfileTag>,
    #[validate(
        length(min = 1, max = 32, message = "must be <= 32 characters"),
        regex(path = *RE_DISPLAY_NAME, message = "contains invalid characters")
    )]
    pub display_name: Option<String>,
    #[validate(
        length(min = 1, max = 128, message = "must be <= 128 characters"),
        url(message = "isn't a valid url")
    )]
    pub avatar: Option<String>,
    #[validate(
        length(min = 1, max = 128, message = "must be <= 128 characters"),
        regex(path = *RE_COLOUR, message = "not supported")
    )]
    pub colour: Option<String>,
    pub hidden: bool,
}

impl Profile {
    pub fn new(user_id: &str, name: &str) -> Self {
        Self {
            user_id: user_id.to_string(),
            name: name.to_string(),
            tags: vec![ProfileTag {
                prefix: Some(format!("{name};")),
                suffix: None,
            }],
            display_name: None,
            avatar: None,
            colour: None,
            hidden: false,
        }
    }
}

impl From<Profile> for Masquerade {
    fn from(val: Profile) -> Self {
        let name = val.display_name.unwrap_or(val.name);
        Self {
            name: Some(name),
            avatar: val.avatar,
            colour: val.colour,
        }
    }
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Name: {}\nDisplay Name: {}\nAvatar: {}\nColour: {}\nHidden: {}",
            self.name,
            self.display_name.as_deref().unwrap_or(""),
            self.avatar.as_deref().unwrap_or(""),
            self.colour.as_deref().unwrap_or(""),
            self.hidden
        )
    }
}
