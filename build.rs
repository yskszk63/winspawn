fn main() {
    windows::build! {
        Windows::Win32::System::Threading::{WaitForSingleObject, GetExitCodeProcess},
        Windows::Win32::System::WindowsProgramming::INFINITE,
    };
}
