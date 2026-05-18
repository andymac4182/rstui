# Vendored A2UI v0.10 schema (provenance)

`basic_catalog.json` and `common_types.json` are verbatim copies of the
canonical Google A2UI v0.10 JSON Schema, fetched from
`github.com/google/A2UI@main` (`specification/v0_10/json/`).

They are embedded (`include_str!`) so the ACP client can send the agent a
**self-contained inline catalog** (`a2uiClientCapabilities.v0.10.inlineCatalogs`):
`capability::a2ui_inline_catalog()` merges `common_types.json`'s `$defs`
into the catalog and rewrites `common_types.json#/$defs/X` refs to local
`#/$defs/X`, so the catalog an agent receives resolves with no external
fetch. Re-sync from upstream if the spec rev changes.
