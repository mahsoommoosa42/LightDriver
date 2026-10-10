//! Windows IPC grants only the current account, administrators and SYSTEM access.
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use std::{io, ptr};
use widestring::U16CString;
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::ConvertSidToStringSidW, GetTokenInformation, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
pub(crate) fn descriptor() -> io::Result<SecurityDescriptor> {
    let sid = current_user_sid()?;
    // Medium integrity permits the ordinary UI to write requests to an elevated
    // service. There is no Everyone/anonymous ACE or privileged hardware command.
    let text = U16CString::from_str(format!(
        "D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{sid})S:(ML;;NW;;;ME)"
    ))
    .map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(text.as_ucstr())
}
fn current_user_sid() -> io::Result<String> {
    let mut token = ptr::null_mut();
    // SAFETY: own process token, read-only TOKEN_QUERY, and bounded aligned output.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut size = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut size);
        }
        if !(std::mem::size_of::<TOKEN_USER>() as u32..=16 * 1024).contains(&size) {
            return Err(io::Error::other("Invalid token information size"));
        }
        let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                size,
                &mut size,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = unsafe { &*buffer.as_ptr().cast::<TOKEN_USER>() };
        let mut text = ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let converted = (|| {
            let mut units = vec![];
            for i in 0..184 {
                let unit = unsafe { *text.add(i) };
                if unit == 0 {
                    return String::from_utf16(&units).map_err(io::Error::other);
                }
                units.push(unit);
            }
            Err(io::Error::other("SID text exceeded its maximum length"))
        })();
        unsafe {
            LocalFree(text.cast());
        }
        converted
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn account_security_descriptor_is_valid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"));
        descriptor().unwrap();
    }
}
