//! This is practically verbatim copy of an example found in <https://gitlab.ucc.gu.uwa.edu.au/matt/embassy-random/-/blob/matt/embassy-hack/examples/nrf/src/bin/usb_hid_keyboard.rs>
//! and it does not seem to compile with the current versions. But since it is not yet possible to
//! use USB, it'll do for now.

use ariel_os::debug::log::info;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_usb::class::hid::{ReportId, RequestHandler, State};
use embassy_usb::control::OutResponse;
use embassy_usb::{Builder, Config, Handler};

static SUSPENDED: AtomicBool = AtomicBool::new(false);

fn test() {
    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("Sukhmel");
    config.product = Some("HID keyboard attempt");
    config.serial_number = Some("001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;
    config.supports_remote_wakeup = true;

    // Create embassy-usb DeviceBuilder using the driver and config.
    // It needs some buffers for building the descriptors.
    let mut device_descriptor = [0; 256];
    let mut config_descriptor = [0; 256];
    let mut bos_descriptor = [0; 256];
    let mut control_buf = [0; 64];
    let request_handler = MyRequestHandler {};
    let device_state_handler = MyDeviceStateHandler::new();

    let mut state = State::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut device_descriptor,
        &mut config_descriptor,
        &mut bos_descriptor,
        &mut control_buf,
        Some(&device_state_handler),
    );
}

struct MyRequestHandler {}

impl RequestHandler for MyRequestHandler {
    fn get_report(&self, id: ReportId, _buf: &mut [u8]) -> Option<usize> {
        info!("Get report for {:?}", id);
        None
    }

    fn set_report(&self, id: ReportId, data: &[u8]) -> OutResponse {
        info!("Set report for {:?}: {:?}", id, data);
        OutResponse::Accepted
    }

    fn set_idle_ms(&self, id: Option<ReportId>, dur: u32) {
        info!("Set idle rate for {:?} to {:?}", id, dur);
    }

    fn get_idle_ms(&self, id: Option<ReportId>) -> Option<u32> {
        info!("Get idle rate for {:?}", id);
        None
    }
}

struct MyDeviceStateHandler {
    configured: AtomicBool,
}

impl MyDeviceStateHandler {
    fn new() -> Self {
        MyDeviceStateHandler {
            configured: AtomicBool::new(false),
        }
    }
}

impl Handler for MyDeviceStateHandler {
    fn enabled(&self, enabled: bool) {
        self.configured.store(false, Ordering::Relaxed);
        SUSPENDED.store(false, Ordering::Release);
        if enabled {
            info!("Device enabled");
        } else {
            info!("Device disabled");
        }
    }

    fn reset(&self) {
        self.configured.store(false, Ordering::Relaxed);
        info!("Bus reset, the Vbus current limit is 100mA");
    }

    fn addressed(&self, addr: u8) {
        self.configured.store(false, Ordering::Relaxed);
        info!("USB address set to: {}", addr);
    }

    fn configured(&self, configured: bool) {
        self.configured.store(configured, Ordering::Relaxed);
        if configured {
            info!(
                "Device configured, it may now draw up to the configured current limit from Vbus."
            )
        } else {
            info!("Device is no longer configured, the Vbus current limit is 100mA.");
        }
    }

    fn suspended(&self, suspended: bool) {
        if suspended {
            info!(
                "Device suspended, the Vbus current limit is 500µA (or 2.5mA for high-power devices with remote wakeup enabled)."
            );
            SUSPENDED.store(true, Ordering::Release);
        } else {
            SUSPENDED.store(false, Ordering::Release);
            if self.configured.load(Ordering::Relaxed) {
                info!(
                    "Device resumed, it may now draw up to the configured current limit from Vbus"
                );
            } else {
                info!("Device resumed, the Vbus current limit is 100mA");
            }
        }
    }
}
