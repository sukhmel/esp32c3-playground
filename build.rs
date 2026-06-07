use embedded_graphics::pixelcolor::raw::RawU24;
use embedded_graphics::pixelcolor::{Rgb565, Rgb666, raw::RawU16};
use embedded_graphics::prelude::RawData;
use std::env;
use std::f64::consts::PI;
use std::fs;
use std::path::Path;
use toml::Value;

fn main() {
    // Rerun if secrets.toml changes
    println!("cargo:rerun-if-changed=secrets.toml");

    let secrets_path = Path::new("secrets.toml");
    let content = fs::read_to_string(secrets_path).expect("Failed to read secrets.toml");
    let config: Value = toml::from_str(&content).expect("Failed to parse secrets.toml");

    // Handle SSID, trying to read as string, then as integer if necessary
    let ssid = if let Some(s) = config["wifi"]["ssid"].as_str() {
        s.to_string()
    } else if let Some(i) = config["wifi"]["ssid"].as_integer() {
        i.to_string()
    } else {
        panic!("SSID not found or invalid format");
    };

    let password = config["wifi"]["password"]
        .as_str()
        .expect("Password not found");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("secrets.rs");

    let rust_code = format!(
        "pub const WIFI_SSID: &str = \"{}\";\npub const WIFI_PASSWORD: &str = \"{}\";",
        ssid, password
    );
    fs::write(dest_path, rust_code).unwrap();

    generate_rainbow_arrays(&out_dir);

    println!("cargo:rustc-env=CONFIG_WIFI_NETWORK={}", ssid);
    println!("cargo:rustc-env=CONFIG_WIFI_PASSWORD={}", password);
}

fn generate_rainbow_arrays(out_dir: &str) {
    let dest_path = Path::new(out_dir).join("rainbows.rs");
    let mut all_content = String::from(
        "use core::f64::consts::PI;\nuse embedded_graphics::pixelcolor::{Rgb565, Rgb666};\n\n",
    );

    for &length in &[128, 256] {
        let mut content =
            format!("#[allow(dead_code)]\npub static RAINBOW_RGB565_{length}: [u16; {length}] = [");

        for step in 0..length {
            let r = (f64::sin(2.0 * step as f64 * PI / length as f64) * 15.0 + 16.0) as u8;
            let g =
                (f64::sin(2.0 * step as f64 * PI / length as f64 + PI / 3.0) * 31.0 + 32.0) as u8;
            let b = (f64::sin(2.0 * step as f64 * PI / length as f64 + PI * 2.0 / 3.0) * 15.0
                + 16.0) as u8;
            let rgb565 = RawU16::from(Rgb565::new(r, g, b)).into_inner();

            if step % 10 == 0 {
                content.push_str("\n\t");
            }
            content.push_str(&format!("{rgb565}, "));
        }
        content.push_str("\n];\n");
        all_content.push_str(&content);
    }

    for &length in &[128, 256] {
        let mut content =
            format!("#[allow(dead_code)]\npub static RAINBOW_RGB666_{length}: [u32; {length}] = [");

        for step in 0..length {
            let r = (f64::sin(2.0 * step as f64 * PI / length as f64) * 31.0 + 32.0) as u8;
            let g =
                (f64::sin(2.0 * step as f64 * PI / length as f64 + PI / 3.0) * 31.0 + 32.0) as u8;
            let b = (f64::sin(2.0 * step as f64 * PI / length as f64 + PI * 2.0 / 3.0) * 31.0
                + 32.0) as u8;
            let rgb666 = RawU24::from(Rgb666::new(r, g, b)).into_inner();

            if step % 10 == 0 {
                content.push_str("\n\t");
            }
            content.push_str(&format!("{rgb666}, "));
        }
        content.push_str("\n];\n");
        all_content.push_str(&content);
    }

    for &length in &[32, 64] {
        let mut content =
            format!("#[allow(dead_code)]\npub static RAINBOW_RGB_U8_{length}: [(u8, u8, u8); {length}] = [");

        for step in 0..length {
            let r = (f64::sin(2.0 * step as f64 * PI / length as f64) * 127.0 + 128.0) as u8;
            let g =
                (f64::sin(2.0 * step as f64 * PI / length as f64 + PI / 3.0) * 127.0 + 128.0) as u8;
            let b = (f64::sin(2.0 * step as f64 * PI / length as f64 + PI * 2.0 / 3.0) * 127.0
                + 128.0) as u8;

            if step % 5 == 0 {
                content.push_str("\n\t");
            }
            content.push_str(&format!("({r}, {g}, {b}), "));
        }
        content.push_str("\n];\n");
        all_content.push_str(&content);
    }

    all_content.push_str(
        r#"
#[allow(dead_code)]
pub fn rgb565_rainbow(step: usize, length: usize) -> Rgb565 {
    let step = step % length;
    let fraction = step as f64 / length as f64;

    let r = (libm::sin(2.0 * PI * fraction) * 15.0 + 16.0) as u8;
    let g = (libm::sin( 2.0 * PI * fraction + PI / 3.0) * 31.0 + 32.0) as u8;
    let b = (libm::sin( 2.0 * PI * fraction + PI * 2.0 / 3.0 ) * 15.0 + 16.0) as u8;

    Rgb565::new(r, g, b)
}

#[allow(dead_code)]
pub fn rgb666_rainbow(step: usize, length: usize) -> Rgb666 {
    let step = step % length;
    let fraction = step as f64 / length as f64;

    let r = (libm::sin(2.0 * PI * fraction) * 31.0 + 32.0) as u8;
    let g = (libm::sin(2.0 * PI * fraction + PI / 3.0) * 31.0 + 32.0) as u8;
    let b = (libm::sin(2.0 * PI * fraction + PI * 2.0 / 3.0) * 31.0 + 32.0) as u8;

    Rgb666::new(r, g, b)
}

#[allow(dead_code)]
pub fn rgb_u8_rainbow(step: usize, length: usize) -> (u8, u8, u8) {
    let step = step % length;
    let fraction = step as f64 / length as f64;

    let r = (libm::sin(2.0 * PI * fraction) * 127.0 + 128.0) as u8;
    let g = (libm::sin(2.0 * PI * fraction + PI / 3.0) * 127.0 + 128.0) as u8;
    let b = (libm::sin(2.0 * PI * fraction + PI * 2.0 / 3.0) * 127.0 + 128.0) as u8;

    (r, g, b)
}
"#,
    );

    fs::write(dest_path, all_content).unwrap();
}
