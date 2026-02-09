use uart_16550::SerialPort;
use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = {
        let mut serial_port = unsafe { SerialPort::new(0x3F8) };
        serial_port.init();
        Mutex::new(serial_port)
    };
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    SERIAL1.lock().write_fmt(args).expect("Printing to serial failed");
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, adding a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($fmt:expr) => ($crate::serial_print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => {
        $crate::serial_print!(
            concat!($fmt, "\n"),
            $($arg)*
        )
    };
}

/// シリアルポートを初期化する便利関数
pub fn init() {
    // lazy_staticが自動的に初期化してくれる
    // 最初はVGAに出力して確認
    crate::vga_buffer::_print(format_args!("Serial port initializing...\n"));
    serial_println!("Serial port initialized");
}

/// シリアルポートに文字列を書き込む便利関数
pub fn write_str(s: &str) {
    _print(format_args!("{}", s));
}

/// シリアルポートに1バイトを書き込む便利関数
pub fn write_byte(byte: u8) {
    use core::fmt::Write;
    let mut serial = SERIAL1.lock();
    serial.write_str(core::str::from_utf8(&[byte]).unwrap_or("?")).expect("Failed to write byte");
}
