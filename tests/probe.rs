use iou::Probe;
use uring_sys::IoRingOp;

#[test]
fn probe() {
    let probe = Probe::new().unwrap();
    assert!(probe.supports(IoRingOp::IORING_OP_NOP));
}
