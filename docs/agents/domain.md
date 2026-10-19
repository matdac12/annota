# Domain docs

Single context: one `GLOSSARY.md` at the repo root, and `docs/adr/` once the first ADR is written.

- **Before exploring**, read `GLOSSARY.md` and any ADR in `docs/adr/` that touches the area. If `docs/adr/` doesn't exist yet, carry on.
- **Use the glossary's terms** (Snap, Region, Overlay, Editor, Box, Note, Note field, Callout, Resident) in code, issue titles, test names and commits. A concept with no term is a signal: either the language is invented (reconsider) or there's a real gap (note it for `/domain-modeling`).
- **Flag ADR conflicts** explicitly rather than overriding silently: "Contradicts ADR-000N, but worth reopening because…"
