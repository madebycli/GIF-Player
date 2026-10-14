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
            ["quit"] => break,
            _ => {
                eprintln!(
                    "Commands: move X Y, click, down, up, scroll DELTA, key EVDEV_CODE, quit"
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
