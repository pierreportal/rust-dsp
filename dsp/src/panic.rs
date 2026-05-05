use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        // In real embedded code you might:
        // - blink an LED
        // - reset MCU
        // - halt safely
    }
}
