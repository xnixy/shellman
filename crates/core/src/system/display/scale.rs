//! Per-monitor scale. Windows documents no call for it; the one every tool uses is
//! DisplayConfigGetDeviceInfo / SetDeviceInfo with the custom types -3 (get: min, current and
//! max, each a step count relative to the recommended step) and -4 (set). Layouts and the step
//! table are from https://github.com/lihas/windows-DPI-scaling-sample (DpiHelper.h):
//!   GET { header; i32 minScaleRel; i32 curScaleRel; i32 maxScaleRel }  SET { header; i32 scaleRel }
//!   steps 100,125,150,175,200,225,250,300,350,400,450,500
//! The minimum counts down from the recommended step to the table's start, so the recommended
//! step's index is `-min`.

pub const STEPS: [u32; 12] = [100, 125, 150, 175, 200, 225, 250, 300, 350, 400, 450, 500];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scale {
    pub min: i32,
    pub cur: i32,
    pub max: i32,
}

impl Scale {
    fn percent_at(&self, rel: i32) -> Option<u32> {
        let i = -self.min + rel;
        usize::try_from(i).ok().and_then(|i| STEPS.get(i).copied())
    }

    pub fn current(&self) -> Option<u32> {
        self.percent_at(self.cur)
    }

    pub fn recommended(&self) -> Option<u32> {
        self.percent_at(0)
    }

    /// Every step the monitor takes, smallest first.
    pub fn options(&self) -> Vec<u32> {
        (self.min..=self.max).filter_map(|r| self.percent_at(r)).collect()
    }

    /// The relative value to send for a wanted percentage, if the monitor takes it.
    pub fn rel_for(&self, percent: u32) -> Option<i32> {
        (self.min..=self.max).find(|r| self.percent_at(*r) == Some(percent))
    }

    /// Whether the numbers describe a table that exists; garbage is not shown.
    pub fn sane(&self) -> bool {
        self.min <= 0 && self.min <= self.cur && self.cur <= self.max && self.current().is_some() && self.percent_at(self.max).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recommended 150% (index 2): two steps down to 100%, nine up to 500%.
    const S: Scale = Scale { min: -2, cur: 0, max: 9 };

    #[test]
    fn reads_percentages_relative_to_the_recommended_step() {
        assert_eq!(S.current(), Some(150));
        assert_eq!(S.recommended(), Some(150));
        assert_eq!(Scale { cur: 5, ..S }.current(), Some(300));
        assert_eq!(S.options().first(), Some(&100));
        assert_eq!(S.options().last(), Some(&500));
        assert_eq!(S.options().len(), 12);
    }

    #[test]
    fn a_percentage_maps_back_to_its_relative_step() {
        assert_eq!(S.rel_for(150), Some(0));
        assert_eq!(S.rel_for(100), Some(-2));
        assert_eq!(S.rel_for(300), Some(5));
        assert_eq!(S.rel_for(130), None);
        assert_eq!(Scale { min: 0, cur: 0, max: 2 }.rel_for(200), None);
    }

    #[test]
    fn nonsense_is_not_sane() {
        assert!(S.sane());
        assert!(!Scale { min: 1, cur: 1, max: 2 }.sane());
        assert!(!Scale { min: -2, cur: 40, max: 50 }.sane());
    }
}
