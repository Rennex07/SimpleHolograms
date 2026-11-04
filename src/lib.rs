//! Port of FancyHolograms (https://github.com/FancyMcPlugins/FancyHolograms),
//! MIT, Copyright (c) 2022-2023 Oliver. See `LICENSE` and `NOTICE`.
//!
//! Text holograms for Pumpkin, implemented as display entities.

pub mod bridge;
pub mod completion;
pub mod config;
pub mod data;
pub mod store;
pub mod text;
pub mod yaml;

use pumpkin_plugin_api::command::{
    Arg, ArgumentType, Command, CommandError, CommandNode, CommandSender, ConsumedArgs, StringType,
};
use pumpkin_plugin_api::commands::CommandSuggestionHandler;
use pumpkin_plugin_api::permission::{Permission, PermissionDefault, PermissionLevel};
use pumpkin_plugin_api::text::TextComponent;
use pumpkin_plugin_api::{Context, Plugin, PluginMetadata, Server, register_plugin};

use bridge::State;
use data::{HologramData, LineEdit, LineEditError, Location, calculate_coordinate};
use yaml::fmt_coord;

static PLACEHOLDER_TASK: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

const PLUGIN_NAME: &str = "SimpleHolograms";

const HOLOGRAM_COMMANDS: &[(&str, &str, &str)] = &[
    ("create", "hologram.create", "Create a hologram"),
    ("list", "hologram.list", "List all holograms"),
    ("info", "hologram.info", "View information about a hologram"),
    ("delete", "hologram.remove", "Remove a hologram"),
    (
        "move",
        "hologram.edit.move_to",
        "Move a hologram to coordinates",
    ),
    (
        "movehere",
        "hologram.edit.move_here",
        "Move a hologram to your position",
    ),
    ("reload", "admin", "Reload the config and holograms"),
];

const EDIT_PERMISSIONS: &[(&str, &str)] = &[
    ("hologram.edit.line.add", "Add a line to a hologram"),
    ("hologram.line.set", "Set a line of a hologram"),
    ("hologram.edit.line.remove", "Remove a line from a hologram"),
    (
        "hologram.edit.insert_before",
        "Insert a line before another line",
    ),
    (
        "hologram.edit.insert_after",
        "Insert a line after another line",
    ),
];

register_plugin!(HologramPlugin);

pub struct HologramPlugin;

impl Plugin for HologramPlugin {
    fn new() -> Self {
        Self
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: PLUGIN_NAME.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            authors: vec!["Rennex07".to_string()],
            description: "Text holograms using display entities, ported from FancyHolograms"
                .to_string(),
            dependencies: vec![],
            permissions: vec![
                pumpkin_plugin_api::permissions::FS_READ_DATA.to_string(),
                pumpkin_plugin_api::permissions::FS_WRITE_DATA.to_string(),
            ],
        }
    }

    fn on_load(&self, context: Context) -> pumpkin_plugin_api::Result<()> {
        let data_folder = std::path::PathBuf::from(context.get_data_folder());
        bridge::init(bridge::load_config(&data_folder), data_folder);

        register_permissions(&context);
        context.register_command(hologram_command(), "command");

        let server = context.get_server();
        let swept = bridge::sweep_marked_entities(&server);
        if swept > 0 {
            tracing::info!("Removed {swept} hologram entities left over from a previous run");
        }
        bridge::load_from_disk(&server, true);
        let task = pumpkin_plugin_api::scheduler::schedule_repeating_task(20, 20, |_| {
            bridge::refresh_placeholders()
        });
        *PLACEHOLDER_TASK.lock().unwrap_or_else(|e| e.into_inner()) = Some(task);
        tracing::info!("SimpleHolograms enabled");
        Ok(())
    }

    fn on_unload(&self, _context: Context) -> pumpkin_plugin_api::Result<()> {
        if let Some(task) = PLACEHOLDER_TASK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            pumpkin_plugin_api::scheduler::cancel_task(task);
        }
        if bridge::with_state(|s| s.dirty) {
            bridge::save_to_disk();
        }
        bridge::remove_all_entities();
        tracing::info!("SimpleHolograms disabled");
        Ok(())
    }
}

fn register_permissions(context: &Context) {
    let mut nodes: Vec<Permission> = vec![Permission {
        node: format!("{PLUGIN_NAME}:admin"),
        description: "Administrative access to the SimpleHolograms commands".to_string(),
        default: PermissionDefault::Op(PermissionLevel::Three),
        children: vec![],
    }];
    nodes.push(Permission {
        node: format!("{PLUGIN_NAME}:command"),
        description: "Access the hologram command root; subcommands have separate permissions"
            .to_string(),
        default: PermissionDefault::Allow,
        children: vec![],
    });

    for (suffix, description) in HOLOGRAM_COMMANDS
        .iter()
        .map(|(_, suffix, description)| (*suffix, *description))
        .chain(EDIT_PERMISSIONS.iter().copied())
    {
        if suffix == "admin" {
            continue;
        }
        nodes.push(Permission {
            node: format!("{PLUGIN_NAME}:{suffix}"),
            description: description.to_string(),
            default: PermissionDefault::Op(PermissionLevel::Three),
            children: vec![],
        });
    }

    for permission in nodes {
        let node = permission.node.clone();
        if let Err(error) = context.register_permission(&permission) {
            tracing::warn!("Could not register permission {node}: {error}");
        }
    }
}

fn permitted(sender: &CommandSender, server: &Server, suffix: &str) -> bool {
    sender.has_permission(server, &format!("{PLUGIN_NAME}:{suffix}"))
}

#[cfg(test)]
mod metadata_tests {
    use super::*;

    #[test]
    fn plugin_name_matches_the_permission_namespace() {
        assert_eq!(HologramPlugin.metadata().name, "SimpleHolograms");
        assert_eq!(format!("{PLUGIN_NAME}:command"), "SimpleHolograms:command");
    }
}

fn hologram_command() -> Command {
    let name = || CommandNode::argument("name", &ArgumentType::String(StringType::SingleWord));
    let existing_name = || name().suggest(HologramSuggestions { editing: false });
    let coords = || CommandNode::argument("coords", &ArgumentType::String(StringType::Greedy));
    let edit_tail = || CommandNode::argument("edit", &ArgumentType::String(StringType::Greedy));
    let create_text = || CommandNode::argument("text", &ArgumentType::String(StringType::Greedy));

    Command::new(
        &[
            "hologram".to_string(),
            "holograms".to_string(),
            "holo".to_string(),
            "fholo".to_string(),
        ],
        "Main command for the SimpleHolograms plugin",
    )
    // Without an executable root the whole tree renders as `Unknown command`
    // for everyone, operators included.
    .execute(Handler::Help)
    .then(CommandNode::literal("help").execute(Handler::Help))
    .then(
        CommandNode::literal("list").execute(Handler::List).then(
            CommandNode::argument("page", &ArgumentType::String(StringType::SingleWord))
                .execute(Handler::List),
        ),
    )
    .then(CommandNode::literal("create").then(name().then(create_text().execute(Handler::Create))))
    .then(
        CommandNode::literal("edit").then(
            edit_tail()
                .suggest(HologramSuggestions { editing: true })
                .execute(Handler::Edit),
        ),
    )
    .then(CommandNode::literal("info").then(existing_name().execute(Handler::Info)))
    .then(CommandNode::literal("delete").then(existing_name().execute(Handler::Delete)))
    .then(CommandNode::literal("remove").then(existing_name().execute(Handler::Delete)))
    .then(
        CommandNode::literal("move").then(
            coords()
                .suggest(HologramSuggestions { editing: false })
                .execute(Handler::Move),
        ),
    )
    .then(CommandNode::literal("movehere").then(existing_name().execute(Handler::MoveHere)))
    .then(CommandNode::literal("reload").execute(Handler::Reload))
}

struct HologramSuggestions {
    editing: bool,
}

impl CommandSuggestionHandler for HologramSuggestions {
    fn suggest(
        &self,
        sender: CommandSender,
        server: Server,
        request: pumpkin_plugin_api::command::SuggestionRequest,
    ) -> pumpkin_plugin_api::command::CommandSuggestions {
        let holograms = bridge::with_state(|state| {
            state
                .holograms
                .values()
                .map(|hologram| (hologram.name.clone(), hologram.text.len()))
                .collect::<Vec<_>>()
        });
        let allowed: Vec<_> = completion::EDITS
            .iter()
            .copied()
            .filter(|edit| {
                let (node, _) = line_edit_permission(LineEdit::parse(edit).expect("known edit"));
                permitted(&sender, &server, node)
            })
            .collect();
        let (offset, values) =
            completion::suggestions(&request.remaining, self.editing, &holograms, &allowed);
        pumpkin_plugin_api::command::CommandSuggestions {
            start: request.start + offset as u32,
            length: (request.remaining.len() - offset) as u32,
            values: values
                .into_iter()
                .map(|value| pumpkin_plugin_api::command::CommandSuggestion {
                    value,
                    tooltip: None,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy)]
enum Handler {
    Help,
    List,
    Create,
    Edit,
    Info,
    Delete,
    Move,
    MoveHere,
    Reload,
}

impl Handler {
    fn permission(self) -> Option<&'static str> {
        match self {
            Self::Help => None,
            Self::List => Some("hologram.list"),
            Self::Create => Some("hologram.create"),
            Self::Edit => None,
            Self::Info => Some("hologram.info"),
            Self::Delete => Some("hologram.remove"),
            Self::Move => Some("hologram.edit.move_to"),
            Self::MoveHere => Some("hologram.edit.move_here"),
            Self::Reload => Some("admin"),
        }
    }
}

fn info(sender: &CommandSender, message: &str) {
    sender.send_message(TextComponent::text(message));
}

fn error(sender: &CommandSender, message: &str) {
    sender.send_error(TextComponent::text(message));
}

fn success(sender: &CommandSender, message: &str) {
    sender.send_message(TextComponent::text(message));
}

/// Missing arguments appear as empty strings, so treat both as absent.
fn arg_str(args: &ConsumedArgs, key: &str) -> Option<String> {
    match args.get_value(key) {
        Arg::Simple(value) | Arg::Msg(value) if !value.is_empty() => Some(value),
        _ => None,
    }
}

fn arg_name(args: &ConsumedArgs, key: &str) -> Option<String> {
    crate::data::split_first(&arg_str(args, key)?).0
}

impl pumpkin_plugin_api::commands::CommandHandler for Handler {
    fn handle(
        &self,
        sender: CommandSender,
        server: Server,
        args: ConsumedArgs,
    ) -> pumpkin_plugin_api::Result<i32, CommandError> {
        // The command root has its own node, registered as `Allow` so the default is a
        // usable `/hologram help`. Checking it here means an administrator who denies it
        // turns the whole tree off in one place, instead of the node being registered and
        // never consulted.
        if !permitted(&sender, &server, "command") {
            error(&sender, "You don't have the required permission to do that");
            return Ok(0);
        }
        if let Some(suffix) = self.permission()
            && !permitted(&sender, &server, suffix)
        {
            error(&sender, "You don't have the required permission to do that");
            return Ok(0);
        }
        if matches!(
            self,
            Self::Create | Self::Edit | Self::Delete | Self::Move | Self::MoveHere
        ) && bridge::with_state(|s| s.save_blocked)
        {
            error(
                &sender,
                "Hologram storage did not load completely; changes are disabled until it is repaired and reloaded",
            );
            return Ok(0);
        }

        match self {
            Self::Help => {
                for line in HELP_TEXT {
                    info(&sender, line);
                }
                // The original returns false after printing help.
                Ok(0)
            }
            Self::List => list(&sender, &args),
            Self::Create => create(&sender, &server, &args),
            Self::Edit => edit(&sender, &server, &args),
            Self::Info => info_command(&sender, &args),
            Self::Delete => delete(&sender, &args),
            Self::Move => move_to(&sender, &server, &args),
            Self::MoveHere => move_here(&sender, &server, &args),
            Self::Reload => reload(&sender, &server),
        }
    }
}

const HELP_TEXT: &[&str] = &[
    "SimpleHolograms commands help:",
    "- /hologram help - Shows all (sub)commands",
    "- /hologram list [page] - Shows an overview of all holograms",
    "- /hologram create <name> <text ...> - Creates a new hologram",
    "- /hologram edit <name> addline <text ...> - Adds a line at the bottom",
    "- /hologram edit <name> setline <line> <text ...> - Edits a line",
    "- /hologram edit <name> removeline <line> - Removes a line",
    "- /hologram edit <name> insertbefore <line> <text ...> - Inserts a line before another",
    "- /hologram edit <name> insertafter <line> <text ...> - Inserts a line after another",
    "- /hologram delete <name> - Removes a hologram",
    "- /hologram info <name> - Shows information about a hologram",
    "- /hologram move <name> <x> <y> <z> - Moves a hologram to the coordinates",
    "- /hologram movehere <name> - Moves a hologram to your position",
    "- /hologram reload - Reloads the config and all holograms",
    "",
    "Coordinates accept ~ (relative to the hologram) and ~~ (relative to you).",
    "Holograms are visible to everyone the server tracks; per-player visibility",
    "is not supported by this port.",
];

fn list(
    sender: &CommandSender,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let page = match arg_str(args, "page") {
        Some(value) => match value.parse::<usize>() {
            Ok(page) if page > 0 => page,
            _ => {
                error(sender, "Invalid page number");
                return Ok(0);
            }
        },
        None => 1,
    };
    let (count, lines) = bridge::with_state(|s| {
        let count = s.holograms.len();
        if count == 0 {
            return (0, Vec::new());
        }
        let Some((offset, pages)) = data::page_bounds(count, page) else {
            return (count, Vec::new());
        };
        let mut lines = vec![
            "List of holograms:".to_string(),
            format!("Page {page}/{pages}"),
        ];
        for data in s.holograms.values().skip(offset).take(10) {
            let location = &data.location;
            lines.push(format!(
                " - {} ({}/{}/{} in {})",
                data.name,
                fmt_coord(location.x),
                fmt_coord(location.y),
                fmt_coord(location.z),
                location.world
            ));
        }
        (count, lines)
    });

    if count == 0 {
        info(
            sender,
            "There are no holograms. Use '/hologram create' to create one",
        );
        return Ok(0);
    }
    if lines.is_empty() {
        error(sender, "Page does not exist");
        return Ok(0);
    }
    for line in lines {
        info(sender, &line);
    }
    Ok(0)
}

fn create(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let Some(player) = sender.as_player() else {
        error(sender, "You must be a sender to use this command");
        return Ok(0);
    };

    let Some(name) = arg_str(args, "name") else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };

    if bridge::with_state(|s| s.get(&name).is_some()) {
        error(sender, "There already exists a hologram with this name");
        return Ok(0);
    }

    if name.contains('.') {
        error(sender, "The name of the hologram cannot contain a dot");
        return Ok(0);
    }

    let text = arg_str(args, "text").unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    if !text.is_empty() {
        lines.extend(text.split('\n').map(str::to_string));
    }
    let (x, y, z) = player.get_position();
    let world = player.get_world();
    let mut data = HologramData::new(
        name,
        Location::new(
            world.get_id(),
            x,
            y,
            z,
            player.get_yaw(),
            player.get_pitch(),
        )
        .in_dimension(world.get_dimension()),
    );
    if !lines.is_empty() {
        data.text = lines;
    }

    bridge::sync(server, &data);
    bridge::with_state(|s| {
        s.holograms.insert(State::key(&data.name), data.clone());
        s.dirty = true;
    });

    save_changed(sender);
    success(sender, "Created the hologram");
    Ok(0)
}

fn edit(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let (name, rest) = crate::data::split_first(&arg_str(args, "edit").unwrap_or_default());
    let Some(name) = name else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };
    // The original resolves the hologram before it looks at the option, so a
    // misspelt name is reported even when the option is nonsense too.
    let Some(data) = bridge::with_state(|s| s.get(&name).cloned()) else {
        error(sender, &format!("Could not find hologram: '{name}'"));
        return Ok(0);
    };

    let (option, tail) = crate::data::split_first(&rest);
    let Some(edit) = option.as_deref().and_then(LineEdit::parse) else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };

    let (node, denied) = line_edit_permission(edit);
    if !permitted(sender, server, node) {
        error(sender, denied);
        return Ok(0);
    }

    apply_line_edit(sender, server, data, edit, &tail)
}

fn line_edit_permission(edit: LineEdit) -> (&'static str, &'static str) {
    match edit {
        LineEdit::Add => (
            "hologram.edit.line.add",
            "You don't have the required permission to add a line to this hologram",
        ),
        LineEdit::Set => (
            "hologram.line.set",
            "You don't have the required permission to set a line to this hologram",
        ),
        LineEdit::Remove => (
            "hologram.edit.line.remove",
            "You don't have the required permission to remove a line from a hologram",
        ),
        LineEdit::InsertBefore => (
            "hologram.edit.insert_before",
            "You don't have the required permission to edit a hologram",
        ),
        LineEdit::InsertAfter => (
            "hologram.edit.insert_after",
            "You don't have the required permission to edit a hologram",
        ),
    }
}

fn apply_line_edit(
    sender: &CommandSender,
    server: &Server,
    mut data: HologramData,
    edit: LineEdit,
    tail: &str,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let (number, text) = if edit == LineEdit::Add {
        (0, line_text(tail))
    } else {
        let (number, rest) = crate::data::split_first(tail);
        let Some(number) = number else {
            error(sender, "Wrong usage: /hologram help");
            return Ok(0);
        };
        // Line numbers are Java ints upstream (`Ints.tryParse`), so anything
        // outside an i32 is "could not parse" rather than a huge index.
        let Ok(number) = number.parse::<i32>() else {
            error(sender, "Could not parse line number");
            return Ok(0);
        };
        (number, line_text(&rest))
    };

    match data::edit_line(&mut data.text, edit, number, &text) {
        Ok(line) => {
            save_text(sender, server, &data);
            match edit {
                LineEdit::InsertBefore | LineEdit::InsertAfter => success(sender, "Inserted line"),
                _ => success(sender, &format!("Changed text for line {line}")),
            }
            Ok(0)
        }
        Err(LineEditError::InvalidIndex) => {
            error(sender, "Invalid line index");
            Ok(0)
        }
        Err(LineEditError::MissingText) => {
            // Upstream has this message only for the insert options; the others
            // would crash on an empty tail, so they get the usage line.
            error(
                sender,
                if matches!(edit, LineEdit::InsertBefore | LineEdit::InsertAfter) {
                    "You need to provide a text to insert"
                } else {
                    "Wrong usage: /hologram help"
                },
            );
            Ok(0)
        }
    }
}

fn line_text(tail: &str) -> String {
    tail.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn save_text(sender: &CommandSender, server: &Server, data: &HologramData) {
    bridge::with_state(|s| {
        s.holograms.insert(State::key(&data.name), data.clone());
        s.dirty = true;
    });
    bridge::sync(server, data);
    save_changed(sender);
}

fn save_changed(sender: &CommandSender) {
    if bridge::with_state(|s| s.config.save_on_changed) && !bridge::save_to_disk() {
        error(
            sender,
            "Hologram changes could not be saved; they remain in memory",
        );
    }
}

fn info_command(
    sender: &CommandSender,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let Some(name) = arg_name(args, "name") else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };
    let Some(data) = bridge::with_state(|s| s.get(&name).cloned()) else {
        error(sender, &format!("Could not find hologram: '{name}'"));
        return Ok(0);
    };

    let location = &data.location;
    let visibility =
        data.effective_visibility_distance(bridge::with_state(|s| s.config.visibility_distance));

    info(
        sender,
        &format!("Information about the {} hologram:", data.name),
    );
    info(sender, &format!("Name: {}", data.name));
    info(sender, "Type: TEXT");
    info(
        sender,
        &format!(
            "Location: {} {} / {} / {}",
            location.world, location.x, location.y, location.z
        ),
    );
    info(sender, &format!("Visibility distance: {visibility} blocks"));

    let scale = data.scale;
    if scale.0 == scale.1 && scale.1 == scale.2 {
        info(sender, &format!("Scale: x{}", scale.0));
    } else {
        info(
            sender,
            &format!("Scale: {}, {}, {}", scale.0, scale.1, scale.2),
        );
    }
    info(
        sender,
        &format!("Billboard: {}", data.billboard.as_str().to_uppercase()),
    );
    info(sender, &format!("Shadow radius: {}", data.shadow_radius));
    info(
        sender,
        &format!("Shadow strength: {}", data.shadow_strength),
    );

    info(sender, "Text: ");
    for line in &data.text {
        info(sender, &format!(" {line}"));
    }
    match text::format_background(data.background) {
        Some(name) => info(sender, &format!("Background: {name}")),
        None => info(sender, "Background: default"),
    }
    info(
        sender,
        &format!("Text alignment: {}", data.text_alignment.name()),
    );
    info(
        sender,
        &format!("See through: {}", enabled(data.see_through)),
    );
    info(
        sender,
        &format!("Text shadow: {}", enabled(data.text_shadow)),
    );
    if data.update_text_interval == -1 {
        info(sender, "Update text interval: not updating");
    } else {
        info(
            sender,
            &format!("Update text interval: {} ticks", data.update_text_interval),
        );
    }
    Ok(0)
}

fn enabled(value: bool) -> &'static str {
    if value { "enabled" } else { "disabled" }
}

fn delete(
    sender: &CommandSender,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let Some(name) = arg_name(args, "name") else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };
    if bridge::with_state(|s| s.get(&name).is_none()) {
        error(sender, &format!("Could not find hologram: '{name}'"));
        return Ok(0);
    }

    bridge::with_state(|s| {
        s.holograms.remove(&State::key(&name));
        s.dirty = true;
    });
    bridge::despawn(&name);
    if !bridge::save_to_disk() {
        error(
            sender,
            "Hologram changes could not be saved; they remain in memory",
        );
    }
    success(sender, "Removed the hologram");
    Ok(0)
}

fn move_to(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let Some(player) = sender.as_player() else {
        error(sender, "You must be a sender to use this command");
        return Ok(0);
    };
    let (name, raw_coords) = crate::data::split_first(&arg_str(args, "coords").unwrap_or_default());
    let Some(name) = name else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };
    let Some(data) = bridge::with_state(|s| s.get(&name).cloned()) else {
        error(sender, &format!("Could not find hologram: '{name}'"));
        return Ok(0);
    };

    let (px, py, pz) = player.get_position();
    let origin = (data.location.x, data.location.y, data.location.z);

    let parts: Vec<&str> = raw_coords.split_whitespace().collect();

    let (x, y, z) = match parts.as_slice() {
        [] => (px, py, pz),
        [x, y, z] => {
            match (
                calculate_coordinate(x, origin.0, px),
                calculate_coordinate(y, origin.1, py),
                calculate_coordinate(z, origin.2, pz),
            ) {
                (Some(x), Some(y), Some(z)) => (x, y, z),
                _ => {
                    error(sender, "Could not parse position");
                    return Ok(0);
                }
            }
        }
        _ => {
            error(sender, "Could not parse position");
            return Ok(0);
        }
    };

    let mut location = Location::new(
        data.location.world.clone(),
        x,
        y,
        z,
        data.location.yaw,
        data.location.pitch,
    );
    location.dimension = data.location.dimension.clone();
    set_location(sender, server, data.name.clone(), location);
    Ok(0)
}

fn move_here(
    sender: &CommandSender,
    server: &Server,
    args: &ConsumedArgs,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    let Some(player) = sender.as_player() else {
        error(sender, "You must be a sender to use this command");
        return Ok(0);
    };
    let Some(name) = arg_name(args, "name") else {
        error(sender, "Wrong usage: /hologram help");
        return Ok(0);
    };
    let Some(data) = bridge::with_state(|s| s.get(&name).cloned()) else {
        error(sender, &format!("Could not find hologram: '{name}'"));
        return Ok(0);
    };

    let (x, y, z) = player.get_position();
    let world = player.get_world();
    let location = Location::new(
        world.get_id(),
        x,
        y,
        z,
        data.location.yaw,
        data.location.pitch,
    )
    .in_dimension(world.get_dimension());
    set_location(sender, server, data.name, location);
    Ok(0)
}

fn set_location(sender: &CommandSender, server: &Server, name: String, location: Location) {
    let Some(mut data) = bridge::with_state(|s| s.get(&name).cloned()) else {
        return;
    };
    if data.location.world != location.world || data.location.dimension != location.dimension {
        bridge::despawn(&name);
    }
    data.location = location;
    bridge::with_state(|s| {
        s.holograms.insert(State::key(&name), data.clone());
        s.dirty = true;
    });
    bridge::sync(server, &data);

    save_changed(sender);

    let moved = &data.location;
    success(
        sender,
        &format!(
            "Moved the hologram to {}/{}/{} {}° {}°",
            fmt_coord(moved.x),
            fmt_coord(moved.y),
            fmt_coord(moved.z),
            fmt_coord(f64::from((moved.yaw + 180.0) % 360.0)),
            fmt_coord(f64::from(moved.pitch % 360.0))
        ),
    );
}

fn reload(
    sender: &CommandSender,
    server: &Server,
) -> pumpkin_plugin_api::Result<i32, CommandError> {
    if bridge::with_state(|s| s.dirty) {
        error(
            sender,
            "Reload refused because hologram changes have not been saved",
        );
        return Ok(0);
    }
    let config = bridge::with_state(|s| {
        let folder = s.data_folder.clone();
        bridge::load_config(&folder)
    });
    bridge::with_state(|s| s.config = config);

    if !bridge::load_from_disk(server, false) {
        error(
            sender,
            "Reload failed: holograms.yml did not load completely; existing holograms were retained",
        );
        return Ok(0);
    }

    success(sender, "Reloaded config and holograms");
    Ok(0)
}
