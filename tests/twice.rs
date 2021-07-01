use tokio::io::{AsyncReadExt, AsyncWriteExt};
use winspawn::{move_fd, spawn, FileDescriptor, Mode};

#[tokio::test]
async fn test_twice() {
    pretty_env_logger::init();

    proc().await;
    proc().await;
}

async fn proc() {
    let (rxtheir, mut txme) = tokio_anon_pipe::anon_pipe().await.unwrap();
    let (mut rxme, txtheir) = tokio_anon_pipe::anon_pipe().await.unwrap();
    eprintln!("{:?}", rxtheir);
    eprintln!("{:?}", txtheir);

    let rxtheir = FileDescriptor::from_raw_handle(rxtheir, Mode::ReadOnly).unwrap();
    let txtheir = FileDescriptor::from_raw_handle(txtheir, Mode::ReadWrite).unwrap();

    let prog = move_fd(&rxtheir, 3, |_| {
        move_fd(&txtheir, 4, |_| {
            eprintln!("spawn");
            spawn("python", ["./tests/test.py"])
        })
    })
    .unwrap();
    drop(rxtheir);
    drop(txtheir);

    // poll process exit
    let task = tokio::spawn(prog);

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

    let exitcode = task.await.unwrap().unwrap();
    assert_eq!(0, exitcode);
}
