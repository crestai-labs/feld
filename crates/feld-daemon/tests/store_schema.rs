use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use feld_daemon::store::Store;
use rusqlite::Connection;

/// The shape of a table: its columns, constraints, indexes and foreign keys.
#[derive(Debug, PartialEq)]
struct Table {
    /// The expressions of the CHECK constraints, their whitespace collapsed.
    checks: BTreeSet<String>,
    autoincrement: bool,
    /// Name, type, NOT NULL, default and position in the primary key.
    columns: BTreeSet<(String, String, bool, Option<String>, u32)>,
    indexes: BTreeSet<Index>,
    /// Table, column, referenced column, ON UPDATE and ON DELETE.
    foreign_keys: BTreeSet<(String, String, Option<String>, String, String)>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Index {
    /// None for an index SQLite names after the table, which a renamed table keeps.
    name: Option<String>,
    columns: Vec<Option<String>>,
    unique: bool,
    origin: String,
    /// The condition of a partial index, its whitespace collapsed.
    condition: Option<String>,
}

/// The shape of the tables of the store at `path`, after the store opens it.
fn shape(path: &Path) -> BTreeMap<String, Table> {
    drop(Store::open(path).unwrap());
    let connection = Connection::open(path).unwrap();
    let strings = |sql: &str, parameters: &[&str]| -> Vec<String> {
        let mut statement = connection.prepare(sql).unwrap();
        statement
            .query_map(rusqlite::params_from_iter(parameters), |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    let mut tables = BTreeMap::new();
    for table in strings(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        &[],
    ) {
        let columns = connection
            .prepare("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(?1)")
            .unwrap()
            .query_map([&table], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let indexes = connection
            .prepare("SELECT name, \"unique\", origin, partial FROM pragma_index_list(?1)")
            .unwrap()
            .query_map([&table], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get::<_, bool>(3)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|(name, unique, origin, partial)| Index {
                columns: connection
                    .prepare("SELECT name FROM pragma_index_info(?1) ORDER BY seqno")
                    .unwrap()
                    .query_map([&name], |row| row.get(0))
                    .unwrap()
                    .collect::<rusqlite::Result<_>>()
                    .unwrap(),
                condition: partial.then(|| {
                    let sql = &strings(
                        "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                        &[&name],
                    )[0];
                    let at = sql.find(" WHERE ").unwrap();
                    sql[at + " WHERE ".len()..]
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                }),
                name: (!name.starts_with("sqlite_autoindex_")).then_some(name),
                unique,
                origin,
            })
            .collect();
        let foreign_keys = connection
            .prepare(
                "SELECT \"table\", \"from\", \"to\", on_update, on_delete FROM pragma_foreign_key_list(?1)",
            )
            .unwrap()
            .query_map([&table], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let sql = &strings(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
            &[&table],
        )[0];
        tables.insert(
            table,
            Table {
                checks: checks(sql),
                autoincrement: sql.contains(" AUTOINCREMENT"),
                columns,
                indexes,
                foreign_keys,
            },
        );
    }
    tables
}

/// The expressions of the CHECK constraints of the `CREATE TABLE` statement `sql`, which
/// SQLite keeps as written and rewrites only for a renamed column.
fn checks(sql: &str) -> BTreeSet<String> {
    sql.match_indices("CHECK")
        .map(|(at, keyword)| {
            let expression = sql[at + keyword.len()..].trim_start();
            let mut depth = 0;
            let end = expression
                .find(|character| {
                    match character {
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    depth == 0
                })
                .unwrap();
            expression[1..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

#[test]
fn a_migrated_store_has_the_shape_of_a_new_store() {
    let temporary = tempfile::tempdir().unwrap();
    let new = temporary.path().join("new.db");
    let migrated = temporary.path().join("migrated.db");
    Connection::open(&migrated)
        .unwrap()
        .execute_batch(&format!(
            "{}PRAGMA user_version = 5;",
            include_str!("fixtures/schema_5.sql")
        ))
        .unwrap();

    let new = shape(&new);
    let migrated = shape(&migrated);
    assert!(new["records"].autoincrement);
    assert!(
        new["records"]
            .checks
            .iter()
            .any(|check| check.contains("'schedule.missed'"))
    );
    assert_eq!(
        migrated.keys().collect::<Vec<_>>(),
        new.keys().collect::<Vec<_>>()
    );
    for (name, table) in &new {
        assert_eq!(&migrated[name], table, "the table {name}");
    }
}
