#[test]
fn register_buffers_by_val() {
    let buf1 = vec![0; 1024].into_boxed_slice();
    let buf2 = vec![0; 1024].into_boxed_slice();
    let ring = iou::IoUring::new(8).unwrap();
    let bufs: Vec<_> = ring.registrar()
                           .register_buffers(vec![buf1, buf2])
                           .unwrap().collect();
    assert_eq!(bufs.len(), 2);
    assert_eq!(bufs[0].index(), 0);
    assert_eq!(bufs[1].index(), 1);
}

#[test]
fn register_buffers_by_ref() {
    let buf1 = vec![0; 1024];
    let buf2 = vec![0; 1024];
    let ring = iou::IoUring::new(8).unwrap();
    let bufs = &[&buf1[..], &buf2[..]];
    let bufs: Vec<_> = ring.registrar()
                           .register_buffers_by_ref(bufs)
                           .unwrap().collect();
    assert_eq!(bufs.len(), 2);
    assert_eq!(bufs[0].index(), 0);
    assert_eq!(bufs[1].index(), 1);
}

#[test]
fn register_buffers_by_mut() {
    let mut buf1 = vec![0; 1024];
    let mut buf2 = vec![0; 1024];
    let ring = iou::IoUring::new(8).unwrap();
    let bufs = &mut [&mut buf1[..], &mut buf2[..]];
    let bufs: Vec<_> = ring.registrar()
                           .register_buffers_by_mut(bufs)
                           .unwrap().collect();
    assert_eq!(bufs.len(), 2);
    assert_eq!(bufs[0].index(), 0);
    assert_eq!(bufs[1].index(), 1);
}
