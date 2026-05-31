use ariel_os::debug::log::info;
use ariel_os::time::{Instant, Timer};
use ariel_os_hal::gpio::{Level, Output};

use crate::pins::Peripherals;

struct Agm1264fDisplay {
    db0: Output,
    db1: Output,
    db2: Output,
    db3: Output,
    db4: Output,
    db5: Output,
    db6: Output,
    db7: Output,
    data: Output,
    cs1: Output,
    cs2: Output,
    read: Output,
    enable: Output,
}

impl Agm1264fDisplay {
    fn new(peripherals: Peripherals) -> Self {
        Self {
            db0: Output::new(peripherals.pin0, Level::Low),
            db1: Output::new(peripherals.pin1, Level::Low),
            db2: Output::new(peripherals.pin2, Level::Low),
            db3: Output::new(peripherals.pin3, Level::Low),
            db4: Output::new(peripherals.pin4, Level::Low),
            db5: Output::new(peripherals.pin5, Level::Low),
            db6: Output::new(peripherals.pin6, Level::Low),
            db7: Output::new(peripherals.pin7, Level::Low),
            data: Output::new(peripherals.pin8, Level::High),
            enable: Output::new(peripherals.pin10, Level::High),
            cs1: Output::new(peripherals.pin18, Level::Low),
            cs2: Output::new(peripherals.pin19, Level::Low),
            read: Output::new(peripherals.pin9, Level::Low), // pulled down, unconnected
        }
    }

    async fn set_address(&mut self, address: u8) {
        if address >= 64 {
            return;
        }
        self.data.set_low();
        self.read.set_low();
        self.db7.set_low();
        self.db6.set_high();
        self.db5.set_level(if address & 0b00100000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db4.set_level(if address & 0b00010000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db3.set_level(if address & 0b00001000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db2.set_level(if address & 0b00000100 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db1.set_level(if address & 0b00000010 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db0.set_level(if address & 0b00000001 != 0 {
            Level::High
        } else {
            Level::Low
        });
        Timer::after_nanos(1000).await;
        self.reset_pins();
    }

    async fn set_page(&mut self, page: u8) {
        if page >= 8 {
            return;
        }
        self.data.set_low();
        self.read.set_low();
        // self.cs1.set_level(if first {Level::High} else {Level::Low});
        // self.cs2.set_level(if second {Level::High} else {Level::Low});
        self.db7.set_high();
        self.db6.set_low();
        self.db5.set_high();
        self.db4.set_high();
        self.db3.set_high();
        self.db2.set_level(if page & 0b00000100 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db1.set_level(if page & 0b00000010 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db0.set_level(if page & 0b00000001 != 0 {
            Level::High
        } else {
            Level::Low
        });
        Timer::after_nanos(1000).await;
        self.reset_pins();
    }

    async fn display_start_line(&mut self, line: u8) {
        if line >= 64 {
            return;
        }
        self.data.set_low();
        self.read.set_low();
        self.db7.set_high();
        self.db6.set_high();
        self.db5.set_level(if line & 0b00100000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db4.set_level(if line & 0b00010000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db3.set_level(if line & 0b00001000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db2.set_level(if line & 0b00000100 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db1.set_level(if line & 0b00000010 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db0.set_level(if line & 0b00000001 != 0 {
            Level::High
        } else {
            Level::Low
        });
        Timer::after_nanos(1000).await;
        self.reset_pins();
    }

    fn reset_pins(&mut self) {
        self.db0.set_low();
        self.db1.set_low();
        self.db2.set_low();
        self.db3.set_low();
        self.db4.set_low();
        self.db5.set_low();
        self.db6.set_low();
        self.db7.set_low();
        self.cs1.set_low();
        self.cs2.set_low();
        self.read.set_low();
        self.data.set_high();
        self.enable.set_high();
    }

    async fn power(&mut self, on: bool) {
        self.data.set_low();
        self.read.set_low();
        self.db7.set_low();
        self.db6.set_low();
        self.db5.set_high();
        self.db4.set_high();
        self.db3.set_high();
        self.db2.set_high();
        self.db1.set_high();
        if on {
            self.db0.set_high();
        } else {
            self.db0.set_high();
        }

        Timer::after_nanos(1000).await;

        self.reset_pins();
    }

    /// I think the timings are wrong, but maybe the pins are, I did not manage to make it work.
    async fn write(&mut self, first: bool, second: bool, data: u8) {
        let start = Instant::now();
        self.enable.set_low();
        Timer::after_nanos(310).await;
        self.data.set_low();
        self.cs1
            .set_level(if first { Level::High } else { Level::Low });
        self.cs2
            .set_level(if second { Level::High } else { Level::Low });
        Timer::after_nanos(140).await;
        self.enable.set_high();
        self.db0.set_level(if data & 0b00000001 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db1.set_level(if data & 0b00000010 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db2.set_level(if data & 0b00000100 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db3.set_level(if data & 0b00001000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db4.set_level(if data & 0b00010000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db5.set_level(if data & 0b00100000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db6.set_level(if data & 0b01000000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        self.db7.set_level(if data & 0b10000000 != 0 {
            Level::High
        } else {
            Level::Low
        });
        Timer::after_nanos(450).await;
        self.enable.set_low();
        Timer::after_nanos((1000 - start.elapsed().as_nanos()).min(10)).await;
        self.reset_pins();
    }
}

#[allow(dead_code)]
async fn control_display(peripherals: Peripherals) {
    Timer::after_millis(10000).await;
    let mut display = Agm1264fDisplay::new(peripherals);
    display.power(true).await;
    display.set_page(0).await;
    display.set_address(32).await;
    display.display_start_line(0).await;
    for index in 0..=255 {
        display.write(true, false, index).await;
        Timer::after_millis(100).await;
    }
    info!("Display write 1 complete.");
    display.set_page(0).await;
    for index in 0..=255 {
        if index % 64 == 0 {
            display.set_page(index / 64).await;
        }
        display.write(false, true, index).await;
    }
    info!("Display write 2 complete.");
    Timer::after_millis(10000).await;
    display.power(false).await;
    info!("Display power off.");
    Timer::after_millis(10000).await;
}
