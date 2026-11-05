// Read-only DDC probe: lists displays, reads VCP 0x60 (input source) and capabilities.
// `ddc-probe set <value>` writes 0x60 (only run deliberately).
use ddc_hi::{Ddc, Display};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    for mut d in Display::enumerate() {
        println!("display: {:?} model={:?} serial={:?}", d.info.id, d.info.model_name, d.info.serial_number);
        match d.handle.get_vcp_feature(0x60) {
            Ok(v) => println!("  0x60 input: current=0x{:02x} max=0x{:02x} raw={:?}", v.value(), v.maximum(), v),
            Err(e) => println!("  0x60 read failed: {e}"),
        }
        match d.handle.capabilities_string() {
            Ok(c) => println!("  caps: {}", String::from_utf8_lossy(&c)),
            Err(e) => println!("  caps failed: {e}"),
        }
        if args.get(1).map(|s| s.as_str()) == Some("set") {
            let v = u16::from_str_radix(args[2].trim_start_matches("0x"), 16).unwrap();
            println!("  set 0x60 = 0x{v:02x}: {:?}", d.handle.set_vcp_feature(0x60, v));
        }
    }
}
