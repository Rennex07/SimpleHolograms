# Architecture and compatibility

This plugin ports the text-hologram subset of FancyHolograms. Unlike the
original's per-player packet displays, holograms are real Pumpkin text-display
entities. The server controls entity tracking; per-player visibility,
visibility permissions and viewer-specific text are not supported. The saved
visibility distance is passed to the display's view-range setting, not used as
a per-player distance gate. Item and block holograms are not supported.

## Lifecycle and storage

Holograms are stored in `holograms.yml`. Spawned displays have gravity disabled
and carry a hidden persistent marker. On startup the plugin sweeps marked
entities from loaded worlds before restoring holograms; on unload it saves and
removes its live entities. A crash can leave entities behind for a subsequent
sweep. Recovery of crash-orphaned entities has not been verified in-game.

An incomplete load disables saving to protect existing data. Reload validates
the file before replacing live entities and refuses when there are unsaved
changes. New and moved holograms record world folder and dimension; older
entries without a dimension use world-name lookup.

## Text and commands

PumpkinPIE server-wide placeholders refresh every 20 ticks, including after a
late provider startup. Player-specific placeholders remain literal because a
shared entity has no viewer context. The legacy `update_text_interval` setting
does not control this refresh.

The command syntax uses `create <name> <text>` and supports `delete` and its
`remove` alias. Empty edit text and out-of-range removals are refused. Several
display properties can be loaded from saved data but have no editing commands.
Namespaced permissions are registered for Pumpkin; handlers also accept the
original permission nodes.

The YAML implementation supports the subset used by the plugin, not arbitrary
YAML. It retains upstream format quirks, including narrowing loaded coordinates
through `f32`, defaulting centered billboard to null, and the lossy round-trip
of unquoted hexadecimal background colours. Unsupported storage versions and
unavailable worlds prevent a complete load.

## Command tree

Attach an executor to the intended argument node, not its parent. Coordinate
tails use a greedy argument and are parsed by the handler; chaining two
single-word arguments did not parse on the target host. Register each literal
only once to avoid command-tree collisions.
