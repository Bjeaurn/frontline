# Trusted-client netcode for LAN play

Clients own their own movement, and the server decides hits. A client reports each shot (tick, origin, direction), and the server raycasts against current hitboxes and applies damage. Profiles and Loadouts stay client-local and are trusted. On a LAN, latency is about 1ms, so client prediction and lag compensation add little. Skipping them saves weeks against a party deadline, and cheating among friends is left to the honor system.

## Consequences

- Internet play or public servers need this revisited: server-authoritative movement, lightyear prediction and lag compensation, and server-side Profiles.
- Damage and score live in one place (the server), so "I shot first" disputes can't arise from client-side kill reporting.
