//! Database rows (config group 38): the tables client scripts query with `db_find` and
//! `db_getfield`. Only rows are decoded (a row says which table it belongs to and holds, per
//! column, a list of tuples of values).

use crate::buf::Buf;
use crate::{Cache, Result};
use std::collections::HashMap;

/// One value of a row.
#[derive(Debug, Clone, PartialEq)]
pub enum DbValue {
    Int(i32),
    Str(String),
}

impl DbValue {
    pub fn int(&self) -> Option<i32> {
        match self {
            DbValue::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            DbValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// One row: for each column, its repeated tuples (each tuple one value per type of the column).
#[derive(Debug, Clone, Default)]
pub struct DbRow {
    pub id: u32,
    pub table: u32,
    pub columns: HashMap<u32, Vec<Vec<DbValue>>>,
}

impl DbRow {
    pub fn decode(id: u32, data: &[u8]) -> DbRow {
        let mut b = Buf::new(data);
        let mut row = DbRow { id, ..Default::default() };
        loop {
            match b.u8() {
                0 => break,
                3 => {
                    let _count = b.u8();
                    loop {
                        let column = b.u8() as u32;
                        if column == 255 {
                            break;
                        }
                        let types: Vec<u32> = (0..b.u8()).map(|_| b.smart_u()).collect();
                        let fields = b.smart_u();
                        let mut tuples = Vec::new();
                        for _ in 0..fields {
                            tuples.push(
                                types
                                    .iter()
                                    .map(|t| if *t == 36 { DbValue::Str(b.string()) } else { DbValue::Int(b.i32()) })
                                    .collect(),
                            );
                        }
                        row.columns.insert(column, tuples);
                    }
                }
                4 => row.table = b.smart_u(),
                _ => break,
            }
        }
        row
    }

    /// The tuple at `index` of `column`.
    pub fn tuple(&self, column: u32, index: usize) -> Option<&[DbValue]> {
        self.columns.get(&column)?.get(index).map(Vec::as_slice)
    }
}

/// Every row of the cache, by table.
pub struct DbTables {
    rows: Vec<DbRow>,
}

impl DbTables {
    pub fn load(cache: &Cache) -> Result<DbTables> {
        let cfg = cache.index(2)?;
        let rows = cache.read_files(&cfg, 38)?.into_iter().map(|(id, d)| DbRow::decode(id, &d)).collect();
        Ok(DbTables { rows })
    }

    pub fn rows_of_all(&self) -> impl Iterator<Item = &DbRow> {
        self.rows.iter()
    }

    pub fn rows_of(&self, table: u32) -> impl Iterator<Item = &DbRow> {
        self.rows.iter().filter(move |r| r.table == table)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_has_its_table_and_tuples_per_column() {
        // table 78; column 0: one int tuple (5); column 1: two (int, string) tuples
        let mut d = vec![4, 78, 3, 2, 0, 1, 0, 1, 0, 0, 0, 5];
        d.extend([1, 2, 0, 36, 2, 0, 0, 0, 1, b'A', 0, 0, 0, 0, 3, b'B', 0]);
        d.extend([255, 0]);
        let row = DbRow::decode(7, &d);
        assert_eq!(row.table, 78);
        assert_eq!(row.tuple(0, 0), Some(&[DbValue::Int(5)][..]));
        assert_eq!(row.tuple(1, 0), Some(&[DbValue::Int(1), DbValue::Str("A".into())][..]));
        assert_eq!(row.tuple(1, 1), Some(&[DbValue::Int(3), DbValue::Str("B".into())][..]));
        assert_eq!(row.tuple(1, 2), None);
    }
}
