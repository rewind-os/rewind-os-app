use rewind_os_engine::cleanup::{self, CategoryReport};
use rewind_os_engine::recommend::{Action, DriveFact, Facts, Level, rules};
use std::path::PathBuf;

const GB: u64 = 1024 * 1024 * 1024;

fn report(id: &str, bytes: u64) -> CategoryReport {
    let category = cleanup::LINUX
        .iter()
        .find(|c| c.id == id)
        .copied()
        .unwrap_or(cleanup::LINUX[0]);
    CategoryReport {
        category,
        files: 1,
        bytes,
        present: true,
        blocked: false,
        note: None,
    }
}

fn drive(total_gb: u64, free_gb: u64) -> DriveFact {
    DriveFact {
        mount: PathBuf::from("/"),
        total: total_gb * GB,
        available: free_gb * GB,
        removable: false,
    }
}

#[test]
fn a_healthy_computer_gets_no_advice() {
    let facts = Facts {
        drives: vec![drive(500, 300)],
        startup_enabled: Some(3),
        ..Facts::default()
    };
    assert!(rules(&facts).is_empty());
}

#[test]
fn a_full_drive_comes_first_and_points_at_the_analyzer() {
    let facts = Facts {
        drives: vec![drive(500, 8)],
        cleanup: vec![report("linux.thumbnails", 2 * GB)],
        startup_enabled: Some(20),
        ..Facts::default()
    };
    let advice = rules(&facts);
    assert!(advice.len() >= 3);
    assert_eq!(advice[0].level, Level::Important);
    assert!(matches!(advice[0].action, Action::Analyze { .. }));
    assert!(advice.windows(2).all(|w| w[0].level <= w[1].level));
}

#[test]
fn junk_advice_cleans_only_the_safe_categories_it_measured() {
    let facts = Facts {
        drives: vec![drive(500, 300)],
        cleanup: vec![
            report("linux.thumbnails", GB),
            report("developer.package_caches", GB),
            report("browsers.chrome", 0),
        ],
        ..Facts::default()
    };
    let advice = rules(&facts);
    let junk = advice.iter().find(|r| r.id == "junk").unwrap();
    let Action::Clean { categories } = &junk.action else {
        panic!("junk advice must clean")
    };
    assert!(categories.contains(&"linux.thumbnails".to_owned()));
    assert!(
        !categories.contains(&"developer.package_caches".to_owned()),
        "not a default category"
    );
    assert!(
        !categories.contains(&"browsers.chrome".to_owned()),
        "nothing to clean there"
    );
}

#[test]
fn blocked_categories_are_not_counted_as_savings() {
    let mut blocked = report("linux.thumbnails", 5 * GB);
    blocked.blocked = true;
    let facts = Facts {
        drives: vec![drive(500, 300)],
        cleanup: vec![blocked],
        ..Facts::default()
    };
    assert!(rules(&facts).iter().all(|r| r.id != "junk"));
}

#[test]
fn small_empty_volumes_are_not_full() {
    let facts = Facts {
        drives: vec![drive(4, 4), drive(2, 1)],
        ..Facts::default()
    };
    assert!(rules(&facts).iter().all(|r| r.id != "drive_full"));
}

#[test]
fn duplicates_and_leftovers_are_advised_only_when_they_free_real_space() {
    let downloads = PathBuf::from("/home/me/Downloads");
    let small = Facts {
        drives: vec![drive(500, 300)],
        download_duplicates: Some((downloads.clone(), 20 * 1024 * 1024)),
        leftovers: Some((3, 40 * 1024 * 1024)),
        ..Facts::default()
    };
    assert!(rules(&small).is_empty());

    let large = Facts {
        download_duplicates: Some((downloads.clone(), 3 * GB)),
        leftovers: Some((4, 2 * GB)),
        ..small
    };
    let advice = rules(&large);
    assert!(
        advice
            .iter()
            .any(|r| matches!(&r.action, Action::Duplicates { path } if *path == downloads))
    );
    assert!(advice.iter().any(|r| r.action == Action::Leftovers));
}
