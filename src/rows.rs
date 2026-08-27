//! Builds the main picker's rows. Catalog rows come first, in catalog order
//! (the file's order is the presentation order). Plugin action rows are
//! appended by M3, sorted among themselves by label.

use crate::catalog::Catalog;
use crate::keys::KeyHints;
use crate::ui::Row;

pub fn catalog_rows(catalog: &Catalog, hints: &KeyHints) -> Vec<Row> {
    catalog
        .commands
        .iter()
        .map(|command| Row {
            id: command.id.clone(),
            label: command.title.clone(),
            hint: hints.native(command.keys_action.as_deref(), command.key.as_deref()),
        })
        .collect()
}
