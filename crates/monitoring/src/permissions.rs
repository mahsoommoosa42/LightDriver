//! Process token checks; no account, policy or driver changes.
pub fn is_administrator() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Security::{
            AllocateAndInitializeSid, CheckTokenMembership, FreeSid, SID_IDENTIFIER_AUTHORITY,
        };
        let authority = SID_IDENTIFIER_AUTHORITY {
            Value: [0, 0, 0, 0, 0, 5],
        };
        let mut sid = std::ptr::null_mut();
        // SAFETY: constructs the well-known built-in Administrators SID, reads the
        // calling process token and frees only the SID allocated by this function.
        unsafe {
            if AllocateAndInitializeSid(&authority, 2, 32, 544, 0, 0, 0, 0, 0, 0, &mut sid) == 0 {
                return Err("Unable to query administrator permissions".into());
            }
            let mut member = 0;
            let success = CheckTokenMembership(std::ptr::null_mut(), sid, &mut member);
            FreeSid(sid);
            if success == 0 {
                Err("Unable to query administrator permissions".into())
            } else {
                Ok(member != 0)
            }
        }
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}
