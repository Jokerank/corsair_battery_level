#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hidapi::{HidApi, HidDevice, HidError};
use std::cell::RefCell;
use std::ffi::OsStr;
use std::iter::once;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW,
    PostQuitMessage, RegisterClassW, SetForegroundWindow, SetTimer, TrackPopupMenu,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, HICON, IDI_APPLICATION, IMAGE_ICON,
    MF_STRING, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_COMMAND, WM_CREATE,
    WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP, WM_TIMER, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const APP_NAME: &str = "Corsair battery level";
const CORSAIR_VID: u16 = 0x1B1C;
const VIRTUOSO_XT_PID: u16 = 0x0A64;
const VIRTUOSO_USAGE_PAGE: u16 = 0xFF42;
const VIRTUOSO_USAGE: u16 = 0x01;
const VIRTUOSO_WIRELESS_BIT: u8 = 0x01;
const VIRTUOSO_DATA_LENGTH: usize = 64;
const VIRTUOSO_PLUGGED_COMMAND: u8 = 0x10;
const VIRTUOSO_LEVEL_COMMAND: u8 = 0x0F;
const LEGACY_DATA_REQ: [u8; 2] = [0xC9, 0x64];
const VOID_BATTERY_MIXUP: u8 = 128;
const BATTERY_POLL_MS: u32 = 60_000;
const READ_TIMEOUT_MS: i32 = 700;
const TIMER_POLL: usize = 1;
const MENU_REFRESH: usize = 1001;
const MENU_EXIT: usize = 1002;
const TRAY_UID: u32 = 1;
const WM_TRAYICON: u32 = WM_USER + 1;

static ICON_DEFAULT: &[u8] = include_bytes!("../../icons/headphones.ico");
static ICON_CHARGING: &[u8] = include_bytes!("../../icons/battery-charging.ico");
static ICON_100: &[u8] = include_bytes!("../../icons/battery-wireless.ico");
static ICON_90: &[u8] = include_bytes!("../../icons/battery-wireless-90.ico");
static ICON_80: &[u8] = include_bytes!("../../icons/battery-wireless-80.ico");
static ICON_70: &[u8] = include_bytes!("../../icons/battery-wireless-70.ico");
static ICON_60: &[u8] = include_bytes!("../../icons/battery-wireless-60.ico");
static ICON_50: &[u8] = include_bytes!("../../icons/battery-wireless-50.ico");
static ICON_40: &[u8] = include_bytes!("../../icons/battery-wireless-40.ico");
static ICON_30: &[u8] = include_bytes!("../../icons/battery-wireless-30.ico");
static ICON_20: &[u8] = include_bytes!("../../icons/battery-wireless-20.ico");
static ICON_10: &[u8] = include_bytes!("../../icons/battery-wireless-10.ico");
static ICON_0: &[u8] = include_bytes!("../../icons/battery-wireless-0.ico");

#[derive(Clone, Copy)]
enum Protocol {
    Legacy,
    Virtuoso,
}

#[derive(Clone, Copy)]
struct DeviceProfile {
    pid: u16,
    name: &'static str,
    protocol: Protocol,
}

#[derive(Clone)]
struct ConnectedDevice {
    name: String,
}

#[derive(Clone, Default)]
struct DeviceStatus {
    connected: bool,
    battery: Option<u8>,
    charging: bool,
}

struct TrayApp {
    hwnd: HWND,
    current_icon: HICON,
}

impl Drop for TrayApp {
    fn drop(&mut self) {
        unsafe {
            let mut nid = notify_data(self.hwnd);
            Shell_NotifyIconW(NIM_DELETE, &mut nid);
            if !self.current_icon.is_null() {
                DestroyIcon(self.current_icon);
            }
        }
    }
}

const PROFILES: &[DeviceProfile] = &[
    DeviceProfile {
        pid: 0x0A38,
        name: "HS70 Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A4F,
        name: "HS70 PRO Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x1B27,
        name: "VOID Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A2B,
        name: "VOID Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A14,
        name: "VOID PRO Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A16,
        name: "VOID PRO Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A1A,
        name: "VOID PRO Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A55,
        name: "VOID ELITE Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A51,
        name: "VOID ELITE Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A3E,
        name: "Virtuoso RGB Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A40,
        name: "Virtuoso RGB Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A42,
        name: "Virtuoso RGB Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A44,
        name: "Virtuoso RGB Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: 0x0A5C,
        name: "Virtuoso RGB Wireless",
        protocol: Protocol::Legacy,
    },
    DeviceProfile {
        pid: VIRTUOSO_XT_PID,
        name: "Virtuoso RGB Wireless XT",
        protocol: Protocol::Virtuoso,
    },
];

thread_local! {
    static APP: RefCell<Option<TrayApp>> = const { RefCell::new(None) };
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
    }
}

fn run() -> Result<(), String> {
    if std::env::args().any(|arg| arg == "--status") {
        return print_status();
    }

    unsafe {
        let instance = GetModuleHandleW(null());
        let class_name = wide("CorsairBatteryLevelRustWindow");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            hIcon: LoadIconW(null_mut(), IDI_APPLICATION),
            lpszClassName: class_name.as_ptr(),
            ..zeroed()
        };

        if RegisterClassW(&window_class) == 0 {
            return Err(format!("RegisterClassW failed: {}", GetLastError()));
        }

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            wide(APP_NAME).as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        );

        if hwnd.is_null() {
            return Err(format!("CreateWindowExW failed: {}", GetLastError()));
        }

        let mut message: MSG = zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }

    Ok(())
}

fn print_status() -> Result<(), String> {
    match poll_status().map_err(|err| err.to_string())? {
        Some((device, status)) => {
            println!("{}", format_tooltip(&device, &status));
        }
        None => println!("No device found"),
    }
    Ok(())
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_CREATE => {
                match TrayApp::new(hwnd) {
                    Ok(mut app) => {
                        app.refresh();
                        SetTimer(hwnd, TIMER_POLL, BATTERY_POLL_MS, None);
                        APP.with(|state| {
                            *state.borrow_mut() = Some(app);
                        });
                    }
                    Err(err) => {
                        eprintln!("{err}");
                        PostQuitMessage(1);
                    }
                }
                0
            }
            WM_TIMER if wparam == TIMER_POLL => {
                with_app(|app| app.refresh());
                0
            }
            WM_COMMAND => {
                match loword(wparam as u32) as usize {
                    MENU_REFRESH => with_app(|app| app.refresh()),
                    MENU_EXIT => PostQuitMessage(0),
                    _ => {}
                }
                0
            }
            WM_TRAYICON => {
                let mouse_message = lparam as u32;
                if mouse_message == WM_RBUTTONUP || mouse_message == WM_LBUTTONUP {
                    show_menu(hwnd);
                }
                0
            }
            WM_DESTROY => {
                APP.with(|state| {
                    *state.borrow_mut() = None;
                });
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn with_app(action: impl FnOnce(&mut TrayApp)) {
    APP.with(|state| {
        if let Some(app) = state.borrow_mut().as_mut() {
            action(app);
        }
    });
}

impl TrayApp {
    fn new(hwnd: HWND) -> Result<Self, String> {
        let icon = load_icon(ICON_DEFAULT)?;
        let mut app = Self {
            hwnd,
            current_icon: null_mut(),
        };
        app.update(icon, "No device found")?;
        Ok(app)
    }

    fn refresh(&mut self) {
        match poll_status() {
            Ok(Some((device, status))) => {
                let tooltip = format_tooltip(&device, &status);
                let icon = status_icon(&status);
                if let Ok(icon) = load_icon(icon) {
                    let _ = self.update(icon, &tooltip);
                }
            }
            Ok(None) | Err(_) => {
                if let Ok(icon) = load_icon(ICON_DEFAULT) {
                    let _ = self.update(icon, "No device found");
                }
            }
        }
    }

    fn update(&mut self, icon: HICON, tooltip: &str) -> Result<(), String> {
        unsafe {
            let mut nid = notify_data(self.hwnd);
            nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            nid.uCallbackMessage = WM_TRAYICON;
            nid.hIcon = icon;
            copy_wide_fixed(&mut nid.szTip, tooltip);

            let action = if self.current_icon.is_null() {
                NIM_ADD
            } else {
                NIM_MODIFY
            };
            if Shell_NotifyIconW(action, &mut nid) != TRUE {
                DestroyIcon(icon);
                return Err(format!("Shell_NotifyIconW failed: {}", GetLastError()));
            }

            if !self.current_icon.is_null() && self.current_icon != icon {
                DestroyIcon(self.current_icon);
            }
            self.current_icon = icon;
        }
        Ok(())
    }
}

fn poll_status() -> Result<Option<(ConnectedDevice, DeviceStatus)>, HidError> {
    let api = HidApi::new()?;
    for info in api.device_list() {
        if info.vendor_id() != CORSAIR_VID {
            continue;
        }
        let Some(profile) = PROFILES
            .iter()
            .find(|profile| profile.pid == info.product_id())
            .copied()
        else {
            continue;
        };
        if matches!(profile.protocol, Protocol::Virtuoso)
            && (info.usage_page() != VIRTUOSO_USAGE_PAGE || info.usage() != VIRTUOSO_USAGE)
        {
            continue;
        }

        let Ok(hid) = info.open_device(&api) else {
            continue;
        };
        let manufacturer = info.manufacturer_string().unwrap_or("Corsair");
        let device = ConnectedDevice {
            name: format!("{manufacturer} {}", profile.name),
        };
        let status = match profile.protocol {
            Protocol::Legacy => query_legacy(&hid)?,
            Protocol::Virtuoso => query_virtuoso(&hid)?,
        };
        return Ok(Some((device, status)));
    }

    Ok(None)
}

fn query_legacy(hid: &HidDevice) -> Result<DeviceStatus, HidError> {
    hid.write(&LEGACY_DATA_REQ)?;
    let mut buf = [0u8; VIRTUOSO_DATA_LENGTH];
    let read = hid.read_timeout(&mut buf, READ_TIMEOUT_MS)?;
    if read < 5 || buf[4] == 0 {
        return Ok(DeviceStatus {
            connected: false,
            ..DeviceStatus::default()
        });
    }

    let mut battery = buf[2];
    if battery > VOID_BATTERY_MIXUP {
        battery -= VOID_BATTERY_MIXUP;
    }

    Ok(DeviceStatus {
        connected: true,
        battery: Some(battery.min(100)),
        charging: buf[4] == 5,
    })
}

fn query_virtuoso(hid: &HidDevice) -> Result<DeviceStatus, HidError> {
    let mut status = DeviceStatus {
        connected: true,
        ..DeviceStatus::default()
    };

    hid.write(&virtuoso_request(VIRTUOSO_PLUGGED_COMMAND))?;
    if let Some(reply) = read_virtuoso_reply(hid)? {
        merge_virtuoso_reply(&mut status, &reply);
    }

    hid.write(&virtuoso_request(VIRTUOSO_LEVEL_COMMAND))?;
    if let Some(reply) = read_virtuoso_reply(hid)? {
        merge_virtuoso_reply(&mut status, &reply);
    }

    Ok(status)
}

fn read_virtuoso_reply(hid: &HidDevice) -> Result<Option<[u8; VIRTUOSO_DATA_LENGTH]>, HidError> {
    let mut buf = [0u8; VIRTUOSO_DATA_LENGTH];
    let read = hid.read_timeout(&mut buf, READ_TIMEOUT_MS)?;
    if read == 0 {
        return Ok(None);
    }
    Ok(Some(buf))
}

fn merge_virtuoso_reply(status: &mut DeviceStatus, data: &[u8; VIRTUOSO_DATA_LENGTH]) {
    if data[0] == 0x01 && data[1] == VIRTUOSO_WIRELESS_BIT && data[2] == 0x02 && data[3] == 0x00 {
        parse_virtuoso_result(status, u16::from(data[4]) | (u16::from(data[5]) << 8));
    } else if data[0] == 0x03
        && data[1] == VIRTUOSO_WIRELESS_BIT
        && data[2] == 0x01
        && data[3] == VIRTUOSO_LEVEL_COMMAND
        && data[4] == 0x00
    {
        let raw = u16::from(data[5]) | (u16::from(data[6]) << 8);
        status.battery = Some(((raw + 5) / 10).min(100) as u8);
    } else if data[0] == 0x03
        && data[1] == VIRTUOSO_WIRELESS_BIT
        && data[2] == 0x01
        && data[3] == VIRTUOSO_PLUGGED_COMMAND
        && data[4] == 0x00
    {
        parse_virtuoso_result(status, u16::from(data[5]));
    }
}

fn parse_virtuoso_result(status: &mut DeviceStatus, result: u16) {
    match result {
        1 => status.charging = true,
        2 => status.charging = false,
        value => status.battery = Some(((value + 5) / 10).min(100) as u8),
    }
}

fn virtuoso_request(command: u8) -> [u8; VIRTUOSO_DATA_LENGTH] {
    let mut request = [0u8; VIRTUOSO_DATA_LENGTH];
    request[0] = 0x02;
    request[1] = 0x08 | VIRTUOSO_WIRELESS_BIT;
    request[2] = 0x02;
    request[3] = command;
    request
}

fn format_tooltip(device: &ConnectedDevice, status: &DeviceStatus) -> String {
    if !status.connected {
        return format!("{}: Disconnected", device.name);
    }
    let state = if status.charging {
        "Charging"
    } else {
        "Connected"
    };
    match status.battery {
        Some(battery) if status.charging && battery >= 100 => {
            format!("{}: Fully charged ({}%)", device.name, battery)
        }
        Some(battery) => format!("{}: {} ({}%)", device.name, state, battery),
        None => format!("{}: {}", device.name, state),
    }
}

fn status_icon(status: &DeviceStatus) -> &'static [u8] {
    if !status.connected || status.battery.is_none() {
        return ICON_DEFAULT;
    }
    if status.charging && status.battery.unwrap_or(0) < 100 {
        return ICON_CHARGING;
    }

    match status.battery.unwrap_or(0) / 10 {
        10 => ICON_100,
        9 => ICON_90,
        8 => ICON_80,
        7 => ICON_70,
        6 => ICON_60,
        5 => ICON_50,
        4 => ICON_40,
        3 => ICON_30,
        2 => ICON_20,
        1 => ICON_10,
        _ => ICON_0,
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }

    let refresh = wide("Refresh device");
    let exit = wide("Exit");
    AppendMenuW(menu, MF_STRING, MENU_REFRESH, refresh.as_ptr());
    AppendMenuW(menu, MF_STRING, MENU_EXIT, exit.as_ptr());

    let mut cursor = POINT { x: 0, y: 0 };
    GetCursorPos(&mut cursor);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
        cursor.x,
        cursor.y,
        0,
        hwnd,
        null(),
    );
    DestroyMenu(menu);
}

fn load_icon(ico: &[u8]) -> Result<HICON, String> {
    let Some(image) = choose_ico_image(ico) else {
        return Err("Invalid icon data".to_string());
    };

    unsafe {
        let icon = CreateIconFromResourceEx(
            image.as_ptr(),
            image.len() as u32,
            TRUE,
            0x0003_0000,
            0,
            0,
            IMAGE_ICON,
        );
        if icon.is_null() {
            return Err(format!(
                "CreateIconFromResourceEx failed: {}",
                GetLastError()
            ));
        }
        Ok(icon)
    }
}

fn choose_ico_image(ico: &[u8]) -> Option<&[u8]> {
    if ico.len() < 6 || u16_le(ico, 2)? != 1 {
        return None;
    }
    let count = u16_le(ico, 4)? as usize;
    let mut best: Option<(usize, usize)> = None;
    for index in 0..count {
        let entry = 6 + index * 16;
        if entry + 16 > ico.len() {
            return None;
        }
        let width = if ico[entry] == 0 {
            256
        } else {
            ico[entry] as usize
        };
        let height = if ico[entry + 1] == 0 {
            256
        } else {
            ico[entry + 1] as usize
        };
        let size = u32_le(ico, entry + 8)? as usize;
        let offset = u32_le(ico, entry + 12)? as usize;
        if offset.checked_add(size)? > ico.len() {
            return None;
        }
        let score = width.abs_diff(32) + height.abs_diff(32);
        if best.map_or(true, |(_, best_score)| score < best_score) {
            best = Some((index, score));
        }
    }

    let index = best?.0;
    let entry = 6 + index * 16;
    let size = u32_le(ico, entry + 8)? as usize;
    let offset = u32_le(ico, entry + 12)? as usize;
    Some(&ico[offset..offset + size])
}

fn u16_le(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn u32_le(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

unsafe fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid
}

fn copy_wide_fixed<const N: usize>(target: &mut [u16; N], text: &str) {
    target.fill(0);
    for (slot, ch) in target.iter_mut().take(N.saturating_sub(1)).zip(wide(text)) {
        *slot = ch;
    }
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(once(0)).collect()
}

fn loword(value: u32) -> u16 {
    (value & 0xFFFF) as u16
}
