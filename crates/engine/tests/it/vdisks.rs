use rewind_os_engine::vdisks::{diskpart_script, parse_df_used, parse_names, plain_path};
use std::path::Path;

#[test]
fn wsl_names_are_read_from_its_utf16_output() {
    // As `wsl --list --running --quiet` printed it on a real machine: UTF-16 through a pipe.
    let raw = "D\0e\0b\0i\0a\0n\0\r\0\n\0";
    assert_eq!(parse_names(raw), vec!["Debian"]);
    assert!(parse_names("").is_empty());
    let two = "\u{feff}Debian\r\ndocker-desktop\r\n\r\n";
    assert_eq!(parse_names(two), vec!["Debian", "docker-desktop"]);
}

#[test]
fn what_linux_uses_is_the_number_under_the_header() {
    assert_eq!(parse_df_used("  Used\n81604378624\n"), Some(81_604_378_624));
    assert_eq!(parse_df_used("df: /: No such file\n"), None);
}

#[test]
fn diskpart_gets_the_disk_in_quotes_and_nothing_else() {
    let path = plain_path(r"\\?\C:\Users\me\AppData\Local\Docker\wsl\main");
    assert_eq!(
        path,
        Path::new(r"C:\Users\me\AppData\Local\Docker\wsl\main")
    );
    let script = diskpart_script(Path::new(r"C:\Users\me\ext4.vhdx")).expect("script");
    assert!(script.starts_with("select vdisk file=\"C:\\Users\\me\\ext4.vhdx\"\n"));
    // Read only: the disk is attached without being written to, then compacted and detached.
    assert!(script.contains("attach vdisk readonly\ncompact vdisk\ndetach vdisk"));
    // A path that could end the quotes or start another command is refused.
    assert!(diskpart_script(Path::new("C:\\a\"\nclean\n\"b.vhdx")).is_none());
    assert!(diskpart_script(Path::new("")).is_none());
}
