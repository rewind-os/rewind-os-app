use rewind_os_engine::power::{Kind, Mode, parse_powercfg, parse_powerprofiles, windows_mode};
use rewind_os_engine::recommend::{Action, Facts, rules};

#[test]
fn powercfg_plans_are_known_by_their_guid_in_any_language() {
    // As this computer prints it: one plan, the active one.
    let english = "\r\nExisting Power Schemes (* Active)\r\n-----------------------------------\r\nPower Scheme GUID: 381b4222-f694-41f0-9685-ff5bb260df2e  (Balanced) *\r\n";
    let plans = parse_powercfg(english);
    assert_eq!(plans.len(), 1);
    assert_eq!((plans[0].kind, plans[0].active), (Kind::Balanced, true));

    let spanish = "Combinaciones de energía existentes (* Activo)\n\
        -----------------------------------\n\
        GUID de plan de energía: 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c  (Alto rendimiento)\n\
        GUID de plan de energía: 381b4222-f694-41f0-9685-ff5bb260df2e  (Equilibrado) *\n\
        GUID de plan de energía: a1841308-3540-4fab-bc81-f71973a5c3f8  (Economizador)\n\
        GUID de plan de energía: 0c1a2b3d-1111-2222-3333-444455556666  (Plan (fabricante))\n";
    let plans = parse_powercfg(spanish);
    let kinds: Vec<Kind> = plans.iter().map(|p| p.kind).collect();
    // Saver first, then balanced and performance, then the rest.
    assert_eq!(
        kinds,
        vec![Kind::Saver, Kind::Balanced, Kind::Performance, Kind::Custom]
    );
    assert_eq!(plans.iter().filter(|p| p.active).count(), 1);
    assert!(plans[1].active);
    // A custom plan keeps its own name, brackets and all.
    assert_eq!(plans[3].name, "Plan (fabricante)");
}

#[test]
fn the_power_mode_is_read_from_its_guid() {
    // As this computer stores it.
    assert_eq!(
        windows_mode("ded574b5-45a0-4f42-8737-46345c09c238"),
        Some(Mode::BestPerformance)
    );
    assert_eq!(
        windows_mode("00000000-0000-0000-0000-000000000000"),
        Some(Mode::Balanced)
    );
    assert_eq!(windows_mode("not a guid"), None);
}

#[test]
fn power_profiles_say_which_is_active_and_which_is_held_back() {
    let out = "  performance:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n    Degraded:   lap-detected\n\n\
               * balanced:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n\n\
               \x20 power-saver:\n    CpuDriver:\tintel_pstate\n    PlatformDriver:\tplatform_profile\n";
    let plans = parse_powerprofiles(out);
    let ids: Vec<&str> = plans.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, vec!["power-saver", "balanced", "performance"]);
    assert!(plans[1].active && !plans[0].active && !plans[2].active);
    assert_eq!(plans[2].degraded.as_deref(), Some("lap-detected"));
    assert!(plans[0].degraded.is_none());
}

#[test]
fn saving_energy_is_only_a_tip() {
    let tip = rules(&Facts {
        power_saving: true,
        ..Facts::default()
    });
    assert!(tip.iter().any(|r| r.action == Action::PowerPlan));
    assert!(
        rules(&Facts::default())
            .iter()
            .all(|r| r.action != Action::PowerPlan)
    );
}
