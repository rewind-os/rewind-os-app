//! Battery health: how much charge each battery still holds against what it was made to hold,
//! and how many charge cycles it has done. Read only, from each system's own report:
//! Windows' battery report (`powercfg /batteryreport /xml`, the same in every language), the
//! kernel's `/sys/class/power_supply` on Linux and `ioreg`'s `AppleSmartBattery` on macOS.

use crate::Result;
use rewind_os_platform::{Os, process};
use serde::Serialize;

/// Below this share of its design capacity a battery counts as worn, as laptop makers and
/// warranties usually draw the line.
pub const WORN: f32 = 0.8;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Battery {
    pub name: String,
    pub maker: Option<String>,
    pub chemistry: Option<String>,
    /// What it was made to hold, in mWh, when the system says.
    pub design_mwh: Option<u64>,
    /// What it holds now when full, in mWh, when the system says.
    pub full_mwh: Option<u64>,
    /// What it holds now against what it was made to hold (0.67 = 67 %).
    pub health: Option<f32>,
    /// Charge cycles; `None` when the battery does not count them (some report 0).
    pub cycles: Option<u32>,
}

impl Battery {
    pub fn worn(&self) -> bool {
        self.health.is_some_and(|h| h < WORN)
    }

    /// The health as a whole percent.
    pub fn percent(&self) -> Option<u8> {
        self.health
            .map(|h| (h * 100.0).round().clamp(0.0, 255.0) as u8)
    }
}

fn ratio(now: u64, design: u64) -> Option<f32> {
    // A battery may hold a little more than its design when new; more than half again is a
    // misreading.
    (design > 0 && now > 0 && now <= design + design / 2)
        .then(|| (now as f64 / design as f64) as f32)
}

/// The text of the first `<tag>…</tag>` in `xml`.
fn tag<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = xml.find(&open)? + open.len();
    let end = start + xml.get(start..)?.find(&close)?;
    xml.get(start..end).map(str::trim)
}

fn text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

/// The batteries in Windows' battery report (`powercfg /batteryreport /xml`).
pub fn parse_windows(xml: &str) -> Vec<Battery> {
    let Some(list) = tag(xml, "Batteries") else {
        return Vec::new();
    };
    list.split("<Battery>")
        .skip(1)
        .filter_map(|block| {
            let block = block.split("</Battery>").next()?;
            let number = |name: &str| tag(block, name).and_then(|v| v.parse::<u64>().ok());
            let design = number("DesignCapacity").filter(|v| *v > 0);
            let full = number("FullChargeCapacity").filter(|v| *v > 0);
            Some(Battery {
                name: text(tag(block, "Id")).unwrap_or_else(|| "Battery".to_owned()),
                maker: text(tag(block, "Manufacturer")),
                chemistry: text(tag(block, "Chemistry")),
                design_mwh: design,
                full_mwh: full,
                health: design.zip(full).and_then(|(d, f)| ratio(f, d)),
                cycles: number("CycleCount")
                    .filter(|c| *c > 0)
                    .and_then(|c| u32::try_from(c).ok()),
            })
        })
        .collect()
}

/// One battery from the kernel's `uevent` file (`POWER_SUPPLY_…=` lines). Energy is in µWh;
/// batteries that report charge (µAh) are turned into energy with their design voltage.
pub fn parse_linux(uevent: &str) -> Option<Battery> {
    let field = |key: &str| {
        uevent.lines().find_map(|l| {
            l.strip_prefix("POWER_SUPPLY_")?
                .strip_prefix(key)?
                .strip_prefix('=')
                .map(str::trim)
        })
    };
    let number = |key: &str| field(key).and_then(|v| v.parse::<u64>().ok());
    if field("TYPE") != Some("Battery") || field("SCOPE") == Some("Device") {
        return None;
    }
    // A virtual machine's battery (WSL and Hyper-V show a full 5 Wh one) says nothing about the
    // computer's.
    let virtual_one = [field("MODEL_NAME"), field("SERIAL_NUMBER")]
        .into_iter()
        .flatten()
        .any(|v| v.to_ascii_lowercase().contains("virtual"));
    if virtual_one {
        return None;
    }
    let (design, full) = match (number("ENERGY_FULL_DESIGN"), number("ENERGY_FULL")) {
        (Some(d), Some(f)) => (d, f),
        _ => (number("CHARGE_FULL_DESIGN")?, number("CHARGE_FULL")?),
    };
    let energy = number("ENERGY_FULL_DESIGN").is_some();
    let volts = number("VOLTAGE_MIN_DESIGN");
    let mwh = |raw: u64| {
        if energy {
            Some(raw / 1000)
        } else {
            // µAh × µV = 10⁻¹² Wh.
            volts.map(|v| (u128::from(raw) * u128::from(v) / 1_000_000_000) as u64)
        }
    };
    Some(Battery {
        name: text(field("MODEL_NAME"))
            .or_else(|| text(field("NAME")))
            .unwrap_or_else(|| "Battery".to_owned()),
        maker: text(field("MANUFACTURER")),
        chemistry: text(field("TECHNOLOGY")),
        design_mwh: mwh(design).filter(|v| *v > 0),
        full_mwh: mwh(full).filter(|v| *v > 0),
        health: ratio(full, design),
        cycles: number("CYCLE_COUNT")
            .filter(|c| *c > 0)
            .and_then(|c| u32::try_from(c).ok()),
    })
}

/// The integer after `<key>name</key>` in a property list.
fn plist_integer(plist: &str, name: &str) -> Option<u64> {
    let key = format!("<key>{name}</key>");
    let rest = plist.get(plist.find(&key)? + key.len()..)?.trim_start();
    tag(
        rest.get(..rest.find("</integer>")? + "</integer>".len())?,
        "integer",
    )?
    .parse()
    .ok()
}

fn plist_string(plist: &str, name: &str) -> Option<String> {
    let key = format!("<key>{name}</key>");
    let rest = plist.get(plist.find(&key)? + key.len()..)?.trim_start();
    if !rest.starts_with("<string>") {
        return None;
    }
    text(tag(rest, "string"))
}

/// The battery in `ioreg -rn AppleSmartBattery -a`. Capacities are in mAh; the voltage (mV)
/// turns them into energy.
pub fn parse_mac(plist: &str) -> Option<Battery> {
    let design = plist_integer(plist, "DesignCapacity").filter(|v| *v > 0)?;
    // Newer macOS reports `MaxCapacity` as a percentage; the raw figure is in mAh.
    let full = plist_integer(plist, "AppleRawMaxCapacity")
        .or_else(|| plist_integer(plist, "NominalChargeCapacity"))
        .or_else(|| plist_integer(plist, "MaxCapacity").filter(|v| *v > 100))?;
    let volts = plist_integer(plist, "Voltage").filter(|v| *v > 0);
    let mwh = |mah: u64| volts.map(|mv| mah * mv / 1000);
    Some(Battery {
        name: plist_string(plist, "DeviceName").unwrap_or_else(|| "Battery".to_owned()),
        maker: plist_string(plist, "Manufacturer"),
        chemistry: None,
        design_mwh: mwh(design),
        full_mwh: mwh(full),
        health: ratio(full, design),
        cycles: plist_integer(plist, "CycleCount")
            .filter(|c| *c > 0)
            .and_then(|c| u32::try_from(c).ok()),
    })
}

/// Every battery this computer has; none on a desktop.
pub fn report() -> Result<Vec<Battery>> {
    Ok(match Os::current() {
        Os::Windows => {
            let file =
                std::env::temp_dir().join(format!("rewind-os-battery-{}.xml", std::process::id()));
            let out = process::run(
                "powercfg",
                &["/batteryreport", "/xml", "/output", &file.to_string_lossy()],
            )?;
            let xml = std::fs::read_to_string(&file).unwrap_or_default();
            let _ = std::fs::remove_file(&file);
            if !out.success() && xml.is_empty() {
                return Ok(Vec::new());
            }
            parse_windows(xml.trim_start_matches('\u{feff}'))
        }
        Os::Linux => std::fs::read_dir("/sys/class/power_supply")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path().join("uevent")).ok())
            .filter_map(|u| parse_linux(&u))
            .collect(),
        Os::MacOs => process::run("ioreg", &["-rn", "AppleSmartBattery", "-a"])
            .ok()
            .filter(|o| o.success())
            .and_then(|o| parse_mac(&o.stdout))
            .into_iter()
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_report_gives_health_and_hides_a_cycle_count_of_zero() {
        // From a real ASUS laptop's report.
        let xml = "\u{feff}<?xml version=\"1.0\"?><BatteryReport><Batteries>
    <Battery>
      <Id>ASUS Battery</Id>
      <Manufacturer>ASUSTeK</Manufacturer>
      <SerialNumber> </SerialNumber>
      <Chemistry>LIon</Chemistry>
      <DesignCapacity>75998</DesignCapacity>
      <FullChargeCapacity>51140</FullChargeCapacity>
      <CycleCount>0</CycleCount>
    </Battery>
  </Batteries><RuntimeEstimates><DesignCapacity><Capacity>75998</Capacity></DesignCapacity></RuntimeEstimates></BatteryReport>";
        let batteries = parse_windows(xml.trim_start_matches('\u{feff}'));
        assert_eq!(batteries.len(), 1);
        let b = &batteries[0];
        assert_eq!(b.percent(), Some(67));
        assert!(b.worn());
        assert_eq!(b.cycles, None);
        assert_eq!(b.maker.as_deref(), Some("ASUSTeK"));
        // A desktop's report has no batteries.
        assert!(parse_windows("<BatteryReport><Batteries></Batteries></BatteryReport>").is_empty());
    }

    #[test]
    fn linux_reads_energy_or_charge_and_skips_other_power_supplies() {
        let energy = "POWER_SUPPLY_NAME=BAT0\nPOWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_TECHNOLOGY=Li-ion\nPOWER_SUPPLY_CYCLE_COUNT=312\nPOWER_SUPPLY_ENERGY_FULL_DESIGN=57000000\nPOWER_SUPPLY_ENERGY_FULL=48450000\nPOWER_SUPPLY_MODEL_NAME=5B10W13930\nPOWER_SUPPLY_MANUFACTURER=SMP\n";
        let b = parse_linux(energy).unwrap();
        assert_eq!(b.percent(), Some(85));
        assert!(!b.worn());
        assert_eq!((b.design_mwh, b.cycles), (Some(57_000), Some(312)));
        let charge = "POWER_SUPPLY_NAME=BAT1\nPOWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_VOLTAGE_MIN_DESIGN=11400000\nPOWER_SUPPLY_CHARGE_FULL_DESIGN=4000000\nPOWER_SUPPLY_CHARGE_FULL=2800000\n";
        let b = parse_linux(charge).unwrap();
        assert_eq!(b.percent(), Some(70));
        assert_eq!(b.design_mwh, Some(45_600));
        // The charger, and a mouse's battery, are not the computer's.
        assert!(parse_linux("POWER_SUPPLY_NAME=AC\nPOWER_SUPPLY_TYPE=Mains\n").is_none());
        let mouse = "POWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_SCOPE=Device\nPOWER_SUPPLY_ENERGY_FULL_DESIGN=1\nPOWER_SUPPLY_ENERGY_FULL=1\n";
        assert!(parse_linux(mouse).is_none());
        // As WSL shows it.
        let wsl = "POWER_SUPPLY_NAME=BAT1\nPOWER_SUPPLY_TYPE=Battery\nPOWER_SUPPLY_CYCLE_COUNT=-1\nPOWER_SUPPLY_ENERGY_FULL_DESIGN=5000000\nPOWER_SUPPLY_ENERGY_FULL=5000000\nPOWER_SUPPLY_MODEL_NAME=Microsoft Hyper-V Virtual Battery\nPOWER_SUPPLY_SERIAL_NUMBER=Virtual\n";
        assert!(parse_linux(wsl).is_none());
    }

    #[test]
    fn mac_reads_the_raw_capacity_not_the_percentage() {
        let plist = "<plist><array><dict>
	<key>AppleRawMaxCapacity</key>
	<integer>4382</integer>
	<key>CycleCount</key>
	<integer>287</integer>
	<key>DesignCapacity</key>
	<integer>4790</integer>
	<key>DeviceName</key>
	<string>bq40z651</string>
	<key>MaxCapacity</key>
	<integer>100</integer>
	<key>Voltage</key>
	<integer>12500</integer>
</dict></array></plist>";
        let b = parse_mac(plist).unwrap();
        assert_eq!(b.percent(), Some(91));
        assert_eq!(b.cycles, Some(287));
        assert_eq!(b.design_mwh, Some(59_875));
        assert!(parse_mac("<plist><array></array></plist>").is_none());
    }

    #[test]
    fn an_impossible_reading_gives_no_health() {
        assert_eq!(ratio(0, 50_000), None);
        assert_eq!(ratio(90_000, 50_000), None);
        assert!(ratio(52_000, 50_000).is_some());
    }
}
