use x86_64::instructions::port::Port;

const PIT_FREQUENCY: u32 = 1193182; // PITの基本周波数
const TIMER_INTERVAL: u32 = 1; // 1Hz (instead of 100Hz) for testing

// PITを初期化してタイマー割り込みを開始
pub fn init() {
    let divisor = PIT_FREQUENCY / TIMER_INTERVAL;
    
    unsafe {
        // PITコマンドポート（0x43）
        let mut command_port = Port::new(0x43);
        // チャネル0、square waveモード、アクセスモードはlow/highバイト両方
        command_port.write(0x36u8);
        
        // ディバイダ設定（チャネル0、ポート0x40）
        let mut data_port = Port::new(0x40);
        // 下位バイト
        data_port.write((divisor & 0xFF) as u8);
        // 上位バイト  
        data_port.write(((divisor >> 8) & 0xFF) as u8);
    }
    
    println!("Timer initialized: {}Hz", TIMER_INTERVAL);
}
