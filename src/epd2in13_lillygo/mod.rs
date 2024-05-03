//! Based on the 2.13" driver and https://github.com/martinberlin/CalEPD

use embedded_hal::{
    delay::DelayNs,
    digital::{InputPin, OutputPin},
    spi::SpiDevice,
};

use crate::buffer_len;
use crate::color::Color;
use crate::interface::DisplayInterface;
use crate::traits::{InternalWiAdditions, RefreshLut, WaveshareDisplay};

pub(crate) mod command;
use self::command::{
    BorderWaveForm, BorderWaveFormFixLevel, BorderWaveFormGs, BorderWaveFormVbd, Command,
    DataEntryModeDir, DataEntryModeIncr, DeepSleepMode, DisplayUpdateControl2, DriverOutput
};

pub(crate) mod constants;

use self::constants::{LUT_FULL_UPDATE, LUT_PARTIAL_UPDATE};

/// Full size buffer for use with the 2in13 v2 and v3 EPD
#[cfg(feature = "graphics")]
pub type Display2in13 = crate::graphics::Display<
    WIDTH,
    HEIGHT,
    false,
    { buffer_len(WIDTH as usize, HEIGHT as usize) },
    Color,
>;

/// Width of the display.
pub const WIDTH: u32 = 122;

/// Height of the display
pub const HEIGHT: u32 = 250;

/// Default Background Color
pub const DEFAULT_BACKGROUND_COLOR: Color = Color::White;
const IS_BUSY_LOW: bool = false;
const SINGLE_BYTE_WRITE: bool = true;

/// Epd2in13 (V2 & V3) driver
///
/// To use this driver for V2 of the display, feature \"epd2in13_v3\" needs to be disabled and feature \"epd2in13_v2\" enabled.
pub struct Epd2in13<SPI, BUSY, DC, RST, DELAY> {
    /// Connection Interface
    interface: DisplayInterface<SPI, BUSY, DC, RST, DELAY, SINGLE_BYTE_WRITE>,

    sleep_mode: DeepSleepMode,

    /// Background Color
    background_color: Color,
    refresh: RefreshLut,
}

impl<SPI, BUSY, DC, RST, DELAY> InternalWiAdditions<SPI, BUSY, DC, RST, DELAY>
    for Epd2in13<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    fn init(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.reset(delay, 10_000, 10_000);

        self.wait_until_idle(spi, delay)?;
        self.command(spi, Command::SwReset)?;
        self.wait_until_idle(spi, delay)?;

        self.set_driver_output(
            spi,
            DriverOutput {
                scan_is_linear: false,
                scan_g0_is_first: false,
                scan_dir_incr: false,
                width: (HEIGHT - 1) as u16,
            },
        )?;

        self.set_data_entry_mode(spi, DataEntryModeIncr::XDecrYIncr, DataEntryModeDir::XDir)?;

        self.set_ram_area(spi, 0, 0, WIDTH - 1, HEIGHT - 1)?;

        self.set_border_waveform(
            spi,
            BorderWaveForm {
                vbd: BorderWaveFormVbd::Gs,
                fix_level: BorderWaveFormFixLevel::Vss,
                gs_trans: BorderWaveFormGs::Lut3,
            },
        )?;

        // self.set_di
        self.cmd_with_data(spi, Command::DisplayUpdateControl1, &[0x00, 0x80])?;

        self.cmd_with_data(spi, Command::TemperatureSensorControlRead, &[0x80])?;

        self.set_ram_address_counters(spi, delay, 0, 0)?;

        self.wait_until_idle(spi, delay)?;
        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> WaveshareDisplay<SPI, BUSY, DC, RST, DELAY>
    for Epd2in13<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    type DisplayColor = Color;
    fn new(
        spi: &mut SPI,
        busy: BUSY,
        dc: DC,
        rst: RST,
        delay: &mut DELAY,
        delay_us: Option<u32>,
    ) -> Result<Self, SPI::Error> {
        let mut epd = Epd2in13 {
            interface: DisplayInterface::new(busy, dc, rst, delay_us),
            sleep_mode: DeepSleepMode::Mode1,
            background_color: DEFAULT_BACKGROUND_COLOR,
            refresh: RefreshLut::Full,
        };

        epd.init(spi, delay)?;
        Ok(epd)
    }

    fn wake_up(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.init(spi, delay)
    }

    fn sleep(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;

        // All sample code enables and disables analog/clocks...
        self.set_display_update_control_2(
            spi,
            DisplayUpdateControl2::new()
                .enable_analog()
                .enable_clock()
                .disable_analog()
                .disable_clock(),
        )?;
        self.command(spi, Command::MasterActivation)?;

        self.set_sleep_mode(spi, self.sleep_mode)?;
        Ok(())
    }

    fn update_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        assert!(buffer.len() == buffer_len(WIDTH as usize, HEIGHT as usize));

        self.init(spi, delay)?;

        self.set_ram_area(spi, 0, 0, WIDTH - 1, HEIGHT - 1)?;
        self.set_ram_address_counters(spi, delay, 0, 0)?;

        self.cmd_with_data(spi, Command::WriteRam, buffer)?;

        // if self.refresh == RefreshLut::Full {
        //     // Always keep the base buffer equal to current if not doing partial refresh.
        //     self.set_ram_area(spi, 0, 0, WIDTH - 1, HEIGHT - 1)?;
        //     self.set_ram_address_counters(spi, delay, 0, 0)?;

        //     self.cmd_with_data(spi, Command::WriteRamRed, buffer)?;
        // }
        Ok(())
    }

    /// Updating only a part of the frame is not supported when using the
    /// partial refresh feature. The function will panic if called when set to
    /// use partial refresh.
    fn update_partial_frame(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<(), SPI::Error> {
        assert!((width * height / 8) as usize == buffer.len());
        assert!(false);


        Ok(())
    }

    /// Never use directly this function when using partial refresh, or also
    /// keep the base buffer in syncd using `set_partial_base_buffer` function.
    fn display_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        // if self.refresh == RefreshLut::Full {
        self.set_display_update_control_2(
            spi,
            DisplayUpdateControl2::new()
                .enable_clock()
                .enable_analog()
                .display()
                .disable_analog()
                .disable_clock(),
        )?;
        // } else {
        //     self.set_display_update_control_2(spi, DisplayUpdateControl2::new().display())?;
        // }
        self.command(spi, Command::MasterActivation)?;
        self.wait_until_idle(spi, delay)?;

        Ok(())
    }

    fn update_and_display_frame(
        &mut self,
        spi: &mut SPI,
        buffer: &[u8],
        delay: &mut DELAY,
    ) -> Result<(), SPI::Error> {
        self.update_frame(spi, buffer, delay)?;
        self.display_frame(spi, delay)?;

        // if self.refresh == RefreshLut::Quick {
        //     self.set_partial_base_buffer(spi, delay, buffer)?;
        // }
        Ok(())
    }

    fn clear_frame(&mut self, spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        assert!(false);
        Ok(())
    }

    fn set_background_color(&mut self, background_color: Color) {
        self.background_color = background_color;
    }

    fn background_color(&self) -> &Color {
        &self.background_color
    }

    fn width(&self) -> u32 {
        WIDTH
    }

    fn height(&self) -> u32 {
        HEIGHT
    }

    fn set_lut(
        &mut self,
        spi: &mut SPI,
        _delay: &mut DELAY,
        refresh_rate: Option<RefreshLut>,
    ) -> Result<(), SPI::Error> {
        let buffer = match refresh_rate {
            Some(RefreshLut::Full) | None => &LUT_FULL_UPDATE,
            Some(RefreshLut::Quick) => &LUT_PARTIAL_UPDATE,
        };

        self.cmd_with_data(spi, Command::WriteLutRegister, buffer)
    }

    fn wait_until_idle(&mut self, _spi: &mut SPI, delay: &mut DELAY) -> Result<(), SPI::Error> {
        self.interface.wait_until_idle(delay, IS_BUSY_LOW);
        Ok(())
    }
}

impl<SPI, BUSY, DC, RST, DELAY> Epd2in13<SPI, BUSY, DC, RST, DELAY>
where
    SPI: SpiDevice,
    BUSY: InputPin,
    DC: OutputPin,
    RST: OutputPin,
    DELAY: DelayNs,
{
    /// When using partial refresh, the controller uses the provided buffer for
    /// comparison with new buffer.
    pub fn set_partial_base_buffer(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        buffer: &[u8],
    ) -> Result<(), SPI::Error> {
        assert!(false);
        // assert!(buffer_len(WIDTH as usize, HEIGHT as usize) == buffer.len());
        // self.set_ram_area(spi, 0, 0, WIDTH - 1, HEIGHT - 1)?;
        // self.set_ram_address_counters(spi, delay, 0, 0)?;

        // self.cmd_with_data(spi, Command::WriteRamRed, buffer)?;
        Ok(())
    }

    /// Selects which sleep mode will be used when triggering the deep sleep.
    pub fn set_deep_sleep_mode(&mut self, mode: DeepSleepMode) {
        self.sleep_mode = mode;
    }

    /// Sets the refresh mode. When changing mode, the screen will be
    /// re-initialized accordingly.
    pub fn set_refresh(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        refresh: RefreshLut,
    ) -> Result<(), SPI::Error> {
        if self.refresh != refresh {
            self.refresh = refresh;
            self.init(spi, delay)?;
        }
        Ok(())
    }

    fn set_border_waveform(
        &mut self,
        spi: &mut SPI,
        borderwaveform: BorderWaveForm,
    ) -> Result<(), SPI::Error> {
        self.cmd_with_data(
            spi,
            Command::BorderWaveformControl,
            &[0x05],
        )
    }

    /// Prepare the actions that the next master activation command will
    /// trigger.
    fn set_display_update_control_2(
        &mut self,
        spi: &mut SPI,
        value: DisplayUpdateControl2,
    ) -> Result<(), SPI::Error> {
        self.cmd_with_data(spi, Command::DisplayUpdateControl2, &[0xF7])
    }

    /// Triggers the deep sleep mode
    fn set_sleep_mode(&mut self, spi: &mut SPI, mode: DeepSleepMode) -> Result<(), SPI::Error> {
        self.cmd_with_data(spi, Command::DeepSleepMode, &[0x01])
    }

    fn set_driver_output(&mut self, spi: &mut SPI, output: DriverOutput) -> Result<(), SPI::Error> {
        self.cmd_with_data(spi, Command::DriverOutputControl, &[0xF9, 0x00, 0x00])
    }

    /// Sets the data entry mode (ie. how X and Y positions changes when writing
    /// data to RAM)
    fn set_data_entry_mode(
        &mut self,
        spi: &mut SPI,
        counter_incr_mode: DataEntryModeIncr,
        counter_direction: DataEntryModeDir,
    ) -> Result<(), SPI::Error> {
        let mode = counter_incr_mode as u8 | counter_direction as u8;
        self.cmd_with_data(spi, Command::DataEntryModeSetting, &[0b00000011])
    }

    /// Sets both X and Y pixels ranges
    fn set_ram_area(
        &mut self,
        spi: &mut SPI,
        start_x: u32,
        start_y: u32,
        end_x: u32,
        end_y: u32,
    ) -> Result<(), SPI::Error> {
        self.cmd_with_data(
            spi,
            Command::SetRamXAddressStartEndPosition,
            &[0x00, 0x0f],
        )?;

        self.cmd_with_data(
            spi,
            Command::SetRamYAddressStartEndPosition,
            &[
                0x00 as u8,
                0x00 as u8,
                0xF9 as u8,
                0x00 as u8,
            ],
        )?;
        Ok(())

    }

    /// Sets both X and Y pixels counters when writing data to RAM
    fn set_ram_address_counters(
        &mut self,
        spi: &mut SPI,
        delay: &mut DELAY,
        x: u32,
        y: u32,
    ) -> Result<(), SPI::Error> {
        self.wait_until_idle(spi, delay)?;
        self.cmd_with_data(spi, Command::SetRamXAddressCounter, &[0x00])?;

        self.cmd_with_data(
            spi,
            Command::SetRamYAddressCounter,
            &[0x00],
        )?;
        Ok(())
    }

    fn command(&mut self, spi: &mut SPI, command: Command) -> Result<(), SPI::Error> {
        self.interface.cmd(spi, command)
    }

    fn cmd_with_data(
        &mut self,
        spi: &mut SPI,
        command: Command,
        data: &[u8],
    ) -> Result<(), SPI::Error> {
        self.interface.cmd_with_data(spi, command, data)
    }
}
