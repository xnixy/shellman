//! Performance counters (PDH): one query, its counters added once, collected once per tick.
//! Wildcard counters (`\PhysicalDisk(*)\...`) give one value per instance; instances that come
//! and go between ticks are followed by PDH itself.

use windows::core::HSTRING;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};

/// Values above 100 are kept (byte rates, and shares the counter reports over 100).
const NOCAP100: u32 = 0x8000;
const FORMAT: PDH_FMT = PDH_FMT(PDH_FMT_DOUBLE.0 | NOCAP100);

pub struct Query {
    query: PDH_HQUERY,
    counters: Vec<PDH_HCOUNTER>,
    /// Reused for the arrays, which hold every instance's name.
    buf: Vec<u64>,
}

impl Query {
    pub fn open() -> Option<Self> {
        let mut query = PDH_HQUERY::default();
        (unsafe { PdhOpenQueryW(None, 0, &mut query) } == 0).then(|| Self { query, counters: Vec::new(), buf: Vec::new() })
    }

    /// Add a counter by its English path; its index for reading, or None if this machine lacks it.
    pub fn add(&mut self, path: &str) -> Option<usize> {
        let mut counter = PDH_HCOUNTER::default();
        let status = unsafe { PdhAddEnglishCounterW(self.query, &HSTRING::from(path), 0, &mut counter) };
        if status != 0 {
            return None;
        }
        self.counters.push(counter);
        Some(self.counters.len() - 1)
    }

    pub fn collect(&self) -> bool {
        unsafe { PdhCollectQueryData(self.query) == 0 }
    }

    /// A single counter's value, if it has a valid one (rates need two collections).
    pub fn value(&self, at: usize) -> Option<f64> {
        let mut v = PDH_FMT_COUNTERVALUE::default();
        let status = unsafe { PdhGetFormattedCounterValue(*self.counters.get(at)?, FORMAT, None, &mut v) };
        // SAFETY: a zeroed or filled f64 either way; only used when PDH says it is valid.
        let value = unsafe { v.Anonymous.doubleValue };
        (status == 0 && v.CStatus == 0).then_some(value)
    }

    /// A wildcard counter's value per instance, `f` called with each name and value.
    pub fn each(&mut self, at: usize, mut f: impl FnMut(&str, f64)) {
        let Some(&counter) = self.counters.get(at) else {
            return;
        };
        let mut bytes = 0u32;
        let mut count = 0u32;
        let status = unsafe { PdhGetFormattedCounterArrayW(counter, FORMAT, &mut bytes, &mut count, None) };
        if status != PDH_MORE_DATA || bytes == 0 {
            return;
        }
        let words = bytes as usize / 8 + 1;
        if self.buf.len() < words {
            self.buf.resize(words, 0);
        }
        let items = self.buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
        let status = unsafe { PdhGetFormattedCounterArrayW(counter, FORMAT, &mut bytes, &mut count, Some(items)) };
        if status != 0 {
            return;
        }
        // SAFETY: PDH filled `count` items at the start of the buffer, their names after them.
        let items = unsafe { std::slice::from_raw_parts(items, count as usize) };
        let mut name = String::new();
        for item in items {
            if item.FmtValue.CStatus != 0 {
                continue;
            }
            name.clear();
            // SAFETY: each name is a terminated string inside the buffer.
            let wide = unsafe { item.szName.as_wide() };
            name.extend(char::decode_utf16(wide.iter().copied()).map(|c| c.unwrap_or('?')));
            f(&name, unsafe { item.FmtValue.Anonymous.doubleValue });
        }
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_wildcard_counter() {
        let mut q = Query::open().unwrap();
        let at = q.add(r"\Processor(*)\% Processor Time").unwrap();
        assert!(q.add(r"\No Such Object(*)\Nothing").is_none());
        q.collect();
        std::thread::sleep(std::time::Duration::from_millis(100));
        q.collect();
        let mut names = Vec::new();
        q.each(at, |n, v| {
            assert!(v >= 0.0);
            names.push(n.to_owned());
        });
        assert!(names.iter().any(|n| n == "_Total"), "{names:?}");
    }
}
