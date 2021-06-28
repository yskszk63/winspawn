fn main() {
    windows::build! {
        Windows::Win32::Foundation::INVALID_HANDLE_VALUE,
        Windows::Win32::System::Threading::{
            WaitForSingleObject,
            GetExitCodeProcess,
            RegisterWaitForSingleObject,
            UnregisterWaitEx,
            InitializeSRWLock,
            AcquireSRWLockExclusive,
            ReleaseSRWLockExclusive,
            TerminateProcess,
        },
        Windows::Win32::System::WindowsProgramming::INFINITE,
    };
}
