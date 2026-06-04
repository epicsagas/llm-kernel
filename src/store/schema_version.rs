/// Trait for schema version management.
/// Implementors define the current schema version and DDL.
pub trait SchemaVersion {
    /// The expected schema version number.
    fn version() -> u32;

    /// The DDL to create/migrate the schema.
    fn ddl() -> &'static str;
}
