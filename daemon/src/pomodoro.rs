// SPDX-License-Identifier: GPL-3.0-or-later

//! Work and break timer: Idle -> Work -> Break -> Idle. Not persisted.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Idle,
    Work,
    Break,
}

#[derive(Clone, Debug)]
pub struct Pomodoro {
    pub phase: Phase,
    pub running: bool,
    remaining_ms: u64,
    last_ms: u64,
    work_ms: u64,
    break_ms: u64,
}

impl Pomodoro {
    pub fn new(work_min: u64, break_min: u64) -> Self {
        Self {
            phase: Phase::Idle,
            running: false,
            remaining_ms: 0,
            last_ms: 0,
            work_ms: work_min * 60_000,
            break_ms: break_min * 60_000,
        }
    }

    /// Starts the work phase from rest, otherwise pauses or resumes.
    pub fn tap(&mut self, now: u64) {
        match (self.phase, self.running) {
            (Phase::Idle, _) => {
                self.phase = Phase::Work;
                self.remaining_ms = self.work_ms;
                self.running = true;
            }
            (_, true) => {
                self.advance(now);
                self.running = false;
            }
            (_, false) => self.running = true,
        }
        self.last_ms = now;
    }

    pub fn reset(&mut self) {
        self.phase = Phase::Idle;
        self.running = false;
        self.remaining_ms = 0;
    }

    /// Moves the clock to `now`. Returns the phase entered when one ended.
    pub fn advance(&mut self, now: u64) -> Option<Phase> {
        let elapsed = now.saturating_sub(self.last_ms);
        self.last_ms = now;
        if !self.running {
            return None;
        }
        if elapsed < self.remaining_ms {
            self.remaining_ms -= elapsed;
            return None;
        }
        match self.phase {
            Phase::Work => {
                self.phase = Phase::Break;
                self.remaining_ms = self.break_ms;
                Some(Phase::Break)
            }
            _ => {
                self.reset();
                Some(Phase::Idle)
            }
        }
    }

    /// Lucide icon and `MM:SS`.
    pub fn view(&self) -> (&'static str, String) {
        let ms = if self.phase == Phase::Idle {
            self.work_ms
        } else {
            self.remaining_ms
        };
        let seconds = ms.div_ceil(1000);
        let icon = match (self.phase, self.running) {
            (Phase::Idle, _) => "timer",
            (_, true) => "play",
            (_, false) => "pause",
        };
        (icon, format!("{:02}:{:02}", seconds / 60, seconds % 60))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pomodoro_transitions() {
        let mut pomodoro = Pomodoro::new(25, 5);
        assert_eq!(pomodoro.view(), ("timer", "25:00".to_owned()));
        pomodoro.tap(0);
        assert_eq!((pomodoro.phase, pomodoro.running), (Phase::Work, true));
        assert_eq!(pomodoro.advance(60_000), None);
        assert_eq!(pomodoro.view(), ("play", "24:00".to_owned()));
        pomodoro.tap(60_000);
        assert_eq!(pomodoro.view().0, "pause");
        // Paused time does not count.
        assert_eq!(pomodoro.advance(10_000_000), None);
        assert_eq!(pomodoro.view().1, "24:00");
        pomodoro.tap(10_000_000);
        assert_eq!(pomodoro.advance(10_000_000 + 24 * 60_000), Some(Phase::Break));
        assert_eq!(pomodoro.view().1, "05:00");
        let now = 10_000_000 + 24 * 60_000;
        assert_eq!(pomodoro.advance(now + 5 * 60_000), Some(Phase::Idle));
        assert_eq!(pomodoro.phase, Phase::Idle);
        pomodoro.tap(now);
        pomodoro.reset();
        assert_eq!(pomodoro.phase, Phase::Idle);
    }
}
