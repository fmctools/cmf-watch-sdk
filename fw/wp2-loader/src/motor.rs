use crate::app::millis;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use wp2_core::pwm::{decide, mark_on, over_cap, pulse, E_LOCKED, E_PAUSED, STOCK_CLOCK};
use crate::plat::stock::{CMU_PWMCLK, PWM_DEVICE};

const CH: u32 = 0;
const PIN_SET_CYCLES: usize = 0;
const PIN_OFF: usize = 4;

// set while a stock popup covers the app (app::pause_tick)
pub static PAUSED: AtomicBool = AtomicBool::new(false);
// set by a cap cut; motor(duty > 0) returns -4 until motor(0), launch or stop_app
static LOCKED: AtomicBool = AtomicBool::new(false);
// millis() | 1 of the off->on switch, 0 while off
static ON_SINCE: AtomicU32 = AtomicU32::new(0);

fn driver(i: usize) -> usize {
    unsafe { *(*((PWM_DEVICE + 8) as *const *const usize)).add(i) }
}

fn set_clock(v: u32) {
    unsafe { (CMU_PWMCLK as *mut u32).write_volatile(v) }
}

pub fn reset() {
    PAUSED.store(false, Ordering::Release);
    LOCKED.store(false, Ordering::Release);
}

// App thread. No BASEPRI around the driver: pin_set_cycles logs through the Zephyr log subsystem before its own
// BASEPRI section, not proven non-blocking. A UI-thread cut that preempts the write is caught by the re-check.
pub extern "C" fn motor(duty_permille: u16, freq_hz: u16) -> i32 { #[cfg(feature = "wp3")] return crate::plat::E_NODEV; // haptic chip: M3
    let (clk, period) = match decide(duty_permille, freq_hz, PAUSED.load(Ordering::Acquire), LOCKED.load(Ordering::Acquire)) {
        Err(e) => return e,
        Ok(None) => {
            off();
            LOCKED.store(false, Ordering::Release);
            return 0;
        }
        Ok(Some(p)) => p,
    };
    mark_on(&ON_SINCE, millis());
    set_clock(clk);
    let set = stock!(driver(PIN_SET_CYCLES), fn(u32, u32, u32, u32, u32) -> i32);
    let rc = set(PWM_DEVICE, CH, period.into(), pulse(period, duty_permille).into(), 1);
    // a cut (cap, pause) ran since the checks above: the watchdog only sets flags before its off()
    if ON_SINCE.load(Ordering::Acquire) == 0 || PAUSED.load(Ordering::Acquire) || LOCKED.load(Ordering::Acquire) {
        off();
        return if LOCKED.load(Ordering::Acquire) { E_LOCKED } else { E_PAUSED };
    }
    // also on a driver error: the watchdog or stop_app restores the stock clock
    rc.min(0)
}

pub fn off() { #[cfg(feature = "wp3")] return;
    stock!(driver(PIN_OFF), fn(u32, u32) -> i32)(PWM_DEVICE, CH);
    set_clock(STOCK_CLOCK);
    ON_SINCE.store(0, Ordering::Release);
}

// UI tick (20 ms): continuous run is capped at CAP_MS, then the motor stays locked; also catches a motor()
// that raced the pause
pub fn watchdog() { #[cfg(feature = "wp3")] return;
    let since = ON_SINCE.load(Ordering::Acquire);
    if since == 0 {
        return;
    }
    if over_cap(millis(), since) {
        LOCKED.store(true, Ordering::Release);
        off();
    } else if PAUSED.load(Ordering::Acquire) {
        off();
    }
}
