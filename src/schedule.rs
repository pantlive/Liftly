//! 纯逻辑计时状态机。时间单位统一为毫秒，由调用方传入单调时钟。

use crate::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Working,
    Breaking,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Warn { secs_left: u64 },
    StartBreak,
    EndBreak,
}

#[derive(Debug, Clone)]
pub struct Schedule {
    phase: Phase,
    deadline_ms: u64,
    warned: bool,
    paused_remaining_ms: u64,
    work_ms: u64,
    break_ms: u64,
    warn_ms: u64,
}

impl Schedule {
    pub fn new(cfg: &Config, now: u64) -> Self {
        let (work_ms, break_ms, warn_ms) = Self::durations(cfg);
        Self {
            phase: Phase::Working,
            deadline_ms: now.saturating_add(work_ms),
            warned: false,
            paused_remaining_ms: 0,
            work_ms,
            break_ms,
            warn_ms,
        }
    }

    /// 根据配置计算工作、休息和提醒时长（毫秒）。
    fn durations(cfg: &Config) -> (u64, u64, u64) {
        let work_ms = u64::from(cfg.work_minutes) * 60_000;
        let break_ms = u64::from(cfg.break_minutes) * 60_000;
        // 配置层已经截断过，这里再做一次防御性限制。
        let warn_ms = (u64::from(cfg.warn_seconds) * 1_000).min(work_ms.saturating_sub(1));
        (work_ms, break_ms, warn_ms)
    }

    /// 计算到下一个整秒边界的等待时间，用于每秒刷新倒计时。
    fn until_next_second(deadline_ms: u64, now: u64) -> u64 {
        let remaining = deadline_ms.saturating_sub(now);
        let until_next_second = match remaining % 1_000 {
            0 => 1_000,
            rest => rest,
        };
        until_next_second.min(remaining.max(1)).min(1_000)
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn deadline_ms(&self) -> u64 {
        self.deadline_ms
    }

    /// 提醒已经发出且仍在工作倒计时阶段。
    pub fn warning_active(&self, now: u64) -> bool {
        self.phase == Phase::Working && self.warned && now < self.deadline_ms
    }

    /// 处理一次时钟推进，返回本次发生的事件。
    pub fn tick(&mut self, now: u64) -> Vec<Event> {
        let mut events = Vec::new();
        match self.phase {
            Phase::Working => {
                if now >= self.deadline_ms {
                    // 如果睡眠跨过了提醒点，则只进入休息，不补发提醒。
                    self.phase = Phase::Breaking;
                    self.deadline_ms = now.saturating_add(self.break_ms);
                    self.warned = false;
                    events.push(Event::StartBreak);
                } else if self.warn_ms > 0 && !self.warned {
                    let warn_at = self.deadline_ms.saturating_sub(self.warn_ms);
                    if now >= warn_at {
                        self.warned = true;
                        let remaining = self.deadline_ms - now;
                        events.push(Event::Warn {
                            secs_left: remaining.div_ceil(1_000),
                        });
                    }
                }
            }
            Phase::Breaking => {
                if now >= self.deadline_ms {
                    self.phase = Phase::Working;
                    self.deadline_ms = now.saturating_add(self.work_ms);
                    self.warned = false;
                    events.push(Event::EndBreak);
                }
            }
            Phase::Paused => {}
        }
        events
    }

    /// 距离下一次需要处理的时刻还有多少毫秒。最小 50ms，避免忙等。
    pub fn next_wakeup_ms(&self, now: u64) -> u32 {
        let delay = match self.phase {
            Phase::Working if self.warned => Self::until_next_second(self.deadline_ms, now),
            Phase::Working => {
                let mut target = self.deadline_ms;
                if self.warn_ms > 0 && !self.warned {
                    target = target.min(self.deadline_ms.saturating_sub(self.warn_ms));
                }
                // 最长 60 秒，保证托盘里的“X 分钟”按时刷新。
                target.saturating_sub(now).min(60_000)
            }
            Phase::Breaking => Self::until_next_second(self.deadline_ms, now),
            Phase::Paused => 60_000,
        };
        delay.max(50).min(u32::MAX as u64) as u32
    }

    /// 仅工作阶段可以暂停。
    pub fn pause(&mut self, now: u64) {
        if self.phase == Phase::Working {
            self.paused_remaining_ms = self.deadline_ms.saturating_sub(now);
            self.phase = Phase::Paused;
        }
    }

    pub fn resume(&mut self, now: u64) {
        if self.phase == Phase::Paused {
            self.phase = Phase::Working;
            self.deadline_ms = now.saturating_add(self.paused_remaining_ms);
            self.paused_remaining_ms = 0;
        }
    }

    /// 立即进入休息，忽略当前工作剩余时间。
    pub fn start_break_now(&mut self, now: u64) {
        self.phase = Phase::Breaking;
        self.deadline_ms = now.saturating_add(self.break_ms);
        self.warned = false;
        self.paused_remaining_ms = 0;
    }

    /// 休息时间到或紧急解锁，回到工作阶段。
    pub fn end_break_now(&mut self, now: u64) {
        if self.phase == Phase::Breaking {
            self.phase = Phase::Working;
            self.deadline_ms = now.saturating_add(self.work_ms);
            self.warned = false;
            self.paused_remaining_ms = 0;
        }
    }

    /// 重新开始一个完整的工作周期。
    pub fn reset_work(&mut self, now: u64) {
        self.phase = Phase::Working;
        self.deadline_ms = now.saturating_add(self.work_ms);
        self.warned = false;
        self.paused_remaining_ms = 0;
    }

    /// 应用新配置。工作阶段重新开始，暂停阶段保留暂停状态，休息阶段不打断当前休息。
    pub fn apply_config(&mut self, cfg: &Config, now: u64) {
        let (work_ms, break_ms, warn_ms) = Self::durations(cfg);
        self.work_ms = work_ms;
        self.break_ms = break_ms;
        self.warn_ms = warn_ms;

        match self.phase {
            Phase::Working => self.reset_work(now),
            Phase::Paused => {
                self.paused_remaining_ms = self.work_ms;
                self.warned = false;
            }
            Phase::Breaking => {
                // 当前休息仍按旧截止时间结束，新时长从下一周期开始生效。
            }
        }
    }

    pub fn remaining_ms(&self, now: u64) -> u64 {
        match self.phase {
            Phase::Paused => self.paused_remaining_ms,
            _ => self.deadline_ms.saturating_sub(now),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    fn cfg(work: u32, brk: u32, warn: u32) -> Config {
        Config {
            work_minutes: work,
            break_minutes: brk,
            warn_seconds: warn,
            ..Config::default()
        }
    }

    #[test]
    fn new_starts_working() {
        let schedule = Schedule::new(&cfg(2, 1, 30), 1_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 1_000 + 2 * MINUTE);
        assert_eq!(schedule.remaining_ms(1_000), 2 * MINUTE);
    }

    #[test]
    fn normal_work_break_cycle() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);

        assert!(schedule.tick(2 * MINUTE - 1).is_empty());
        assert_eq!(schedule.phase(), Phase::Working);

        assert_eq!(schedule.tick(2 * MINUTE), vec![Event::StartBreak]);
        assert_eq!(schedule.phase(), Phase::Breaking);
        assert_eq!(schedule.deadline_ms(), 3 * MINUTE);

        assert!(schedule.tick(3 * MINUTE - 1).is_empty());
        assert_eq!(schedule.tick(3 * MINUTE), vec![Event::EndBreak]);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 5 * MINUTE);
    }

    #[test]
    fn warning_is_sent_once() {
        let mut schedule = Schedule::new(&cfg(2, 1, 60), 0);
        assert_eq!(schedule.tick(60_000), vec![Event::Warn { secs_left: 60 }]);
        assert!(schedule.tick(90_000).is_empty());
        assert_eq!(schedule.tick(120_000), vec![Event::StartBreak]);
    }

    #[test]
    fn delayed_tick_skips_warning_when_deadline_crossed() {
        let mut schedule = Schedule::new(&cfg(2, 1, 60), 0);
        assert_eq!(schedule.tick(120_000), vec![Event::StartBreak]);
        assert_eq!(schedule.phase(), Phase::Breaking);
    }

    #[test]
    fn warning_rounds_seconds_up() {
        let mut schedule = Schedule::new(&cfg(2, 1, 60), 0);
        assert_eq!(schedule.tick(60_001), vec![Event::Warn { secs_left: 60 }]);
        assert_eq!(schedule.remaining_ms(60_001), 59_999);
    }

    #[test]
    fn pause_and_resume_keep_remaining_time() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        schedule.pause(30_000);
        assert_eq!(schedule.phase(), Phase::Paused);
        assert_eq!(schedule.remaining_ms(999_999), 90_000);

        // 暂停期间 tick 不产生事件。
        assert!(schedule.tick(999_999).is_empty());
        assert_eq!(schedule.phase(), Phase::Paused);

        schedule.resume(1_000_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 1_090_000);
        assert!(schedule.tick(1_089_999).is_empty());
        assert_eq!(schedule.tick(1_090_000), vec![Event::StartBreak]);
    }

    #[test]
    fn start_break_now_from_working() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        schedule.start_break_now(10_000);
        assert_eq!(schedule.phase(), Phase::Breaking);
        assert_eq!(schedule.deadline_ms(), 70_000);
        assert_eq!(schedule.tick(70_000), vec![Event::EndBreak]);
    }

    #[test]
    fn end_break_now_returns_to_work_early() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        schedule.start_break_now(10_000);
        schedule.end_break_now(20_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 20_000 + 2 * MINUTE);
        assert_eq!(schedule.remaining_ms(20_000), 2 * MINUTE);
    }

    #[test]
    fn reset_work_clears_pause_and_warning() {
        let mut schedule = Schedule::new(&cfg(2, 1, 30), 0);
        assert_eq!(schedule.tick(90_000), vec![Event::Warn { secs_left: 30 }]);
        schedule.pause(100_000);
        schedule.reset_work(200_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 200_000 + 2 * MINUTE);
        assert_eq!(schedule.remaining_ms(200_000), 2 * MINUTE);
    }

    #[test]
    fn next_wakeup_working_honors_warning_and_cap() {
        let schedule = Schedule::new(&cfg(1, 1, 30), 0);
        assert_eq!(schedule.next_wakeup_ms(0), 30_000);

        // 默认 45 分钟工作时，最长 60 秒必须醒来刷新托盘。
        let schedule = Schedule::new(&Config::default(), 0);
        assert_eq!(schedule.next_wakeup_ms(0), 60_000);
    }

    #[test]
    fn next_wakeup_after_warning_ticks_each_second() {
        let mut schedule = Schedule::new(&cfg(1, 1, 30), 0);
        assert_eq!(schedule.next_wakeup_ms(0), 30_000);
        assert_eq!(schedule.tick(30_000), vec![Event::Warn { secs_left: 30 }]);
        assert_eq!(schedule.next_wakeup_ms(30_000), 1_000);
        assert_eq!(schedule.next_wakeup_ms(30_400), 600);
        assert_eq!(schedule.next_wakeup_ms(59_990), 50);
    }

    #[test]
    fn next_wakeup_without_warning_keeps_sixty_second_cap() {
        let schedule = Schedule::new(&cfg(5, 1, 0), 0);
        assert_eq!(schedule.next_wakeup_ms(0), 60_000);
        assert_eq!(schedule.next_wakeup_ms(240_000), 60_000);
        assert_eq!(schedule.next_wakeup_ms(299_000), 1_000);
    }

    #[test]
    fn warning_active_tracks_working_and_paused_phases() {
        let mut schedule = Schedule::new(&cfg(1, 1, 30), 0);
        assert!(!schedule.warning_active(0));
        assert!(!schedule.warning_active(29_999));

        assert_eq!(schedule.tick(30_000), vec![Event::Warn { secs_left: 30 }]);
        assert!(schedule.warning_active(30_000));
        assert!(schedule.warning_active(59_999));
        // 到点后即使还没 tick，也不再处于提醒倒计时。
        assert!(!schedule.warning_active(60_000));

        assert_eq!(schedule.tick(60_000), vec![Event::StartBreak]);
        assert!(!schedule.warning_active(60_000));

        let mut schedule = Schedule::new(&cfg(1, 1, 30), 0);
        assert_eq!(schedule.tick(30_000), vec![Event::Warn { secs_left: 30 }]);
        schedule.pause(40_000);
        assert!(!schedule.warning_active(40_000));
    }

    #[test]
    fn apply_config_restarts_working_cycle() {
        let mut schedule = Schedule::new(&cfg(2, 1, 30), 0);
        assert_eq!(schedule.tick(90_000), vec![Event::Warn { secs_left: 30 }]);

        schedule.apply_config(&cfg(5, 2, 60), 100_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 100_000 + 5 * MINUTE);
        assert_eq!(schedule.remaining_ms(100_000), 5 * MINUTE);
        assert!(!schedule.warning_active(100_000));
        // 提醒点也按新配置重新计算。
        assert_eq!(
            schedule.tick(100_000 + 4 * MINUTE),
            vec![Event::Warn { secs_left: 60 }]
        );
    }

    #[test]
    fn apply_config_keeps_paused_state_with_new_work_time() {
        let mut schedule = Schedule::new(&cfg(2, 1, 30), 0);
        assert_eq!(schedule.tick(90_000), vec![Event::Warn { secs_left: 30 }]);
        schedule.pause(100_000);
        assert_eq!(schedule.remaining_ms(100_000), 20_000);

        schedule.apply_config(&cfg(3, 2, 10), 200_000);
        assert_eq!(schedule.phase(), Phase::Paused);
        assert_eq!(schedule.remaining_ms(200_000), 3 * MINUTE);
        assert!(!schedule.warning_active(200_000));
        assert_eq!(schedule.next_wakeup_ms(200_000), 60_000);

        schedule.resume(200_000);
        assert_eq!(schedule.phase(), Phase::Working);
        assert_eq!(schedule.deadline_ms(), 200_000 + 3 * MINUTE);
        assert_eq!(
            schedule.tick(200_000 + 3 * MINUTE - 10_000),
            vec![Event::Warn { secs_left: 10 }]
        );
    }

    #[test]
    fn apply_config_does_not_interrupt_current_break() {
        let mut schedule = Schedule::new(&cfg(2, 1, 30), 0);
        schedule.start_break_now(10_000);
        assert_eq!(schedule.deadline_ms(), 70_000);

        schedule.apply_config(&cfg(3, 5, 10), 20_000);
        assert_eq!(schedule.phase(), Phase::Breaking);
        assert_eq!(schedule.deadline_ms(), 70_000);

        assert_eq!(schedule.tick(70_000), vec![Event::EndBreak]);
        assert_eq!(schedule.deadline_ms(), 70_000 + 3 * MINUTE);
        assert_eq!(
            schedule.tick(70_000 + 3 * MINUTE - 10_000),
            vec![Event::Warn { secs_left: 10 }]
        );
        // 下一次休息使用新的休息时长。
        schedule.start_break_now(250_000);
        assert_eq!(schedule.deadline_ms(), 250_000 + 5 * MINUTE);
    }

    #[test]
    fn next_wakeup_breaking_refreshes_each_second() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        schedule.start_break_now(0);
        assert_eq!(schedule.next_wakeup_ms(0), 1_000);
        assert_eq!(schedule.next_wakeup_ms(500), 500);
        assert_eq!(schedule.next_wakeup_ms(1_000), 1_000);
        assert_eq!(schedule.next_wakeup_ms(59_500), 500);
        assert_eq!(schedule.next_wakeup_ms(60_000), 50);
    }

    #[test]
    fn next_wakeup_paused_is_a_slow_refresh() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        schedule.pause(1_000);
        assert_eq!(schedule.next_wakeup_ms(1_000), 60_000);
    }

    #[test]
    fn next_wakeup_never_busy_loops() {
        let mut schedule = Schedule::new(&cfg(2, 1, 0), 0);
        assert!(schedule.next_wakeup_ms(2 * MINUTE - 10) >= 50);
        schedule.start_break_now(0);
        assert!(schedule.next_wakeup_ms(60_000) >= 50);
    }
}
