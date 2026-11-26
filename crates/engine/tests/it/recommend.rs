use rewind_os_engine::cleanup::{self, CategoryReport};
use rewind_os_engine::recommend::{Action, DriveFact, Facts, Level, VirtualDiskFacts, rules};
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

fn virtual_disk_advice(
    reclaimable: Option<u64>,
    unknown: u64,
) -> Vec<rewind_os_engine::recommend::Recommendation> {
    rules(&Facts {
        drives: vec![drive(500, 300)],
        virtual_disks: Some(VirtualDiskFacts {
            total: 80 * GB + unknown,
            reclaimable,
            unknown,
        }),
        ..Facts::default()
    })
    .into_iter()
    .filter(|r| r.action == Action::VirtualDisks)
    .collect()
}

#[test]
fn virtual_disks_are_advised_when_compacting_would_give_a_lot_back() {
    let worth = virtual_disk_advice(Some(20 * GB), 0);
    assert_eq!(worth.len(), 1);
    assert_eq!(worth[0].saving, Some(20 * GB));
    assert!(
        virtual_disk_advice(Some(GB), 0).is_empty(),
        "little to give back"
    );
    // A large disk whose use inside is not known (Docker) gets a tip, with no promise, even
    // next to a running distribution with little to give back.
    let unknown = virtual_disk_advice(Some(GB), 40 * GB);
    assert_eq!(unknown.len(), 1);
    assert!(
        unknown
            .iter()
            .all(|r| r.level == Level::Tip && r.saving.is_none())
    );
    assert!(virtual_disk_advice(None, 10 * GB).is_empty());
}

#[test]
fn restore_points_matter_more_when_the_drive_is_tight() {
    let advice = |free_gb, old| {
        rules(&Facts {
            drives: vec![drive(500, free_gb)],
            restore_points: Some((old, 12 * GB)),
            ..Facts::default()
        })
        .into_iter()
        .find(|r| r.action == Action::RestorePoints)
    };
    let roomy = advice(300, 4).map(|r| r.level);
    let tight = advice(60, 4).map(|r| r.level);
    assert!(tight.is_some() && roomy.is_some() && tight < roomy);
    assert!(
        advice(300, 0).is_none(),
        "only the newest is left, and it is kept"
    );
}
