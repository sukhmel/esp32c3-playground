#![no_main]
#![no_std]
extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/secrets.rs"));

use crate::buzzer::{Melody, SoundLed, buzz};
use crate::inter_task::{
    CHAR_CHANNEL, COORDINATES_CHANNEL, MESSAGE_CHANNEL, MESSAGE_SIZE, SOUND_CHANNEL, TOUCH_CHANNEL,
};
use crate::pins::Peripherals;
use crate::touch::Xpt2046TouchInput;
use ariel_os::asynch::Spawner;
use ariel_os::debug::log::{debug, error, info, warn};
use ariel_os::reexports::embassy_net::{IpListenEndpoint, Stack, tcp::TcpSocket};
use ariel_os::time::{Duration, Timer, with_timeout};
use ariel_os_hal::gpio::{Level, Output};
#[cfg(not(feature = "async_ili9341"))]
use core::cell::RefCell;
use display::Display;
use embassy_futures::join::{join3, join4};
#[cfg(feature = "async_ili9341")]
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
#[cfg(not(feature = "async_ili9341"))]
use embedded_hal_bus::spi::RefCellDevice;
#[cfg(not(feature = "async_ili9341"))]
use esp_hal::delay::Delay;
use esp_hal::gpio::OutputPin;
use esp_hal::ledc::Ledc;
use esp_hal::spi::master::{Config, Spi};
use esp_hal::time::Rate;

mod buzzer;
mod display;
mod input;
pub mod inter_task;
#[cfg(feature = "keyboard")]
mod keyboard;
mod led;
pub mod pins;
mod touch;

pub mod rainbow {
    include!(concat!(env!("OUT_DIR"), "/rainbows.rs"));
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    info!("{}", info);
    loop {}
}

#[ariel_os::task(autostart, peripherals)]
async fn ui(peripherals: Peripherals) {
    info!("Starting UI");
    let raw_spi = Spi::new(
        peripherals.binary.spi,
        Config::default().with_frequency(Rate::from_mhz(5)),
    )
    .unwrap()
    .with_miso(peripherals.binary.pin5)
    .with_mosi(peripherals.binary.pin7)
    .with_sck(peripherals.binary.pin6);
    #[cfg(feature = "async_ili9341")]
    let raw_spi = raw_spi.into_async();
    #[cfg(not(feature = "async_ili9341"))]
    let shared_spi = RefCell::new(raw_spi);
    #[cfg(feature = "async_ili9341")]
    let shared_spi = embassy_sync::mutex::Mutex::<NoopRawMutex, _>::new(raw_spi);
    let cs_pin = Output::new(peripherals.binary.pin10, Level::High);
    let dc_pin = Output::new(peripherals.binary.pin9, Level::Low);
    let rst_pin = Output::new(peripherals.binary.pin18, Level::Low);
    #[cfg(not(feature = "async_ili9341"))]
    let mut buffer = [0u8; 512];
    #[cfg(not(feature = "async_ili9341"))]
    let mut display = Display::new(&shared_spi, cs_pin, dc_pin, rst_pin, &mut buffer);
    #[cfg(not(feature = "async_ili9341"))]
    let touch_spi = RefCellDevice::new(&shared_spi, touch_cs_pin, Delay::new()).unwrap();
    let mut touch = Xpt2046TouchInput::create(
        &shared_spi,
        peripherals.binary.pin4,
        peripherals.binary.pin8,
        320,
    )
    .unwrap();
    #[cfg(feature = "async_ili9341")]
    let mut display = Display::new(&shared_spi, cs_pin, dc_pin, rst_pin).await;
    let ledc = Ledc::new(peripherals.binary.ledc);
    // let rmt = Rmt::new(peripherals.binary.rmt, Rate::from_mhz(80)).unwrap();
    // let buzzer = SoundLed::new(peripherals.binary.pin19, ledc, peripherals.binary.pin8, rmt);
    join4(
        display.debug_input(
            COORDINATES_CHANNEL.receiver(),
            MESSAGE_CHANNEL.receiver(),
            TOUCH_CHANNEL.receiver(),
        ),
        buzz(peripherals.binary.pin19, ledc, SOUND_CHANNEL.receiver()),
        input::read_joystick(peripherals.analog),
        touch.run(),
    )
    .await;
    info!("Finished UI");
}

#[allow(dead_code)]
async fn blast_sound<'a>(speaker: impl OutputPin, ledc: Ledc<'static>) {
    buzz(speaker, ledc, SOUND_CHANNEL.receiver()).await;
}

#[ariel_os::task()]
async fn network(spawner: Spawner) {
    info!(
        "Hello from main()! Running on a {} board",
        ariel_os::buildinfo::BOARD,
    );
    let net = ariel_os::net::network_stack().await.unwrap();
    info!("Connecting to {}", WIFI_SSID);
    net.wait_config_up().await;
    if let Some(ip) = net.config_v4() {
        info!("IP: {:?}", ip.address.address());
        let mut channel_msg = heapless::String::<MESSAGE_SIZE>::new();
        if core::fmt::write(
            &mut channel_msg,
            format_args!("{}:8080", ip.address.address()),
        )
        .is_ok()
        {
            MESSAGE_CHANNEL.send(channel_msg).await;
        } else {
            warn!("Failed to format message");
        }

        if let Err(_) = spawner.spawn(run_echo_server(net)) {
            error!("Failed to spawn the echo server background task: busy");
        }
    } else {
        error!("Failed to get IP address");
    }
}

#[ariel_os::task(pool_size = 1)]
async fn run_echo_server(stack: Stack<'static>) -> ! {
    let mut rx_buffer = [0; 1024];
    let mut tx_buffer = [0; 1024];
    let mut echo_buffer = [0; 1024];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    info!("Server function started. Listening on port 8080...");

    loop {
        debug!("creating socket");
        let mut begin = true;
        match socket.accept(IpListenEndpoint::from(8080)).await {
            Err(e) => {
                error!("Failed to accept client: {:?}", e);
                socket.abort();
                Timer::after_millis(150).await;
                continue;
            }
            Ok(_) => {
                info!("Client connected!");
            }
        }

        'interaction: loop {
            match socket.read(&mut echo_buffer).await {
                Ok(0) => {
                    break;
                } // Client closed connection
                Ok(n) => {
                    let mut channel_msg = heapless::String::<MESSAGE_SIZE>::new();
                    if let Ok(utf8_str) = core::str::from_utf8(&echo_buffer[..n.min(MESSAGE_SIZE)])
                    {
                        // Push the slice into our fixed-capacity heapless string safely
                        let _ = channel_msg.push_str(utf8_str);
                    } else {
                        let _ = channel_msg.push_str("[Invalid UTF-8 Received]");
                    };
                    if n > 3 && &echo_buffer[0..4] == b"STOP" {
                        begin = false;
                        SOUND_CHANNEL.send(None).await;
                    }
                    if n > 3 && &echo_buffer[0..4] == b"LIST" {
                        begin = false;
                        let mut list = heapless::String::<MESSAGE_SIZE>::new();
                        for i in 0.. {
                            let Ok(name) = Melody::try_from(i) else {
                                break;
                            };
                            if core::fmt::write(&mut list, format_args!("{:2} = {}\r\n", i, name))
                                .is_ok()
                            {
                                let _ = channel_msg.push_str(&list);
                                let _ = socket.write(list.as_bytes()).await;
                                list.clear();
                            } else {
                                warn!("Failed to format list");
                                break;
                            }
                        }
                    }
                    if n > 5 && &echo_buffer[0..5] == b"INPUT" {
                        info!("Input started");
                        begin = false;
                        let mut char = heapless::String::<4>::new();
                        CHAR_CHANNEL.clear();
                        while let Ok(ch) =
                            with_timeout(Duration::from_secs(15), CHAR_CHANNEL.receive()).await
                        {
                            if let Err(_) = core::fmt::write(&mut char, format_args!("{}", ch)) {
                                warn!("Failed to write char: {}", ch);
                                char.clear();
                                continue;
                            }
                            if socket.write(char.as_bytes()).await.is_err() {
                                warn!("Failed to write character");
                                break 'interaction;
                            }
                            char.clear();
                        }
                        info!("Input finished");
                        continue;
                    }
                    if n > 5 && &echo_buffer[0..5] == b"PLAY " {
                        begin = false;
                        if let Ok(Ok(command)) = core::str::from_utf8(&echo_buffer[5..(n.min(7))])
                            .map(|s| s.trim().parse::<u8>())
                        {
                            let melody = Melody::try_from(command);
                            if let Ok(melody) = melody {
                                let mut name = heapless::String::<32>::new();
                                if core::fmt::write(
                                    &mut name,
                                    format_args!("Playing {}\r\n", melody),
                                )
                                .is_ok()
                                {
                                    let _ = channel_msg.push_str(&name);
                                    let _ = socket.write(name.as_bytes()).await;
                                }
                                SOUND_CHANNEL.send(Some(melody)).await;
                                continue;
                            } else {
                                warn!("Invalid command received: {}", command);
                            }
                        } else {
                            warn!(
                                "Invalid command received: {:?}",
                                &echo_buffer[0..(n.min(7))]
                            );
                        }
                    }
                    // CHANNEL.send(channel_msg).await;
                    if let Err(_) = MESSAGE_CHANNEL.try_send(channel_msg) {
                        warn!("Channel full! Dropped message");
                    }
                    if begin {
                        begin = false;
                        let mut header = heapless::String::<256>::new();
                        if core::fmt::write(
                            &mut header,
                            format_args!(
                                "HTTP/1.1 200 OK\r\n\
                                 Content-Type: text/plain; charset=utf-8\r\n\
                                 Content-Length: {}\r\n\
                                 Connection: close\r\n\r\n",
                                n
                            ),
                        )
                        .is_ok()
                        {
                            if socket.write(header.as_bytes()).await.is_err() {
                                info!("Failed to write header");
                                break;
                            } else if socket.write(&echo_buffer[..n]).await.is_err() {
                                info!("Failed to write response body");
                                break;
                            }
                        } else {
                            warn!("Failed to format header");
                            break;
                        }
                    } else if socket.write(&echo_buffer[..n]).await.is_err() {
                        info!("Failed to write response body");
                        break;
                    }
                }
                Err(e) => {
                    info!("Read error: {:?}", e);
                    break;
                }
            }
        }

        if let Err(e) = socket.flush().await {
            warn!("Flush error: {:?}", e);
        }
        socket.close();
        Timer::after_millis(150).await;
        // Continuously read until the client sends its matching FIN acknowledgement (returns Ok(0))
        // This stops your server from pulling the rug while curl is still reading the payload.
        let mut discard_buf = [0; 64];
        loop {
            match socket.read(&mut discard_buf).await {
                Ok(0) => break, // Client acknowledged the close! We can exit safely.
                Ok(_) => {}     // Discard any residual padding data
                Err(_) => break,
            }
        }
        info!("Client disconnected");
    }
}

#[ariel_os::spawner(autostart)]
fn main(spawner: Spawner) {
    spawner.spawn(network(spawner)).unwrap();
}
