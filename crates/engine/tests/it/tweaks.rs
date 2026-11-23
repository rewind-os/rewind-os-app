//! Every setting must be fully reversible and never left half applied.

use rewind_os_engine::Result;
use rewind_os_engine::tweaks::{self, CATALOG, Change, Data, State, Store};
use std::cell::RefCell;
use std::collections::HashMap;

/// An in-memory registry. `fail_after` makes the n-th write fail, like a denied key would.
#[derive(Default)]
struct Memory {
    values: RefCell<HashMap<(String, String), Data>>,
    writes: RefCell<usize>,
    fail_after: Option<usize>,
}

fn key(c: &Change) -> (String, String) {
    (c.path.to_owned(), c.name.to_owned())
}

impl Store for Memory {
    fn read(&self, c: &Change) -> Option<Data> {
        self.values.borrow().get(&key(c)).copied()
    }

    fn write(&self, c: &Change, data: Data) -> Result<()> {
        let mut writes = self.writes.borrow_mut();
        *writes += 1;
        if self.fail_after.is_some_and(|n| *writes > n) {
            return Err(rewind_os_engine::EngineError::NeedsAdmin);
        }
        self.values.borrow_mut().insert(key(c), data);
        Ok(())
    }

    fn delete(&self, c: &Change) -> Result<()> {
        self.values.borrow_mut().remove(&key(c));
        Ok(())
    }
}

#[test]
fn every_setting_applies_and_undoes_back_to_the_default() {
    let store = Memory::default();
    for tweak in CATALOG {
        assert_eq!(tweaks::state(&store, tweak), State::Default, "{}", tweak.id);
        tweaks::set_in(&store, tweak, true).unwrap();
        assert_eq!(tweaks::state(&store, tweak), State::Applied, "{}", tweak.id);
        tweaks::set_in(&store, tweak, false).unwrap();
        assert_eq!(tweaks::state(&store, tweak), State::Default, "{}", tweak.id);
    }
}

#[test]
fn a_failure_halfway_puts_back_what_was_written() {
    let tweak = CATALOG.iter().find(|t| t.changes.len() > 2).unwrap();
    let store = Memory {
        fail_after: Some(2),
        ..Memory::default()
    };
    let before = store.values.borrow().clone();
    assert!(tweaks::set_in(&store, tweak, true).is_err());
    assert_eq!(tweaks::state(&store, tweak), State::Default);
    assert_eq!(
        *store.values.borrow(),
        before,
        "values written before the failure must go back"
    );
}

#[test]
fn a_value_someone_else_changed_shows_as_partial_not_applied() {
    let tweak = CATALOG.iter().find(|t| t.changes.len() > 1).unwrap();
    let store = Memory::default();
    store.write(&tweak.changes[0], tweak.changes[0].on).unwrap();
    assert_eq!(tweaks::state(&store, tweak), State::Partial);
}

#[test]
fn ids_are_unique() {
    let mut ids: Vec<_> = CATALOG.iter().map(|t| t.id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total);
}
