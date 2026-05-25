# Corsair Battery Level
Displays the current state and battery level of a Corsair headset as a tray icon on the task bar.

It should work with most corsair headsets.

This fork adds support for the Corsair Virtuoso RGB Wireless XT.

<img width="374" height="104" alt="image" src="https://github.com/user-attachments/assets/03e891ee-b727-4a62-aa3f-d6e345fea453" />

## Usage 
You can either download the [latest build](https://github.com/SaifAqqad/corsair_battery_level/releases/latest/) and run it, or if you have node installed, you can clone the repo, run `npm i` then `node app.js`

## Build instructions
Run `npm i` then run the build script `npm run build`

## Rust build
This fork also includes a Rust tray implementation in `rust/`.

Build it with:
```sh
cd rust
cargo build --release
```

The release binary is written to `rust/target/release/corsair-battery-level-rs.exe`.

To test HID polling from the console, run `cargo run -- --status`.

## Dependencies:
* [node-hid](https://github.com/node-hid/node-hid)
* [systray2](https://github.com/felixhao28/node-systray)

##

This is just an attempt to rewrite a version of [mx0c/Corsair-Void-Pro-Battery-Overlay](https://github.com/mx0c/Corsair-Void-Pro-Battery-Overlay) in node, mostly as an exercise. so all credits to them.
