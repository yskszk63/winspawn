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
    pretty_env_logger::init();

    let (rxtheir, txme) = tokio_anon_pipe::anon_pipe_we_write().unwrap();
    let (rxme, txtheir) = tokio_anon_pipe::anon_pipe_we_read().unwrap();
    eprintln!("{:?}", rxtheir);
    eprintln!("{:?}", txtheir);

    let rxtheir = into_raw_handle(rxtheir);
    let txtheir = into_raw_handle(txtheir);

    let rxtheir = FileDescriptor::from_raw_handle(rxtheir, Mode::ReadOnly).unwrap();
    let txtheir = FileDescriptor::from_raw_handle(txtheir, Mode::ReadWrite).unwrap();

    let mut prog = swap_fd(&rxtheir, 3, |_| {
        swap_fd(&txtheir, 4, |_| {
            eprintln!("spawn");
            spawn("python", ["./test.py"])
        })
    })
    .unwrap();
    drop(rxtheir);
    drop(txtheir);

    eprintln!("connect");
    let mut txme = txme.connect().await.unwrap();
    let mut rxme = rxme.connect().await.unwrap();

    eprintln!("write");
    txme.write_all(b"Hello").await.unwrap();
    eprintln!("wrote");
    txme.shutdown().await.unwrap();
    drop(txme);

    eprintln!("read");
    let mut buf = vec![];
    rxme.read_to_end(&mut buf).await.unwrap();
    assert_eq!(b"Hello".as_ref(), &buf);
    eprintln!("OK");

    let exitcode = prog.wait().unwrap();
    assert_eq!(0, exitcode);
}
