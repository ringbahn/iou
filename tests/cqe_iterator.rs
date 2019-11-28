use std::io;

#[test]
fn iterator_test() -> io::Result<()> {
    unsafe {
        let mut io_uring = iou::IoUring::new(32)?;

        let mut sqe = io_uring.next_sqe().unwrap();
        sqe.prep_nop();
        sqe.set_user_data(0x01);

        let mut sqe = io_uring.next_sqe().unwrap();
        sqe.prep_nop();
        sqe.set_user_data(0x02);

        let mut sqe = io_uring.next_sqe().unwrap();
        sqe.prep_nop();
        sqe.set_user_data(0x03);

        io_uring.submit_sqes()?;

        let mut user_datas = [0x01, 0x02, 0x03];
        io_uring.wait_for_cqes(3).unwrap().for_each(|cqe| {

            let ud = user_datas.iter_mut().find(|&&mut ud| cqe.user_data() == ud)
                                          .expect("received unexpected CQE");

            if *ud == 0 { panic!("received same CQE more than once") } else { *ud = 0; }

        });

        assert_eq!(user_datas, [0x00, 0x00, 0x00]);

        let mut count = 0;
        io_uring.peek_for_cqes().for_each(|_| count += 1);
        assert_eq!(count, 0, "all CQEs should be processed");

        Ok(())
    }
}
