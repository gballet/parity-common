// Copyright 2015-2018 Parity Technologies (UK) Ltd.
// This file is part of Parity.

// Parity is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Parity is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Parity.  If not, see <http://www.gnu.org/licenses/>.

use kvdb::{DBOp, DBTransaction, DBValue, KeyValueDB};
use parking_lot::RwLock;
use std::{
	collections::{BTreeMap, HashMap},
	io,
};

/// A key-value database fulfilling the `KeyValueDB` trait, living in memory.
/// This is generally intended for tests and is not particularly optimized.
#[derive(Default)]
pub struct InMemory {
	columns: RwLock<HashMap<u32, BTreeMap<Vec<u8>, DBValue>>>,
}

/// Create an in-memory database with the given number of columns.
/// Columns will be indexable by 0..`num_cols`
pub fn create(num_cols: u32) -> InMemory {
	let mut cols = HashMap::new();

	for idx in 0..num_cols {
		cols.insert(idx, BTreeMap::new());
	}

	InMemory { columns: RwLock::new(cols) }
}

impl KeyValueDB for InMemory {
	fn get(&self, col: u32, key: &[u8]) -> io::Result<Option<DBValue>> {
		let columns = self.columns.read();
		match columns.get(&col) {
			None => Err(io::Error::new(io::ErrorKind::Other, format!("No such column family: {:?}", col))),
			Some(map) => Ok(map.get(key).cloned()),
		}
	}

	fn get_by_prefix(&self, col: u32, prefix: &[u8]) -> Option<Box<[u8]>> {
		let columns = self.columns.read();
		match columns.get(&col) {
			None => None,
			Some(map) => {
				map.iter().find(|&(ref k, _)| k.starts_with(prefix)).map(|(_, v)| v.to_vec().into_boxed_slice())
			}
		}
	}

	fn write_buffered(&self, transaction: DBTransaction) {
		let mut columns = self.columns.write();
		let ops = transaction.ops;
		for op in ops {
			match op {
				DBOp::Insert { col, key, value } => {
					if let Some(col) = columns.get_mut(&col) {
						col.insert(key.into_vec(), value);
					}
				}
				DBOp::Delete { col, key } => {
					if let Some(col) = columns.get_mut(&col) {
						col.remove(&*key);
					}
				}
			}
		}
	}

	fn flush(&self) -> io::Result<()> {
		Ok(())
	}

	fn iter<'a>(&'a self, col: u32) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
		match self.columns.read().get(&col) {
			Some(map) => Box::new(
				// TODO: worth optimizing at all?
				map.clone().into_iter().map(|(k, v)| (k.into_boxed_slice(), v.into_vec().into_boxed_slice())),
			),
			None => Box::new(None.into_iter()),
		}
	}

	fn iter_from_prefix<'a>(
		&'a self,
		col: u32,
		prefix: &'a [u8],
	) -> Box<dyn Iterator<Item = (Box<[u8]>, Box<[u8]>)> + 'a> {
		match self.columns.read().get(&col) {
			Some(map) => Box::new(
				map.clone()
					.into_iter()
					.filter(move |&(ref k, _)| k.starts_with(prefix))
					.map(|(k, v)| (k.into_boxed_slice(), v.into_vec().into_boxed_slice())),
			),
			None => Box::new(None.into_iter()),
		}
	}

	fn restore(&self, _new_db: &str) -> io::Result<()> {
		Err(io::Error::new(io::ErrorKind::Other, "Attempted to restore in-memory database"))
	}
}

#[cfg(test)]
mod tests {
	use super::{create, KeyValueDB};

	#[test]
	fn get_fails_with_non_existing_column() {
		let db = create(1);
		assert!(db.get(1, &[]).is_err());
	}

	#[test]
	fn put_and_get() {
		let db = create(1);

		let key1 = b"key1";

		let mut transaction = db.transaction();
		transaction.put(0, key1, b"horse");
		db.write_buffered(transaction);
		assert_eq!(&*db.get(0, key1).unwrap().unwrap(), b"horse");
	}

	#[test]
	fn delete_and_get() {
		let db = create(1);

		let key1 = b"key1";

		let mut transaction = db.transaction();
		transaction.put(0, key1, b"horse");
		db.write_buffered(transaction);
		assert_eq!(&*db.get(0, key1).unwrap().unwrap(), b"horse");

		let mut transaction = db.transaction();
		transaction.delete(0, key1);
		db.write_buffered(transaction);
		assert!(db.get(0, key1).unwrap().is_none());
	}

	#[test]
	fn iter() {
		let db = create(1);

		let key1 = b"key1";
		let key2 = b"key2";

		let mut transaction = db.transaction();
		transaction.put(0, key1, key1);
		transaction.put(0, key2, key2);
		db.write_buffered(transaction);

		let contents: Vec<_> = db.iter(0).into_iter().collect();
		assert_eq!(contents.len(), 2);
		assert_eq!(&*contents[0].0, key1);
		assert_eq!(&*contents[0].1, key1);
		assert_eq!(&*contents[1].0, key2);
		assert_eq!(&*contents[1].1, key2);
	}

	#[test]
	fn iter_from_prefix() {
		let db = create(1);

		let key1 = b"0";
		let key2 = b"a";
		let key3 = b"ab";

		let mut transaction = db.transaction();
		transaction.put(0, key1, key1);
		transaction.put(0, key2, key2);
		transaction.put(0, key3, key3);
		db.write_buffered(transaction);

		let contents: Vec<_> = db.iter_from_prefix(0, b"").into_iter().collect();
		assert_eq!(contents.len(), 3);
		assert_eq!(&*contents[0].0, key1);
		assert_eq!(&*contents[0].1, key1);
		assert_eq!(&*contents[1].0, key2);
		assert_eq!(&*contents[1].1, key2);
		assert_eq!(&*contents[2].0, key3);
		assert_eq!(&*contents[2].1, key3);

		let contents: Vec<_> = db.iter_from_prefix(0, b"0").into_iter().collect();
		assert_eq!(contents.len(), 1);
		assert_eq!(&*contents[0].0, key1);
		assert_eq!(&*contents[0].1, key1);

		let contents: Vec<_> = db.iter_from_prefix(0, b"a").into_iter().collect();
		assert_eq!(contents.len(), 2);
		assert_eq!(&*contents[0].0, key2);
		assert_eq!(&*contents[0].1, key2);
		assert_eq!(&*contents[1].0, key3);
		assert_eq!(&*contents[1].1, key3);

		let contents: Vec<_> = db.iter_from_prefix(0, b"ab").into_iter().collect();
		assert_eq!(contents.len(), 1);
		assert_eq!(&*contents[0].0, key3);
		assert_eq!(&*contents[0].1, key3);

		let contents: Vec<_> = db.iter_from_prefix(0, b"abc").into_iter().collect();
		assert_eq!(contents.len(), 0);
	}
}
