//! Where groups go when a page is wide enough to put two side by side. Pure arithmetic, so it
//! is tested without a window.

/// The narrowest an island may be to share a row.
pub const MIN_COLUMN: f32 = 330.0;

/// The width of each island when two fit side by side in a column `full` points wide with `gap`
/// between them; `None` when they do not, and the groups stack instead.
pub fn two_up(full: f32, gap: f32) -> Option<f32> {
    let each = (full - gap) / 2.0;
    (each >= MIN_COLUMN).then_some(each)
}

/// The height both islands of a row take: the taller one, so the two read as one row.
pub fn row_height(heights: [f32; 2]) -> f32 {
    heights[0].max(heights[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wide_column_takes_two() {
        assert_eq!(two_up(760.0, 12.0), Some(374.0));
    }

    #[test]
    fn a_narrow_column_stacks() {
        assert_eq!(two_up(600.0, 12.0), None);
        // Exactly two minimum columns and a gap still fit.
        assert_eq!(two_up(2.0 * MIN_COLUMN + 12.0, 12.0), Some(MIN_COLUMN));
    }

    #[test]
    fn a_row_is_as_tall_as_its_taller_island() {
        assert_eq!(row_height([120.0, 180.0]), 180.0);
        assert_eq!(row_height([200.0, 80.0]), 200.0);
    }
}
