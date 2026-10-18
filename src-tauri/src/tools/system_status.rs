//! "How much battery do I have?", "Am I online?", "Is my computer full?": read-only facts.

use serde::Serialize;
use windows::Win32::Networking::NetworkListManager::{
    INetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET, NLM_CONNECTIVITY_IPV6_INTERNET,
    NetworkListManager,
};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::core::HSTRING;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComputerStatus {
    pub has_battery: bool,
    pub battery_percent: Option<u8>,
    pub plugged_in: Option<bool>,
    pub internet: Option<bool>,
    pub disk_free_gb: Option<f64>,
    pub disk_total_gb: Option<f64>,
}

/// Turns the raw power report into plain facts (255 / flag 128 mean unknown / no battery).
pub fn battery_facts(ac_line: u8, flag: u8, percent: u8) -> (bool, Option<u8>, Option<bool>) {
    let has_battery = flag != 128 && flag != 255;
    let percent = (has_battery && percent <= 100).then_some(percent);
    let plugged = match ac_line {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    (has_battery, percent, plugged)
}

fn round1(bytes: u64) -> f64 {
    (bytes as f64 / 1_073_741_824.0 * 10.0).round() / 10.0
}

pub fn read() -> ComputerStatus {
    let mut p = SYSTEM_POWER_STATUS::default();
    let (has_battery, battery_percent, plugged_in) =
        if unsafe { GetSystemPowerStatus(&mut p) }.is_ok() {
            battery_facts(p.ACLineStatus, p.BatteryFlag, p.BatteryLifePercent)
        } else {
            (false, None, None)
        };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let internet = unsafe {
        CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
    }
    .and_then(|m| unsafe { m.GetConnectivity() })
    .ok()
    .map(|c| c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0) != 0);

    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let (mut free, mut total) = (0u64, 0u64);
    let disk = unsafe {
        GetDiskFreeSpaceExW(
            &HSTRING::from(format!("{system_drive}\\")),
            Some(&mut free),
            Some(&mut total),
            None,
        )
    }
    .is_ok();

    ComputerStatus {
        has_battery,
        battery_percent,
        plugged_in,
        internet,
        disk_free_gb: disk.then(|| round1(free)),
        disk_total_gb: disk.then(|| round1(total)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_report_is_interpreted_plainly() {
        assert_eq!(battery_facts(1, 8, 64), (true, Some(64), Some(true)));
        assert_eq!(battery_facts(0, 2, 12), (true, Some(12), Some(false)));
        assert_eq!(
            battery_facts(1, 128, 255),
            (false, None, Some(true)),
            "desktop PC"
        );
        assert_eq!(battery_facts(255, 255, 255), (false, None, None), "unknown");
    }

    #[test]
    fn live_status_reads() {
        let s = read();
        println!("{s:?}");
        assert!(s.disk_total_gb.is_some_and(|t| t > 1.0));
    }
}
