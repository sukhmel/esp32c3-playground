#![no_main]
#![no_std]
extern crate alloc;

include!(concat!(env!("OUT_DIR"), "/secrets.rs"));

use core::cell::RefCell;
#[cfg(feature = "wifi")]
use crate::buzzer::Melody;
use crate::buzzer::{SoundLed, buzz};
#[cfg(feature = "ble")]
use crate::inter_task::KEYPRESS_CHANNEL;
#[cfg(feature = "wifi")]
use crate::inter_task::{CHAR_CHANNEL, MESSAGE_SIZE};
use crate::inter_task::{ButtonState, BUTTON_STATE_SIGNAL, COORDINATES_CHANNEL, IP_DISPLAY, SOUND_CHANNEL, TOUCH_CHANNEL};
#[cfg(feature = "ble")]
use crate::keyboard::serve_keyboard;
use crate::pins::Peripherals;
use crate::touch::Xpt2046TouchInput;
use ariel_os::asynch::Spawner;
use ariel_os::debug::log::info;
#[cfg(feature = "wifi")]
use ariel_os::debug::log::{debug, error, warn};
use ariel_os::debug::println;
#[cfg(feature = "wifi")]
use ariel_os::reexports::embassy_net::{IpListenEndpoint, Stack, tcp::TcpSocket};
#[cfg(feature = "wifi")]
use ariel_os::time::{Duration, Timer, with_timeout};
use ariel_os_hal::gpio::{Level, Output};
#[cfg(not(feature = "async_ili9341"))]
use core::cell::RefCell;
use critical_section::Mutex;
use display::Display;
use embassy_futures::join::{join4};
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
use esp_hal::gpio::{Event, Input, InputConfig, Io, Pull};
use esp_hal::handler;

mod buzzer;
mod display;
mod input;
pub mod inter_task;
#[cfg(feature = "ble")]
mod keyboard;
mod led;
pub mod pins;
mod touch;

pub mod rainbow {
    include!(concat!(env!("OUT_DIR"), "/rainbows.rs"));
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

static BUTTON: Mutex<RefCell<Option<(Input, ButtonState)>>> = Mutex::new(RefCell::new(None));

/// The button clicks could have been processed with
/// [`embedded_hal_async::digital::Wait::wait_for_low`], but this is more of a learning exercise.
///
/// Although it looks like IRQ slows down the system a bit. I should compare it to waiting.
/// BLE connected with 45 ms time, IRQ, no movement: 286–311 ms
/// BLE connected with 45 ms time, IRQ, movement: 288–677 ms
/// BLE advertises, IRQ, no movement: 233–260 ms
/// BLE advertises, IRQ, movement: 233–447 ms
#[handler]
fn handler() {
    critical_section::with(|cs| {
        let mut button = BUTTON.borrow_ref_mut(cs);
        let Some((button, state)) = button.as_mut() else {
            // Some other interrupt has occurred
            // before the button was set up.
            return;
        };

        if button.is_interrupt_set() {
            match state {
                ButtonState::Pressed => {
                    info!("Button released");
                    let _ = BUTTON_STATE_SIGNAL.signal(ButtonState::Released);
                    button.unlisten();
                    button.listen(Event::LowLevel);
                    *state = ButtonState::Released;
                }
                ButtonState::Released => {
                    info!("Button pressed");
                    let _ = BUTTON_STATE_SIGNAL.signal(ButtonState::Pressed);
                    button.unlisten();
                    button.listen(Event::HighLevel);
                    *state = ButtonState::Pressed;
                }
            }
        }
    });
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
    // let ledc = Ledc::new(peripherals.binary.ledc);
    // let rmt = Rmt::new(peripherals.binary.rmt, Rate::from_mhz(80)).unwrap();
    // let buzzer = SoundLed::new(peripherals.binary.pin19, ledc, peripherals.binary.pin8, rmt);

    let mut io = Io::new(peripherals.system.io_mux);
    io.set_interrupt_handler(handler);
    // Set up the input and store it in the static variable.
    // This example uses a push button that is high when not
    // pressed and low when pressed.
    let config = InputConfig::default().with_pull(Pull::Up);
    let mut button = Input::new(peripherals.binary.pin19, config);
    critical_section::with(|cs| {
        button.listen(Event::LowLevel);
        BUTTON.borrow_ref_mut(cs).replace((button, ButtonState::Released));
    });

    info!("Starting join");
    #[cfg(feature = "ble")]
    let keyboard = serve_keyboard(KEYPRESS_CHANNEL.receiver());
    // Keep the join arity stable without BLE; pending() never resolves and
    // never touches the time driver.
    #[cfg(not(feature = "ble"))]
    let keyboard = core::future::pending::<()>();
    let _ = join4(
        keyboard,
        display.debug_input(
            COORDINATES_CHANNEL.receiver(),
            IP_DISPLAY.receiver().unwrap(),
            TOUCH_CHANNEL.receiver(),
        ),
        // buzz(peripherals.binary.pin19, ledc, SOUND_CHANNEL.receiver()),
        touch.run(),
        input::read_joystick(peripherals.analog),
    )
    .await;
    info!("Finished UI");
}

#[allow(dead_code)]
async fn blast_sound<'a>(speaker: impl OutputPin, ledc: Ledc<'static>) {
    buzz(speaker, ledc, SOUND_CHANNEL.receiver()).await;
}

#[cfg(feature = "wifi")]
#[ariel_os::task()]
async fn network(spawner: Spawner) {
    info!(
        "Hello from main()! Running on a {} board",
        ariel_os::buildinfo::BOARD,
    );
    let net = ariel_os::net::network_stack().await.unwrap();
    info!("Connecting to {}", WIFI_SSID);
    net.wait_config_up().await;
    info!("net up");
    if let Some(ip) = net.config_v4() {
        info!("IP: {:?}", ip.address.address());
        let mut channel_msg = heapless::String::<MESSAGE_SIZE>::new();
        if core::fmt::write(
            &mut channel_msg,
            format_args!("{}:8080", ip.address.address()),
        )
        .is_ok()
        {
            // Latest-value channel: always shows the current address, never blocks.
            IP_DISPLAY.sender().send(channel_msg);
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

/// Resolves once the BLE connection state watch reports `target`.
#[cfg(feature = "wifi")]
async fn wait_for(ble: &mut inter_task::BleStateReceiver, target: bool) {
    while ble.changed().await != target {}
}

#[cfg(feature = "wifi")]
#[ariel_os::task(pool_size = 1)]
async fn run_echo_server(stack: Stack<'static>) -> ! {
    let mut rx_buffer = [0; 64];
    let mut tx_buffer = [0; 64];
    let mut echo_buffer = [0; 64];

    let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
    info!("Server function started. Listening on port 8080...");

    loop {
        debug!("creating socket");
        let mut begin = true;
        let accept = socket.accept(IpListenEndpoint::from(8080)).await;
        match accept {
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
                    // Show the most recent line on screen (latest-value watch).
                    IP_DISPLAY.sender().send(channel_msg);
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
    #[cfg(feature = "wifi")]
    spawner.spawn(network(spawner)).unwrap();
    #[cfg(not(feature = "wifi"))]
    let _ = spawner;
}
