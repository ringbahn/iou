// exhaust the SQ/CQ to prove that everything is working properly

#[test]
fn exhaust_queue_with_prepare_sqe() {
    let mut io_uring = iou::IoUring::new(8).unwrap();

    for counter in 0..64 {
        unsafe {
            let mut sqe = io_uring.prepare_sqe().unwrap();
            sqe.prep_nop();
            sqe.set_user_data(counter);
            io_uring.submit_sqes_and_wait(1).unwrap();
            let cqe = io_uring.peek_for_cqe().unwrap();
            assert_eq!(cqe.user_data(), counter);
        }
    }
}

#[test]
fn exhaust_queue_with_prepare_sqes() {
    let mut io_uring = iou::IoUring::new(8).unwrap();

    for base in (0..64).filter(|x| x % 4 == 0) {
        unsafe {
            let mut counter = base;
            let mut sqes = io_uring.prepare_sqes(4).unwrap();
            for mut sqe in sqes.hard_linked() {
                sqe.prep_nop();
                sqe.set_user_data(counter);
                counter += 1;
            }

            io_uring.submit_sqes_and_wait(4).unwrap();

            for counter in base..counter {
                let cqe = io_uring.peek_for_cqe().unwrap();
                assert_eq!(cqe.user_data(), counter);
            }
        }
    }
}
