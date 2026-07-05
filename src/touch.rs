//! Based on <https://github.com/lampaBiurkowa/esp32-ili9341-slint/blob/master/src/touch_input.rs>

use crate::inter_task::TOUCH_CHANNEL;
use ariel_os::debug::log::{info, warn};
use ariel_os::time::{Duration, Timer};
use embassy_embedded_hal::shared_bus::asynch::spi::SpiDeviceWithConfig;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embedded_graphics::geometry::Point;
use esp_hal::time::Rate;
use esp_hal::{
    Async, Blocking,
    delay::Delay,
    gpio::{Input, InputPin, Level, Output, OutputPin},
    spi::master::{Config, Spi},
};
use xpt2046_async::Xpt2046;

#[derive(Debug)]
pub(crate) enum TouchInputError {
    Xpt2046Init,
    AcquireInputData,
}

#[derive(Debug)]
pub enum TouchInputResponse {
    Moved { x: i32, y: i32 },
    Pressed { x: i32, y: i32 },
    Released { x: i32, y: i32 },
    NoInput,
}

pub(crate) trait TouchInputProvider {
    async fn get_input(&mut self) -> Result<TouchInputResponse, TouchInputError>;
}

pub(crate) struct Xpt2046TouchInput<'a> {
    driver: Xpt2046<SpiDeviceWithConfig<'a, NoopRawMutex, Spi<'a, Async>, Output<'a>>, Input<'a>>,
    last_pos: Option<(i32, i32)>,
    screen_width: i32,
}

impl<'a> Xpt2046TouchInput<'a> {
    pub(crate) fn create(
        spi: &'a Mutex<NoopRawMutex, Spi<'a, Async>>,
        touch_cs_pin: impl OutputPin + 'a,
        irq_pin: impl InputPin + 'a,
        screen_width: i32,
    ) -> Result<Self, TouchInputError> {
        let touch_irq_pin = Input::new(irq_pin, Default::default());
        let touch_cs = Output::new(touch_cs_pin, Level::High, Default::default());
        let touch_spi_dev = SpiDeviceWithConfig::new(
            spi,
            touch_cs,
            Config::default().with_frequency(Rate::from_mhz(5)),
        );
        let xpt = Xpt2046::new(
            touch_spi_dev,
            touch_irq_pin,
            xpt2046_async::Orientation::LandscapeFlipped,
        );
        Ok(Self {
            driver: xpt,
            last_pos: None,
            screen_width,
        })
    }

    pub(crate) async fn init(&mut self) -> Result<(), TouchInputError> {
        self.driver
            .init(&mut Delay::new())
            .await
            .map_err(|_| TouchInputError::Xpt2046Init)
    }

    pub(crate) async fn run(&mut self) -> Result<(), TouchInputError> {
        info!("touch: task started");
        self.init().await?;
        loop {
            if let Ok(x) = self.get_input().await {
                if let Err(_) = TOUCH_CHANNEL.try_send(x) {
                    warn!("Failed to send touch input to channel")
                }
            }
            Timer::after(Duration::from_millis(10)).await
        }
    }
}

impl<'a> TouchInputProvider for Xpt2046TouchInput<'a> {
    async fn get_input(&mut self) -> Result<TouchInputResponse, TouchInputError> {
        self.driver
            .run()
            .await
            .map_err(|_| TouchInputError::AcquireInputData)?;

        if self.driver.is_touched() {
            // TODO: make adjustments, maybe configurable
            let p = self.driver.get_touch_point();
            let x = self.screen_width - 2 * p.x;
            let y = 2 * p.y;

            match self.last_pos.replace((x, y)) {
                Some(prev) if (prev.0 != x && prev.1 != y) => {
                    Ok(TouchInputResponse::Moved { x, y })
                }
                None => Ok(TouchInputResponse::Pressed { x, y }),
                _ => Ok(TouchInputResponse::Moved { x, y }),
            }
        } else if let Some(prev) = self.last_pos.take() {
            Ok(TouchInputResponse::Released {
                x: prev.0,
                y: prev.1,
            })
        } else {
            Ok(TouchInputResponse::NoInput)
        }
    }
}
