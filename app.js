const HID = require('node-hid'),
    SysTray = require('systray2').default,
    path = require('path'),
    LEGACY_DATA_REQ = [0xC9, 0x64],
    BATTERY_POLL_MS = 60 * 1000,
    CORSAIR_VID = 0x1B1C,
    PROTOCOLS = {
        legacy: "legacy",
        virtuoso: "virtuoso"
    },
    VIRTUOSO_DATA_LENGTH = 64,
    VIRTUOSO_USAGE_PAGE = 0xFF42,
    VIRTUOSO_USAGE = 0x01,
    VIRTUOSO_WIRELESS_BIT = 0x01,
    VIRTUOSO_BATTERY_COMMANDS = {
        plugged: 0x10,
        level: 0x0F
    },
    KNOWN_PIDS = {
        0x0A38: { name: "HS70 Wireless", protocol: PROTOCOLS.legacy },
        0x0A4F: { name: "HS70 PRO Wireless", protocol: PROTOCOLS.legacy },
        0x1B27: { name: "VOID Wireless", protocol: PROTOCOLS.legacy },
        0x0A2B: { name: "VOID Wireless", protocol: PROTOCOLS.legacy },
        0x0A14: { name: "VOID PRO Wireless", protocol: PROTOCOLS.legacy },
        0x0A16: { name: "VOID PRO Wireless", protocol: PROTOCOLS.legacy },
        0x0A1A: { name: "VOID PRO Wireless", protocol: PROTOCOLS.legacy },
        0x0A55: { name: "VOID ELITE Wireless", protocol: PROTOCOLS.legacy },
        0x0A51: { name: "VOID ELITE Wireless", protocol: PROTOCOLS.legacy },
        0x0A3E: { 
            name: "Virtuoso RGB Wireless SE", 
            protocol: PROTOCOLS.virtuoso,
            usagePage: VIRTUOSO_USAGE_PAGE,
            usage: VIRTUOSO_USAGE,
            wirelessBit: VIRTUOSO_WIRELESS_BIT 
        },
        0x0A40: { name: "Virtuoso RGB Wireless", protocol: PROTOCOLS.legacy },
        0x0A42: { name: "Virtuoso RGB Wireless", protocol: PROTOCOLS.legacy },
        0x0A44: { name: "Virtuoso RGB Wireless", protocol: PROTOCOLS.legacy },
        0x0A5C: { name: "Virtuoso RGB Wireless", protocol: PROTOCOLS.legacy },
        0x0A64: {
            name: "Virtuoso RGB Wireless XT",
            protocol: PROTOCOLS.virtuoso,
            usagePage: VIRTUOSO_USAGE_PAGE,
            usage: VIRTUOSO_USAGE,
            wirelessBit: VIRTUOSO_WIRELESS_BIT
        }
    },
    DEVICE_STATES = {
        0: "Disconnected",
        1: "Connected",
        2: "Low battery",
        4: "Fully charged",
        5: "Charging"
    },
    TRAY_ICONS = {
        default: path.join(__dirname, "icons/headphones.ico"),
        charging: path.join(__dirname, "icons/battery-charging.ico"),
        10: path.join(__dirname, "icons/battery-wireless.ico"),
        9: path.join(__dirname, "icons/battery-wireless-90.ico"),
        8: path.join(__dirname, "icons/battery-wireless-80.ico"),
        7: path.join(__dirname, "icons/battery-wireless-70.ico"),
        6: path.join(__dirname, "icons/battery-wireless-60.ico"),
        5: path.join(__dirname, "icons/battery-wireless-50.ico"),
        4: path.join(__dirname, "icons/battery-wireless-40.ico"),
        3: path.join(__dirname, "icons/battery-wireless-30.ico"),
        2: path.join(__dirname, "icons/battery-wireless-20.ico"),
        1: path.join(__dirname, "icons/battery-wireless-10.ico"),
        0: path.join(__dirname, "icons/battery-wireless-0.ico"),
    },
    MENU_ITEMS = [
        {
            title: "Refresh device",
            tooltip: "Refresh device",
            checked: false,
            enabled: true,
            click: init_device
        },
        {
            title: "Exit",
            tooltip: "Exit",
            checked: false,
            enabled: true,
            click: () => {
                tray.kill(false);
                process.exit(0);
            }
        }
    ],
    TRAY_OPTIONS = {
        menu: {
            icon: TRAY_ICONS["default"],
            title: "Corsair battery level",
            tooltip: "No device found",
            items: MENU_ITEMS
        },
        debug: false,
        copyDir: true
    },
    VOID_BATTERY_MICUP = 128,
    tray = new SysTray(TRAY_OPTIONS);
let device_info = null,
    device_hid = null,
    battery_poll = null,
    device_generation = 0,
    device_status = get_default_status();
process.on('exit', () => { tray.kill(false); });
tray.ready().then(() => {
    tray._rl.on('line', handle_tray_line);
    init_device();
});

function handle_tray_line(line) {
    let event;
    try {
        event = JSON.parse(line);
    } catch {
        return;
    }
    if (event.type !== 'clicked' || !event.item) {
        return;
    }
    const menuItem = MENU_ITEMS.find(item => item.title === event.item.title);
    if (menuItem && menuItem.click) {
        menuItem.click();
    }
}

function init_device() {
    close_device();
    [device_hid, device_info] = get_HID();
    if (!device_hid) {
        reset_tray();
        return;
    }
    const generation = device_generation;
    device_info.full_name = `${device_info.manufacturer} ${device_info.profile.name}`;
    device_hid.setNonBlocking(1);
    device_hid.on('data', data => update_tray(data, generation));
    device_hid.on('error', () => handle_device_error(generation));
    device_hid.resume();
    update_tray_status(device_status);
    if (device_info.profile.protocol === PROTOCOLS.virtuoso) {
        query_virtuoso_battery(generation);
        battery_poll = setInterval(() => query_virtuoso_battery(generation), BATTERY_POLL_MS);
    }
}

function close_device() {
    device_generation++;
    if (battery_poll) {
        clearInterval(battery_poll);
        battery_poll = null;
    }
    if (device_hid) {
        device_hid.removeAllListeners('data');
        device_hid.removeAllListeners('error');
        try {
            device_hid.close();
        } catch {}
    }
    device_info = device_hid = null;
    device_status = get_default_status();
}

function get_default_status() {
    return {
        connected: true,
        battery: null,
        charging: false,
        state: 1
    };
}

function handle_device_error(generation) {
    if (generation !== device_generation) {
        return;
    }
    close_device();
    reset_tray();
}

function get_HID() {
    let dList = HID.devices(), hidDevice, infoObj;
    for (let deviceObj of dList) {
        const profile = KNOWN_PIDS[deviceObj.productId];
        if (deviceObj.vendorId !== CORSAIR_VID || profile === undefined)
            continue;
        if (profile.protocol === PROTOCOLS.virtuoso && !is_matching_virtuoso_interface(deviceObj, profile))
            continue;
        try {
            hidDevice = new HID.HID(deviceObj.path);
            if (profile.protocol === PROTOCOLS.legacy) {
                hidDevice.write(LEGACY_DATA_REQ);
            }
            hidDevice.pause();
            infoObj = {
                ...deviceObj,
                profile
            };
            break;
        } catch {
            hidDevice = infoObj = null;
            continue;
        }
    }
    return [hidDevice, infoObj];
}

function is_matching_virtuoso_interface(deviceObj, profile) {
    return deviceObj.usagePage === profile.usagePage && deviceObj.usage === profile.usage;
}

function update_tray(data, generation) {
    if (generation !== device_generation || !device_info) {
        return;
    }
    const parsedStatus = device_info.profile.protocol === PROTOCOLS.virtuoso
        ? parse_virtuoso_data(data)
        : parse_legacy_data(data);
    if (!parsedStatus) {
        return;
    }
    device_status = {
        ...device_status,
        ...parsedStatus
    };
    update_tray_status(device_status);
}

function parse_legacy_data([, , battery, , state]) {
    if (battery > VOID_BATTERY_MICUP) {
        battery = battery - VOID_BATTERY_MICUP;
    }
    if (state === 0 || DEVICE_STATES[state] === undefined) {
        return {
            connected: false,
            state: 0
        };
    }
    return {
        connected: true,
        battery: clamp_battery(battery),
        charging: state === 5,
        state
    };
}

function parse_virtuoso_data(data) {
    const wirelessBit = device_info.profile.wirelessBit;
    if (is_virtuoso_battery_reply(data, wirelessBit)) {
        return parse_virtuoso_result(data[4] | (data[5] << 8));
    }
    if (is_virtuoso_unsolicited_battery(data, wirelessBit)) {
        return {
            connected: true,
            battery: clamp_battery(Math.round((data[5] | (data[6] << 8)) / 10))
        };
    }
    if (is_virtuoso_unsolicited_plugged(data, wirelessBit)) {
        return parse_virtuoso_result(data[5]);
    }
    return null;
}

function is_virtuoso_battery_reply(data, wirelessBit) {
    return data[0] === 0x01 && data[1] === wirelessBit && data[2] === 0x02 && data[3] === 0x00;
}

function is_virtuoso_unsolicited_battery(data, wirelessBit) {
    return data[0] === 0x03 && data[1] === wirelessBit && data[2] === 0x01
        && data[3] === VIRTUOSO_BATTERY_COMMANDS.level && data[4] === 0x00;
}

function is_virtuoso_unsolicited_plugged(data, wirelessBit) {
    return data[0] === 0x03 && data[1] === wirelessBit && data[2] === 0x01
        && data[3] === VIRTUOSO_BATTERY_COMMANDS.plugged && data[4] === 0x00;
}

function parse_virtuoso_result(result) {
    if (result === 1 || result === 2) {
        return {
            connected: true,
            charging: result === 1,
            state: result === 1 ? 5 : 1
        };
    }
    return {
        connected: true,
        battery: clamp_battery(Math.round(result / 10))
    };
}

function query_virtuoso_battery(generation) {
    if (generation !== device_generation || !device_hid || !device_info || device_info.profile.protocol !== PROTOCOLS.virtuoso) {
        return;
    }
    try {
        device_hid.write(build_virtuoso_request(VIRTUOSO_BATTERY_COMMANDS.plugged, device_info.profile));
        device_hid.write(build_virtuoso_request(VIRTUOSO_BATTERY_COMMANDS.level, device_info.profile));
    } catch {
        handle_device_error(generation);
    }
}

function build_virtuoso_request(command, profile) {
    return pad_virtuoso_report(0x02, 0x08 | profile.wirelessBit, 0x02, command);
}

function pad_virtuoso_report(...bytes) {
    return bytes.concat(Array(VIRTUOSO_DATA_LENGTH - bytes.length).fill(0));
}

function update_tray_status(status) {
    let icon, tooltip;
    if (!status.connected) {
        icon = TRAY_ICONS["default"];
        tooltip = `${device_info.full_name}: ${DEVICE_STATES[0]}`;
    } else if (status.charging) {
        icon = status.battery >= 100 ? TRAY_ICONS[10] : TRAY_ICONS["charging"];
        tooltip = format_tooltip(status, status.battery >= 100 ? 4 : 5);
    } else if (status.battery === null) {
        icon = TRAY_ICONS["default"];
        tooltip = `${device_info.full_name}: ${DEVICE_STATES[1]}`;
    } else {
        icon = TRAY_ICONS[Math.floor(status.battery / 10)];
        tooltip = format_tooltip(status, status.state);
    }
    tray.sendAction({
        type: 'update-menu',
        menu: {
            icon,
            tooltip,
            title: tooltip,
            items: MENU_ITEMS
        }
    });
}

function format_tooltip(status, state) {
    const stateText = DEVICE_STATES[state] || DEVICE_STATES[1];
    if (status.battery === null) {
        return `${device_info.full_name}: ${stateText}`;
    }
    return `${device_info.full_name}: ${stateText} (${status.battery}%)`;
}

function clamp_battery(battery) {
    return Math.max(0, Math.min(100, battery));
}

function reset_tray() {
    tray.sendAction({
        type: 'update-menu',
        menu: {
            icon: TRAY_ICONS["default"],
            title: "Corsair battery level",
            tooltip: "No device found",
            items: MENU_ITEMS
        }
    });
}
