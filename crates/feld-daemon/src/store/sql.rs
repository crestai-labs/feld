//! The model's enums as SQLite text, in the words the API uses.

use std::str::FromStr;

use rusqlite::ToSql;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSqlOutput, ValueRef};

use feld_core::model::{ActorKind, EndKind, ObservationKind, Ownership, RecordKind, StartKind};

/// One of the model's enums as the text of a column: `params![Text(kind)]` writes it and
/// `row.get::<_, Text<ActorKind>>(0)?.0` reads it.
#[derive(Clone, Copy, Debug)]
pub struct Text<T>(pub T);

/// An enum stored as the text [`Text`] writes and reads.
pub trait TextEnum: Copy + FromStr<Err = String> {
    fn as_text(self) -> &'static str;
}

macro_rules! text_enum {
    ($($kind:ty),* $(,)?) => {$(
        impl TextEnum for $kind {
            fn as_text(self) -> &'static str {
                self.as_str()
            }
        }
    )*};
}

text_enum!(
    ActorKind,
    StartKind,
    Ownership,
    EndKind,
    ObservationKind,
    RecordKind,
);

impl<T: TextEnum> ToSql for Text<T> {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.0.as_text()))
    }
}

impl<T: TextEnum> FromSql for Text<T> {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        value
            .as_str()?
            .parse()
            .map(Text)
            .map_err(|message: String| FromSqlError::Other(message.into()))
    }
}
