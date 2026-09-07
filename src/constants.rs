pub const HELP_MESSAGE: &str = "## Create
`@%DISPLAY_NAME% create {name} {display_name}`
## Use
`name;Text you want to send.`
## Edit
`@%DISPLAY_NAME% display {name} {display_name}`
`@%DISPLAY_NAME% avatar {name} {url}`
`@%DISPLAY_NAME% colour {name} {colour}`
To remove a field
`@%DISPLAY_NAME% display {name} clear`
## Delete
`@%DISPLAY_NAME% delete {name}`
## List
`@%DISPLAY_NAME% list`
`@%DISPLAY_NAME% list all` include hidden profiles (DM only)
## Hide
Hidden profiles are left out of `list` but can still be used.
`@%DISPLAY_NAME% hide {name}`
`@%DISPLAY_NAME% unhide {name}`
## Info
`@%DISPLAY_NAME% author` reply to a message to get original author
## Default
Messages sent without a prefix will use your default profile if set.
`@%DISPLAY_NAME% default {name}` set a global default profile
`@%DISPLAY_NAME% server_default {name}` set a server default profile
`@%DISPLAY_NAME% channel_default {name}` set a channel default profile
To remove defaults use the same command but without a name
`@%DISPLAY_NAME% default` remove global default profile
## Permissions
-Required
`Masquerade` users will also need this.
-Optional
`ManageMessages` to delete the original message.
`ManageRoles` to set masquerade colour.

[Support Server](https://rvlt.gg/SPMxwwC8)";
