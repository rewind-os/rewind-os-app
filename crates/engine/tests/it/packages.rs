use rewind_os_engine::packages::{
    parse_dpkg, parse_flatpak, parse_pacman, parse_rpm, parse_size, parse_snap,
};
use rewind_os_engine::programs::Scope;
use std::collections::HashSet;

#[test]
fn sizes_in_any_unit_become_bytes() {
    assert_eq!(parse_size("800 B"), Some(800));
    assert!(parse_size("1.5 MiB") > parse_size("1.5 MB"));
    assert!(parse_size("2,0 GB") > parse_size("900 MB"));
    assert_eq!(parse_size("unknown"), None);
}

#[test]
fn dpkg_keeps_programs_installed_on_purpose_and_drops_the_base_system_and_libraries() {
    let out = "gh\t2.4\t40000\tGitHub <x@y>\tii \toptional\tutils\n\
               bash\t5.2\t7000\tMatthias <m@x>\tii \trequired\tshells\n\
               libssl3:amd64\t3.0\t6000\tDebian <d@x>\tii \toptional\tlibs\n\
               old-tool\t1.0\t10\tSomeone\trc \toptional\tutils\n\
               curl\t8.0\t500\tDebian <d@x>\tii \toptional\tweb\n";
    let manual: HashSet<String> = ["gh", "bash", "libssl3", "old-tool"]
        .into_iter()
        .map(String::from)
        .collect();
    let names: Vec<String> = parse_dpkg(out, Some(&manual))
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert_eq!(names, vec!["gh"], "curl was installed as a dependency");
    let all = parse_dpkg(out, None);
    let gh = all.iter().find(|p| p.name == "gh").unwrap();
    assert_eq!(gh.publisher.as_deref(), Some("GitHub"));
    assert!(gh.estimated_bytes > Some(40_000));
}

#[test]
fn rpm_pacman_flatpak_and_snap_outputs_are_read() {
    let rpm = parse_rpm("firefox\t120.0-1.fc39\t250000000\tFedora Project\t1700000000\n");
    assert_eq!(rpm.len(), 1);
    assert!(rpm[0].installed_on.is_some());

    let pacman = parse_pacman(
        "Name            : vlc\nVersion         : 3.0.20-1\nInstalled Size  : 64.12 MiB\nPackager        : Arch <a@b>\n\n\
         Name            : git\nVersion         : 2.43.0-1\nInstalled Size  : 25.40 MiB\n",
    );
    assert_eq!(pacman.len(), 2);
    assert!(pacman[0].estimated_bytes > Some(60 * 1024 * 1024));

    let flatpak = parse_flatpak(
        "org.gimp.GIMP\tGNU Image Manipulation Program\t2.10.36\t520.1 MB\tflathub\tuser\n",
    );
    assert_eq!(flatpak[0].name, "GNU Image Manipulation Program");
    assert_eq!(flatpak[0].scope, Scope::User);
    assert!(flatpak[0].key.contains("org.gimp.GIMP"));

    let snap = parse_snap(
        "Name    Version   Rev    Tracking       Publisher   Notes\n\
         core22  20240111  1122   latest/stable  canonical✓  base\n\
         snapd   2.61      20671  latest/stable  canonical✓  snapd\n\
         spotify 1.2.26    74     latest/stable  spotify✓    -\n",
    );
    let names: Vec<&str> = snap.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, vec!["spotify"]);
    assert_eq!(snap[0].publisher.as_deref(), Some("spotify"));
}

#[test]
fn an_apt_removal_that_takes_other_packages_is_seen() {
    use rewind_os_engine::packages::apt_removals;
    let alone = "NOTE: simulation\nRemv neofetch [7.1.0-4]\n";
    assert_eq!(apt_removals(alone), vec!["neofetch"]);
    let cascade =
        "Remv python3-pip [23.0]\nRemv python3:amd64 [3.11.2-1]\nRemv python3-venv [3.11]\n";
    let removed = apt_removals(cascade);
    assert!(removed.len() > 1 && removed.contains(&"python3".to_owned()));
}

#[test]
fn package_names_can_never_become_options() {
    use rewind_os_engine::packages::safe_package_name;
    assert!(safe_package_name("python3.11-venv"));
    assert!(safe_package_name("libstdc++6:amd64"));
    assert!(!safe_package_name("--purge"));
    assert!(!safe_package_name("foo bar"));
    assert!(!safe_package_name("x;rm -rf /"));
}
