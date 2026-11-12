//! A monitor's supported modes grouped the way a picker wants them: each resolution once, with
//! the refresh rates it takes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub w: u32,
    pub h: u32,
    pub hz: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub w: u32,
    pub h: u32,
    /// Highest first, each once.
    pub rates: Vec<u32>,
    pub recommended: bool,
}

impl Group {
    pub fn label(&self) -> String {
        format!("{} \u{d7} {}", self.w, self.h)
    }
}

/// Resolutions largest first. The recommended one is the monitor's preferred mode when it is
/// in the list, else the largest.
pub fn group(modes: &[Mode], preferred: Option<(u32, u32)>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for m in modes {
        match groups.iter_mut().find(|g| g.w == m.w && g.h == m.h) {
            Some(g) => {
                if !g.rates.contains(&m.hz) {
                    g.rates.push(m.hz);
                }
            }
            None => groups.push(Group { w: m.w, h: m.h, rates: vec![m.hz], recommended: false }),
        }
    }
    for g in &mut groups {
        g.rates.sort_unstable_by(|a, b| b.cmp(a));
    }
    groups.sort_by(|a, b| (b.w as u64 * b.h as u64, b.w).cmp(&(a.w as u64 * a.h as u64, a.w)));
    let pick = preferred
        .and_then(|(w, h)| groups.iter().position(|g| g.w == w && g.h == h))
        .or(if groups.is_empty() { None } else { Some(0) });
    if let Some(i) = pick {
        groups[i].recommended = true;
    }
    groups
}

/// The rate to use after the resolution changes: the same one if the new resolution has it,
/// else the highest not above the old one, else its highest.
pub fn rate_for(group: &Group, wanted: u32) -> u32 {
    if group.rates.contains(&wanted) {
        return wanted;
    }
    group.rates.iter().copied().find(|r| *r <= wanted).or(group.rates.first().copied()).unwrap_or(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(w: u32, h: u32, hz: u32) -> Mode {
        Mode { w, h, hz }
    }

    #[test]
    fn grouped_by_resolution_with_rates_high_to_low() {
        let g = group(&[m(1920, 1080, 60), m(3840, 2160, 30), m(1920, 1080, 144), m(3840, 2160, 60), m(1920, 1080, 60)], Some((3840, 2160)));
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].w, g[0].rates.clone(), g[0].recommended), (3840, vec![60, 30], true));
        assert_eq!((g[1].w, g[1].rates.clone(), g[1].recommended), (1920, vec![144, 60], false));
    }

    #[test]
    fn without_a_preferred_mode_the_largest_is_recommended() {
        let g = group(&[m(800, 600, 60), m(1024, 768, 60)], None);
        assert!(g[0].recommended && g[0].w == 1024 && !g[1].recommended);
        let g = group(&[m(800, 600, 60), m(1024, 768, 60)], Some((1, 1)));
        assert!(g[0].recommended);
        assert!(group(&[], None).is_empty());
    }

    #[test]
    fn a_new_resolution_keeps_the_rate_or_the_nearest_below() {
        let g = Group { w: 1, h: 1, rates: vec![144, 60, 30], recommended: false };
        assert_eq!(rate_for(&g, 60), 60);
        assert_eq!(rate_for(&g, 100), 60);
        assert_eq!(rate_for(&g, 20), 144);
    }
}
