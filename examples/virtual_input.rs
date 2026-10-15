//! Persistent virtual devices for real headless compositor acceptance (stdin commands).
use std::{
    io::{BufRead, Write},
    os::fd::AsFd,
};
use wayland_client::{
    Connection,
    globals::registry_queue_init,
    protocol::{
        wl_pointer::{Axis, ButtonState},
        wl_registry, wl_seat,
    },
};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};
use wayland_protocols_wlr::virtual_pointer::v1::client::{
    zwlr_virtual_pointer_manager_v1::ZwlrVirtualPointerManagerV1,
    zwlr_virtual_pointer_v1::ZwlrVirtualPointerV1,
};
struct Driver;
impl wayland_client::Dispatch<wl_registry::WlRegistry, wayland_client::globals::GlobalListContents>
    for Driver
{
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &Connection,
        _: &wayland_client::QueueHandle<Self>,
    ) {
    }
}
wayland_client::delegate_noop!(Driver: ignore wl_seat::WlSeat);
wayland_client::delegate_noop!(Driver: ignore ZwlrVirtualPointerManagerV1);
wayland_client::delegate_noop!(Driver: ignore ZwlrVirtualPointerV1);
wayland_client::delegate_noop!(Driver: ignore ZwpVirtualKeyboardManagerV1);
wayland_client::delegate_noop!(Driver: ignore ZwpVirtualKeyboardV1);
fn main() -> anyhow::Result<()> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<Driver>(&conn)?;
    let qh = queue.handle();
    let mut state = Driver;
    let seat = globals.bind::<wl_seat::WlSeat, _, _>(&qh, 1..=9, ())?;
    let pointers = globals.bind::<ZwlrVirtualPointerManagerV1, _, _>(&qh, 1..=2, ())?;
    let keyboards = globals.bind::<ZwpVirtualKeyboardManagerV1, _, _>(&qh, 1..=1, ())?;
    let pointer = pointers.create_virtual_pointer(Some(&seat), &qh, ());
    let keyboard = keyboards.create_virtual_keyboard(&seat, &qh, ());
    let keymap = "xkb_keymap { xkb_keycodes { include \"evdev+aliases(qwerty)\" }; xkb_types { include \"complete\" }; xkb_compatibility { include \"complete\" }; xkb_symbols { include \"pc+us+inet(evdev)\" }; };\0";
    let mut file = tempfile::tempfile()?;
    file.write_all(keymap.as_bytes())?;
    keyboard.keymap(1, file.as_fd(), keymap.len() as u32);
    queue.roundtrip(&mut state)?;
    println!("ready");
    let clock = std::time::Instant::now();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let parts = line.split_whitespace().collect::<Vec<_>>();
        let time = clock.elapsed().as_millis() as u32;
        match parts.as_slice() {
            ["move", x, y] => pointer.motion_absolute(time, x.parse()?, y.parse()?, 2560, 720),
            ["click"] => {
                pointer.button(time, 0x110, ButtonState::Pressed);
                pointer.frame();
                pointer.button(time, 0x110, ButtonState::Released);
            }
            ["down"] => pointer.button(time, 0x110, ButtonState::Pressed),
            ["up"] => pointer.button(time, 0x110, ButtonState::Released),
            ["scroll", value] => pointer.axis(time, Axis::VerticalScroll, value.parse()?),
            ["key", code] => {
                let code = code.parse()?;
                keyboard.key(time, code, 1);
                keyboard.key(time, code, 0);
            }
            ["keydown", code] => keyboard.key(time, code.parse()?, 1),
            ["keyup", code] => keyboard.key(time, code.parse()?, 0),
            ["ctrl-a"] | ["ctrl-w"] | ["ctrl-c"] | ["ctrl-v"] | ["ctrl-f"] => {
                let code = match parts[0] {
                    "ctrl-a" => 30,
                    "ctrl-c" => 46,
                    "ctrl-v" => 47,
                    "ctrl-f" => 33,
                    _ => 17,
                };
                keyboard.modifiers(4, 0, 0, 0);
                keyboard.key(time, code, 1);
                keyboard.key(time, code, 0);
                keyboard.modifiers(0, 0, 0, 0);
            }
            ["text", ..] => {
                for ch in line.strip_prefix("text ").unwrap_or("").chars() {
                    let lower = ch.to_ascii_lowercase();
                    let code = match lower {
                        'a' => 30,
                        'b' => 48,
                        'c' => 46,
                        'd' => 32,
                        'e' => 18,
                        'f' => 33,
                        'g' => 34,
                        'h' => 35,
                        'i' => 23,
                        'j' => 36,
                        'k' => 37,
                        'l' => 38,
                        'm' => 50,
                        'n' => 49,
                        'o' => 24,
                        'p' => 25,
                        'q' => 16,
                        'r' => 19,
                        's' => 31,
                        't' => 20,
                        'u' => 22,
                        'v' => 47,
                        'w' => 17,
                        'x' => 45,
                        'y' => 21,
                        'z' => 44,
                        '1'..='9' => 2 + (lower as u32 - '1' as u32),
                        '0' => 11,
                        ' ' => 57,
                        '/' => 53,
                        '.' => 52,
                        '-' => 12,
                        '_' => 12,
                        _ => anyhow::bail!("Unsupported virtual input character: {ch}"),
                    };
                    keyboard.modifiers(u32::from(ch.is_ascii_uppercase() || ch == '_'), 0, 0, 0);
                    keyboard.key(time, code, 1);
                    keyboard.key(time, code, 0);
                }
                keyboard.modifiers(0, 0, 0, 0);
            }
            ["quit"] => break,
            _ => {
                eprintln!(
                    "Commands: move X Y, click, down, up, scroll DELTA, key|keydown|keyup EVDEV_CODE, text TEXT, ctrl-a|c|f|v|w, quit"
                );
                continue;
            }
        }
        pointer.frame();
        queue.roundtrip(&mut state)?;
        println!("done {line}");
    }
    keyboard.destroy();
    pointer.destroy();
    conn.flush()?;
    Ok(())
}
