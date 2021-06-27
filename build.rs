fn main() {
    windows::build! {
        Windows::Win32::Foundation::INVALID_HANDLE_VALUE,
        Windows::Win32::System::Threading::{
            WaitForSingleObject,
            GetExitCodeProcess,
            RegisterWaitForSingleObject,
            UnregisterWaitEx
        },
        Windows::Win32::System::WindowsProgramming::INFINITE,
    };
}
