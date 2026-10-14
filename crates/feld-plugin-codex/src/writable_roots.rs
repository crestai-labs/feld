//! The spool in Codex's `config.toml`: the `writable_roots` of `[sandbox_workspace_write]`
//! that lets a `workspace-write` sandbox write the requests of `feld` into it
//! (`docs/spec/design/spool.md`). The document is edited in place, so every other root, key
//! and comment stays as the person wrote it.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use feld_core::home::FeldHome;
use serde::{Deserialize, Serialize};
use toml_edit::{Array, DocumentMut, Item, Table, TableLike, Value};

const TABLE: &str = "sandbox_workspace_write";
const KEY: &str = "writable_roots";

/// What adding the spool brought into a `config.toml`, which a restore takes away again: the
/// element, with the key or the table that holds it once nothing else is left there.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Added {
    /// The element, in a list that was there.
    Element,
    /// The `writable_roots` key, in a table that was there.
    Key,
    /// The `[sandbox_workspace_write]` table.
    Table,
}

/// Codex's `config.toml` as it is, empty when there is none.
pub fn read(path: &Path) -> Result<DocumentMut> {
    match fs::read_to_string(path) {
        Ok(text) => text
            .parse()
            .with_context(|| format!("decode {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(DocumentMut::new()),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// The writable roots the document lists, refused when they are not an array of paths.
pub fn roots(document: &DocumentMut) -> Result<Vec<String>> {
    let Some(roots) = table(document).and_then(|table| table.get(KEY)) else {
        return Ok(Vec::new());
    };
    roots
        .as_array()
        .context("writable_roots of Codex config.toml must be an array")?
        .iter()
        .map(|root| {
            root.as_str()
                .map(str::to_owned)
                .context("writable_roots of Codex config.toml must hold paths")
        })
        .collect()
}

/// Whether a root the document lists lets the sandbox write into `spool`.
pub fn covers(document: &DocumentMut, spool: &Path) -> Result<bool> {
    Ok(roots(document)?
        .iter()
        .any(|root| feld_core::spool::covered_by(spool, Path::new(root))))
}

/// Whether the document lists `path` itself among the writable roots.
pub fn lists(document: &DocumentMut, path: &Path) -> Result<bool> {
    let path = path.display().to_string();
    Ok(roots(document)?.contains(&path))
}

/// Puts `next` where the writable roots list `current`, and takes out any other `current`;
/// when `next` is listed already, `current` only goes.
pub fn replace(document: &mut DocumentMut, current: &Path, next: &Path) -> Result<()> {
    let current = current.display().to_string();
    let listed_next = lists(document, next)?;
    let Some(roots) = document
        .get_mut(TABLE)
        .and_then(Item::as_table_like_mut)
        .and_then(|table| table.get_mut(KEY))
        .and_then(Item::as_array_mut)
    else {
        bail!("writable_roots of Codex config.toml must be an array");
    };
    let listed: Vec<usize> = roots
        .iter()
        .enumerate()
        .filter(|(_, root)| root.as_str() == Some(current.as_str()))
        .map(|(index, _)| index)
        .collect();
    let Some((&first, rest)) = listed.split_first() else {
        bail!("writable_roots of Codex config.toml do not list {current}");
    };
    for &index in rest.iter().rev() {
        roots.remove(index);
    }
    if listed_next {
        roots.remove(first);
        return Ok(());
    }
    let decor = roots.get(first).map(|root| root.decor().clone());
    roots.replace(first, next.display().to_string());
    if let (Some(decor), Some(root)) = (decor, roots.get_mut(first)) {
        *root.decor_mut() = decor;
    }
    Ok(())
}

/// Adds `spool` to the writable roots, creating the key or the table when it is not there.
pub fn add(document: &mut DocumentMut, spool: &Path) -> Result<Added> {
    let mut added = Added::Element;
    if !document.contains_key(TABLE) {
        let mut table = Table::new();
        table.set_implicit(false);
        document.insert(TABLE, Item::Table(table));
        added = Added::Table;
    }
    let table = document[TABLE]
        .as_table_like_mut()
        .context("sandbox_workspace_write of Codex config.toml must be a table")?;
    if !table.contains_key(KEY) {
        table.insert(KEY, Item::Value(Value::Array(Array::new())));
        if added == Added::Element {
            added = Added::Key;
        }
    }
    let Some(roots) = table.get_mut(KEY).and_then(Item::as_array_mut) else {
        bail!("writable_roots of Codex config.toml must be an array");
    };
    roots.push(spool.display().to_string());
    Ok(added)
}

/// Takes the one element `spool` that adding it brought out of the writable roots, and the
/// key or the table `added` names once nothing is left in it: whether there was a spool to
/// take out.
pub fn remove(document: &mut DocumentMut, spool: &Path, added: Added) -> bool {
    let spool = spool.display().to_string();
    let Some(table) = document.get_mut(TABLE).and_then(Item::as_table_like_mut) else {
        return false;
    };
    let Some(roots) = table.get_mut(KEY).and_then(Item::as_array_mut) else {
        return false;
    };
    let Some(index) = roots
        .iter()
        .enumerate()
        .filter(|(_, root)| root.as_str() == Some(spool.as_str()))
        .map(|(index, _)| index)
        .last()
    else {
        return false;
    };
    roots.remove(index);
    // The element that comes first keeps no space before it.
    if index == 0
        && let Some(first) = roots.get_mut(0)
    {
        first.decor_mut().set_prefix("");
    }
    if added != Added::Element && roots.is_empty() {
        table.remove(KEY);
    }
    if added == Added::Table && table.is_empty() {
        document.remove(TABLE);
    }
    true
}

/// Writes the document back as a person's file.
pub fn write(home: &FeldHome, path: &Path, document: &DocumentMut) -> Result<()> {
    home.write_person_file(path, document.to_string().as_bytes())
}

fn table(document: &DocumentMut) -> Option<&dyn TableLike> {
    document.get(TABLE).and_then(Item::as_table_like)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPOOL: &str = "/Users/alice/.feld/spool";

    fn edited(text: &str) -> (String, Added) {
        let mut document: DocumentMut = text.parse().unwrap();
        let created = add(&mut document, Path::new(SPOOL)).unwrap();
        (document.to_string(), created)
    }

    fn removed(text: &str, created: Added) -> String {
        let mut document: DocumentMut = text.parse().unwrap();
        assert!(remove(&mut document, Path::new(SPOOL), created));
        document.to_string()
    }

    #[test]
    fn the_spool_joins_the_roots_a_person_listed_and_leaves_with_them_kept() {
        let person = "# mine\nmodel = \"o3\"\n\n[sandbox_workspace_write]\n# roots\nwritable_roots = [\"/work\"] # trailing\nnetwork_access = false\n";
        let (added, created) = edited(person);
        assert_eq!(created, Added::Element);
        assert_eq!(
            added,
            "# mine\nmodel = \"o3\"\n\n[sandbox_workspace_write]\n# roots\nwritable_roots = [\"/work\", \"/Users/alice/.feld/spool\"] # trailing\nnetwork_access = false\n"
        );
        assert_eq!(removed(&added, created), person);
    }

    #[test]
    fn a_table_or_key_the_spool_brought_goes_once_empty() {
        let person = "model = \"o3\"\n";
        let (added, created) = edited(person);
        assert_eq!(created, Added::Table);
        assert_eq!(
            added,
            "model = \"o3\"\n\n[sandbox_workspace_write]\nwritable_roots = [\"/Users/alice/.feld/spool\"]\n"
        );
        assert_eq!(removed(&added, created), person);

        let person = "[sandbox_workspace_write]\nnetwork_access = true\n";
        let (added, created) = edited(person);
        assert_eq!(created, Added::Key);
        assert_eq!(removed(&added, created), person);
        // A key a person put there stays, empty or not.
        assert_eq!(
            removed(&added, Added::Element),
            "[sandbox_workspace_write]\nnetwork_access = true\nwritable_roots = []\n"
        );
    }

    #[test]
    fn a_dotted_or_inline_table_takes_the_spool_where_it_is() {
        let (added, _) = edited("sandbox_workspace_write.writable_roots = [\"/work\"]\n");
        assert_eq!(
            added,
            "sandbox_workspace_write.writable_roots = [\"/work\", \"/Users/alice/.feld/spool\"]\n"
        );
        let (added, created) = edited("sandbox_workspace_write = { network_access = true }\n");
        assert_eq!(created, Added::Key);
        assert_eq!(
            added,
            "sandbox_workspace_write = { network_access = true , writable_roots = [\"/Users/alice/.feld/spool\"] }\n"
        );
    }

    #[test]
    fn the_shared_spool_takes_the_place_of_the_whole_spool() {
        let mut document: DocumentMut = "[sandbox_workspace_write]\nwritable_roots = [\"/work\", \"/Users/alice/.feld/spool\", \"/tmp\"] # trailing\n"
            .parse()
            .unwrap();
        assert!(lists(&document, Path::new(SPOOL)).unwrap());
        let shared = Path::new("/Users/alice/.feld/spool/shared");
        assert!(!lists(&document, shared).unwrap());
        replace(&mut document, Path::new(SPOOL), shared).unwrap();
        assert_eq!(
            document.to_string(),
            "[sandbox_workspace_write]\nwritable_roots = [\"/work\", \"/Users/alice/.feld/spool/shared\", \"/tmp\"] # trailing\n"
        );
        assert!(!lists(&document, Path::new(SPOOL)).unwrap());
        assert!(replace(&mut document, Path::new(SPOOL), shared).is_err());
    }

    #[test]
    fn a_root_at_or_above_the_spool_covers_it() {
        for (roots, covered) in [
            ("[\"/work\"]", false),
            ("[\"/Users/alice/.feld/spool\"]", true),
            ("[\"/Users/alice/.feld\"]", true),
            ("[\"/Users/alice/.feldx\"]", false),
        ] {
            let document: DocumentMut =
                format!("[sandbox_workspace_write]\nwritable_roots = {roots}\n")
                    .parse()
                    .unwrap();
            assert_eq!(
                covers(&document, Path::new(SPOOL)).unwrap(),
                covered,
                "{roots}"
            );
        }
        let document: DocumentMut = "[sandbox_workspace_write]\nwritable_roots = \"/work\"\n"
            .parse()
            .unwrap();
        assert!(covers(&document, Path::new(SPOOL)).is_err());
    }
}
