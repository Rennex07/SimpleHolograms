# Pumpkin API compatibility

The target is Pumpkin `0.2.1+26.3-26.51`, commit
`224f9f8144ca53a8d0800acb87607d714196e2a6`, with plugin WIT v0.1. The
plugin API and component WIT must match the host; the version string alone
does not establish compatibility.

The plugin uses real text-display entities and Pumpkin's display APIs. The
server handles entity tracking, so per-viewer visibility and player-specific
placeholder rendering are unavailable. Shared PumpkinPIE placeholders
refresh periodically. Some saved display properties lack editing commands.

The YAML reader supports the plugin's storage subset rather than general YAML.
An incomplete load disables writes to avoid discarding unsupported or
unavailable data. See [DESIGN.md](DESIGN.md) for lifecycle details and format
compatibility.

Host-free tests do not verify visual appearance or crash-orphan recovery in a
client. Entity rendering, persistence and completion should also be checked
in-game against the target server.
