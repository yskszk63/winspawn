fn main() {
    windows::build! {
        Windows::Win32::System::Threading::{WaitForSingleObject, GetExitCodeProcess, RegisterWaitForSingleObject},
        Windows::Win32::System::WindowsProgramming::INFINITE,
    };
}
