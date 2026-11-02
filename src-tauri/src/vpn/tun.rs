//! Готовность адаптера TUN: чтение таблицы Windows, без запуска PowerShell.
use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2};
use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

pub const NAME: &str = "ZapretGUI";

pub fn ready() -> bool {
    unsafe {
        let mut table = std::ptr::null_mut();
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return false;
        }
        let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
        let up = rows.iter().any(|row| {
            let len = row.Alias.iter().position(|&c| c == 0).unwrap_or(row.Alias.len());
            row.OperStatus == IfOperStatusUp && String::from_utf16_lossy(&row.Alias[..len]) == NAME
        });
        FreeMibTable(table.cast());
        up
    }
}
