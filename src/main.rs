#![no_std]
#![no_main]     // なんと、Rustカーネルを作るときはmain関数はだめらしい。
use core::panic::PanicInfo;
use runix::println;

// パニック時のハンドラらしい。カーネルを作るときはこれがないといけない。
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // やった！パニックを表示できた！
    println!("{}", _info);
    loop {}
}


// エントリーポイントらしい。main関数。
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    println!("Hello World{}", "!");

    runix::init(); // 割り込みの初期化

    fn stack_overflow() {
        stack_overflow(); // 再帰呼び出しでスタックオーバーフローを起こす
    }

    stack_overflow();

    loop {}
}
