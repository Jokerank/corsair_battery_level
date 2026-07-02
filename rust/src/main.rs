#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hidapi::{HidApi, HidDevice, HidError};
use std::cell::RefCell;
use std::ffi::c_void;
use std::ffi::OsStr;
use std::iter::once;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, TRUE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ, RGBQUAD,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostQuitMessage,
    RegisterClassW, SetForegroundWindow, SetTimer, TrackPopupMenu, TranslateMessage, CS_HREDRAW,
    CS_VREDRAW, CW_USEDEFAULT, HICON, ICONINFO, IDI_APPLICATION, MF_STRING, MSG, TPM_BOTTOMALIGN,
    TPM_LEFTALIGN, TPM_RIGHTBUTTON, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_LBUTTONUP, WM_RBUTTONUP,
    WM_TIMER, WM_USER, WNDCLASSW, WS_OVERLAPPEDWINDOW,
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
const ICON_SIZE: usize = 32;
const ICON_PIXELS: usize = ICON_SIZE * ICON_SIZE;

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
        name: "Virtuoso RGB Wireless SE",
        protocol: Protocol::Virtuoso,
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
        let icon = create_status_icon(None)?;
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
                if let Ok(icon) = create_status_icon(Some(&status)) {
                    let _ = self.update(icon, &tooltip);
                }
            }
            Ok(None) | Err(_) => {
                if let Ok(icon) = create_status_icon(None) {
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

fn create_status_icon(status: Option<&DeviceStatus>) -> Result<HICON, String> {
    let mut canvas = Canvas::new();
    match status {
        Some(status) if status.connected => {
            if let Some(battery) = status.battery {
                draw_battery_icon(&mut canvas, battery, status.charging);
            } else {
                draw_disconnected_icon(&mut canvas);
            }
        }
        _ => draw_disconnected_icon(&mut canvas),
    }
    create_icon_from_pixels(&canvas.pixels)
}

fn draw_battery_icon(canvas: &mut Canvas, battery: u8, charging: bool) {
    if charging && battery < 100 {
        draw_mask(canvas, &CHARGING_ICON_MASK, WHITE);
    } else {
        draw_mask(canvas, &WIRELESS_ICON_MASK, WHITE);
        apply_battery_level(canvas, battery);
    }
}

fn draw_disconnected_icon(canvas: &mut Canvas) {
    draw_mask(canvas, &HEADPHONES_ICON_MASK, WHITE);
}

fn draw_mask(canvas: &mut Canvas, mask: &[&str; ICON_SIZE], color: u32) {
    for (y, row) in mask.iter().enumerate() {
        for (x, marker) in row.as_bytes().iter().enumerate() {
            if let Some(alpha) = mask_alpha(*marker) {
                canvas.blend_pixel(x as i32, y as i32, with_alpha(color, alpha));
            }
        }
    }
}

fn apply_battery_level(canvas: &mut Canvas, battery: u8) {
    let battery = battery.min(100);
    if battery == 100 {
        return;
    }

    draw_battery_cutout_top(canvas);

    let empty_rows = ((100 - battery) as f32 * 18.0 / 100.0).ceil() as i32;
    let fill_y = (8 + empty_rows).clamp(9, 26);

    for y in 9..fill_y {
        draw_empty_battery_row(canvas, y);
    }
    draw_battery_fill_boundary(canvas, fill_y, battery);
}

fn draw_battery_cutout_top(canvas: &mut Canvas) {
    canvas.set_pixel(5, 8, with_alpha(WHITE, 128));
    for x in 6..=15 {
        canvas.set_pixel(x, 8, with_alpha(WHITE, 24));
    }
    canvas.set_pixel(16, 8, with_alpha(WHITE, 64));
}

fn draw_empty_battery_row(canvas: &mut Canvas, y: i32) {
    canvas.set_pixel(5, y, with_alpha(WHITE, 64));
    for x in 6..=14 {
        canvas.set_pixel(x, y, 0);
    }
    canvas.set_pixel(15, y, with_alpha(WHITE, 24));
}

fn draw_battery_fill_boundary(canvas: &mut Canvas, y: i32, battery: u8) {
    if battery == 0 {
        canvas.set_pixel(5, y, with_alpha(WHITE, 128));
        for x in 6..=14 {
            canvas.set_pixel(x, y, with_alpha(WHITE, 64));
        }
        canvas.set_pixel(15, y, with_alpha(WHITE, 128));
        return;
    }

    for x in 5..=15 {
        canvas.set_pixel(x, y, with_alpha(WHITE, 196));
    }
}

fn mask_alpha(marker: u8) -> Option<u8> {
    match marker {
        b'1' => Some(24),
        b'2' => Some(64),
        b'3' => Some(128),
        b'4' => Some(196),
        b'#' => Some(255),
        _ => None,
    }
}

const fn with_alpha(color: u32, alpha: u8) -> u32 {
    (color & 0x00FF_FFFF) | ((alpha as u32) << 24)
}

const WHITE: u32 = argb(255, 250, 250, 250);

const WIRELESS_ICON_MASK: [&str; 32] = [
    "................................",
    "................................",
    "......222222222.................",
    "......2#######4.................",
    "......2#######4.................",
    "..13444#######44432.............",
    "..2###############3......121....",
    "..2###############4.....12431...",
    "..2###############4.....14##2...",
    "..2###############4......2##42..",
    "..2###############4..122.13##3..",
    "..2###############4..2#41.24#41.",
    "..2###############4.13##3.13##2.",
    "..2###############4..24#41.2##3.",
    "..2###############4...3##2.2##3.",
    "..2###############4...3##2.2##3.",
    "..2###############4...3##2.2##3.",
    "..2###############4...3##2.2##3.",
    "..2###############4..24#41.2##3.",
    "..2###############4.13##3.13##2.",
    "..2###############4..2#41.24#41.",
    "..2###############4..122.13##3..",
    "..2###############4......2##42..",
    "..2###############4.....14##2...",
    "..2###############4......2431...",
    "..2###############4......121....",
    "..2###############4.............",
    "..2###############4.............",
    "..2###############3.............",
    "..12222222222222221.............",
    "................................",
    "................................",
];

const CHARGING_ICON_MASK: [&str; 32] = [
    "................................",
    "................................",
    "...........1222222221...........",
    "...........1########1...........",
    "...........1########1...........",
    "........2344########4432........",
    ".......14##############41.......",
    ".......1################1.......",
    ".......1################1.......",
    ".......1################1.......",
    ".......1########44######1.......",
    ".......1########24######1.......",
    ".......1#######314######1.......",
    ".......1#######2.4######1.......",
    ".......1######31.4######1.......",
    ".......1#####42..4######1.......",
    ".......1#####31..344####1.......",
    ".......1####42.....3####1.......",
    ".......1####3.....24####1.......",
    ".......1####443..13#####1.......",
    ".......1######4..24#####1.......",
    ".......1######4.13######1.......",
    ".......1######4.2#######1.......",
    ".......1######413#######1.......",
    ".......1######42########1.......",
    ".......1######44########1.......",
    ".......1################1.......",
    ".......14##############41.......",
    ".......13##############31.......",
    "........1222222222222221........",
    "................................",
    "................................",
];

const HEADPHONES_ICON_MASK: [&str; 32] = [
    "................................",
    "...........1233333321...........",
    ".........124########421.........",
    "........24############42........",
    "..121..24###42222234###42.......",
    "..2431.24#421......124##42......",
    ".13##31.231..........13##41.....",
    "..24##31.1............14##3.....",
    "...24##31..............24#42....",
    "....3###31..............3##3....",
    "...14####31.............2##31...",
    "...14#44##31............14#41...",
    "...14#424##31...........14#41...",
    "...1##4.24##31...........4##1...",
    "...1##4..24##31..........4##1...",
    "...1##411124##31...1111114##1...",
    "...1########4##31..14#######1...",
    "...1########24##31.14#######1...",
    "...1########124##31.24######1...",
    "...1########1.24##31.24#####1...",
    "...1########1..24##31.24####1...",
    "...1########1...24##31.24###1...",
    "...1########1....24##31.24##1...",
    "...1########1.....24##31.2441...",
    "...1########1......2###31.231...",
    "...1########1......1####31.1....",
    "...1##4444431......1344##31.....",
    "...14#41111111111.....24##31....",
    "....3##########41......24#42....",
    "....14##########1.......232.....",
    ".....123444444431........11.....",
    "................................",
];

struct Canvas {
    pixels: [u32; ICON_PIXELS],
}

impl Canvas {
    fn new() -> Self {
        Self {
            pixels: [0; ICON_PIXELS],
        }
    }

    fn blend_pixel(&mut self, x: i32, y: i32, src: u32) {
        if x < 0 || y < 0 || x >= ICON_SIZE as i32 || y >= ICON_SIZE as i32 {
            return;
        }
        let index = y as usize * ICON_SIZE + x as usize;
        self.pixels[index] = blend_argb(self.pixels[index], src);
    }

    fn set_pixel(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= ICON_SIZE as i32 || y >= ICON_SIZE as i32 {
            return;
        }
        let index = y as usize * ICON_SIZE + x as usize;
        self.pixels[index] = color;
    }
}

fn blend_argb(dst: u32, src: u32) -> u32 {
    let sa = (src >> 24) & 0xFF;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }

    let da = (dst >> 24) & 0xFF;
    let out_a = sa + (da * (255 - sa) + 127) / 255;
    if out_a == 0 {
        return 0;
    }

    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;

    let blend_channel = |s: u32, d: u32| (s * sa + d * da * (255 - sa) / 255 + out_a / 2) / out_a;

    (out_a << 24)
        | (blend_channel(sr, dr) << 16)
        | (blend_channel(sg, dg) << 8)
        | blend_channel(sb, db)
}

const fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

fn create_icon_from_pixels(pixels: &[u32; ICON_PIXELS]) -> Result<HICON, String> {
    unsafe {
        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: ICON_SIZE as i32,
                biHeight: -(ICON_SIZE as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: (ICON_PIXELS * size_of::<u32>()) as u32,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [RGBQUAD {
                rgbBlue: 0,
                rgbGreen: 0,
                rgbRed: 0,
                rgbReserved: 0,
            }],
        };
        let mut bits: *mut c_void = null_mut();
        let color_bitmap = CreateDIBSection(
            null_mut(),
            &mut bitmap_info,
            DIB_RGB_COLORS,
            &mut bits,
            null_mut(),
            0,
        );
        if color_bitmap.is_null() || bits.is_null() {
            return Err(format!("CreateDIBSection failed: {}", GetLastError()));
        }

        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, ICON_PIXELS);

        let mask_bits = [0u8; ICON_SIZE * 4];
        let mask_bitmap = CreateBitmap(
            ICON_SIZE as i32,
            ICON_SIZE as i32,
            1,
            1,
            mask_bits.as_ptr() as *const c_void,
        );
        if mask_bitmap.is_null() {
            DeleteObject(color_bitmap as HGDIOBJ);
            return Err(format!("CreateBitmap mask failed: {}", GetLastError()));
        }

        let icon_info = ICONINFO {
            fIcon: TRUE,
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bitmap,
            hbmColor: color_bitmap,
        };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color_bitmap as HGDIOBJ);
        DeleteObject(mask_bitmap as HGDIOBJ);

        if icon.is_null() {
            return Err(format!("CreateIconIndirect failed: {}", GetLastError()));
        }
        Ok(icon)
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
