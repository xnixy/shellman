//! Turning a sound device on or off, as the Sound applet's Enable and Disable do: the audio
//! service hides or shows the endpoint through `IPolicyConfig::SetEndpointVisibility`. Windows
//! may let only administrators do it, so a normal run can be refused with access denied; the page
//! then asks the elevated helper (`crate::elevate`) to make the same call.

use super::audio::{init_com, policy, POLICY_CONFIG_CLIENT};
use windows::core::PCWSTR;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};

/// E_ACCESSDENIED, as an HRESULT value.
pub const ACCESS_DENIED: i32 = 0x8007_0005_u32 as i32;

/// Why a change did not happen, as an HRESULT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused(pub i32);

impl Refused {
    /// Whether an administrator could do it instead.
    pub fn needs_admin(self) -> bool {
        self.0 == ACCESS_DENIED
    }
}

/// Show (`enabled`) or hide the endpoint `id`.
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), Refused> {
    init_com();
    let wide: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the interface is created by its own class; `wide` outlives the call.
    unsafe {
        let policy = CoCreateInstance::<_, policy::IPolicyConfig>(&POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)
            .map_err(|e| Refused(e.code().0))?;
        let hr = policy::set_visibility(&policy, PCWSTR(wide.as_ptr()), i32::from(enabled));
        if hr.is_ok() {
            Ok(())
        } else {
            Err(Refused(hr.0))
        }
    }
}

/// What to do after a direct attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Done,
    /// Ask the elevated helper.
    Elevate,
    Fail(String),
}

/// The step after `attempt`: access denied goes to the helper, any other refusal is final.
pub fn next_step(attempt: Result<(), Refused>) -> Next {
    match attempt {
        Ok(()) => Next::Done,
        Err(r) if r.needs_admin() => Next::Elevate,
        Err(Refused(hr)) => Next::Fail(format!("Windows did not allow the change (0x{:08X}).", hr as u32)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_attempt_goes_on_only_when_access_was_denied() {
        assert_eq!(next_step(Ok(())), Next::Done);
        assert_eq!(next_step(Err(Refused(ACCESS_DENIED))), Next::Elevate);
        assert!(matches!(next_step(Err(Refused(0x8007_0490_u32 as i32))), Next::Fail(m) if m.contains("80070490")));
    }

    #[test]
    fn only_access_denied_asks_for_an_administrator() {
        assert!(Refused(ACCESS_DENIED).needs_admin());
        assert!(!Refused(0x8000_4005_u32 as i32).needs_admin());
        assert!(!Refused(0x8007_0490_u32 as i32).needs_admin());
    }

    /// An unelevated "show" on the device named by SHELLMAN_TEST_ENDPOINT, which must already be
    /// shown (so nothing changes); says whether Windows lets a normal run do it. By hand only.
    #[test]
    #[ignore]
    fn unelevated_show_of_a_shown_device() {
        let id = std::env::var("SHELLMAN_TEST_ENDPOINT").expect("set SHELLMAN_TEST_ENDPOINT");
        let r = set_enabled(&id, true);
        println!("{r:?} -> {:?} (access denied is {ACCESS_DENIED:#x})", next_step(r));
    }

    /// Turns the output named by SHELLMAN_TEST_ENDPOINT off and on again without elevation and
    /// checks the device list is the same afterwards. Never run it on a default device. By hand
    /// only; if the first step is refused nothing was changed.
    #[test]
    #[ignore]
    fn unelevated_off_and_on_again() {
        use crate::system::sound_dev::{list, list_disabled, Flow};
        let id = std::env::var("SHELLMAN_TEST_ENDPOINT").expect("set SHELLMAN_TEST_ENDPOINT");
        crate::system::live::init_mta();
        let en = crate::system::audio::enumerator().unwrap();
        let ids = |en: &_| list(en, Flow::Output).into_iter().map(|d| d.id).collect::<Vec<_>>();
        let before = ids(&en);
        assert!(before.contains(&id));
        let off = set_enabled(&id, false);
        println!("disable: {off:?} -> {:?}", next_step(off));
        let mid = ids(&en);
        println!("listed while off: {}, in disabled group: {}", mid.contains(&id), list_disabled(&en, Flow::Output).iter().any(|d| d.id == id));
        let on = set_enabled(&id, true);
        println!("enable: {on:?}");
        std::thread::sleep(std::time::Duration::from_millis(500));
        println!("list equals the one before: {}", ids(&en) == before);
    }

    /// What Windows says to an unelevated call, on an id that names no device (so nothing can
    /// change); by hand only.
    #[test]
    #[ignore]
    fn unelevated_answer_for_a_made_up_device() {
        let r = set_enabled("{0.0.0.00000000}.{00000000-0000-0000-0000-000000000000}", true);
        println!("{r:?} (access denied is {ACCESS_DENIED:#x})");
    }
}
