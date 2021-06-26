use std::ffi::c_void;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use winspawn::{spawn, swap_fd, FileDescriptor, Mode};

type HANDLE = *mut c_void;

#[cfg(windows)]
fn into_raw_handle<P>(p: P) -> HANDLE
where
    P: std::os::windows::io::AsRawHandle,
{
    use std::mem;
    use std::os::windows::io::AsRawHandle;

    let r = p.as_raw_handle();
    mem::forget(p);
    r
}

#[cfg(not(windows))]
fn into_raw_handle<P>(_: P) -> HANDLE {
    panic!("stub")
}

#[tokio::test]
async fn test_simple() {
    let (rxtheir, mut txme) = tokio_anon_pipe::anon_pipe().await.unwrap();
    let (mut rxme, txtheir) = tokio_anon_pipe::anon_pipe().await.unwrap();

    let rxtheir = into_raw_handle(rxtheir);
    let txtheir = into_raw_handle(txtheir);

    let rxtheir = FileDescriptor::from_raw_handle(rxtheir, Mode::ReadOnly).unwrap();
    let txtheir = FileDescriptor::from_raw_handle(txtheir, Mode::ReadWrite).unwrap();

    let mut prog = swap_fd(rxtheir, 3, |_| {
        swap_fd(txtheir, 4, |_| spawn("python", ["./simple.rs"]))
    }).unwrap();

    txme.write_all(b"Hello").await.unwrap();
    drop(txme);

    let mut buf = vec![];
    rxme.read_to_end(&mut buf).await.unwrap();
    assert_eq!(b"Hello".as_ref(), &buf);

    let exitcode = prog.wait().unwrap();
    assert_eq!(0, exitcode);
}
