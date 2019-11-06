pub const LIBURING_UDATA_TIMEOUT: libc::__u64 = libc::__u64::max_value();

#[repr(C)]
pub struct io_uring {
    pub sq: io_uring_sq,
    pub cq: io_uring_cq,
    pub flags: libc::c_uint,
    pub ring_fd: libc::c_int,
}

#[repr(C)]
pub struct io_uring_sq {
    pub khead: *mut libc::c_uint,
    pub ktail: *mut libc::c_uint,
    pub kring_mask: *mut libc::c_uint,
    pub kring_entries: *mut libc::c_uint,
    pub kflags: *mut libc::c_uint,
    pub kdropped: *mut libc::c_uint,
    pub array: *mut libc::c_uint,
    pub sqes: *mut io_uring_sqe,

    pub sqe_head: libc::c_uint,
    pub sqe_tail: libc::c_uint,

    pub ring_sz: libc::size_t,
    pub ring_ptr: *mut libc::c_void,
}

#[repr(C)]
pub struct io_uring_sqe {
    pub opcode: libc::__u8,     /* type of operation for this sqe */
    pub flags: libc::__u8,      /* IOSQE_ flags */
    pub ioprio: libc::__u16,    /* ioprio for the request */
    pub fd: libc::__s32,        /* file descriptor to do IO on */
    pub off_addr2: off_addr2,
    pub addr: libc::__u64,      /* pointer to buffer or iovecs */
    pub len: libc::__u32,       /* buffer size or number of iovecs */
    pub cmd_flags: cmd_flags,
    pub user_data: libc::__u64, /* data to be passed back at completion time */
    pub buf_index: buf_index,   /* index into fixed buffers, if used */
}

#[repr(C)]
pub union off_addr2 {
    pub off: libc::__u64,
    pub addr2: libc::__u64,
}

#[repr(C)]
pub union cmd_flags {
    pub rw_flags: __kernel_rwf_t,
    pub fsync_flags: libc::__u32,
    pub poll_events: libc::__u16,
    pub sync_range_flags: libc::__u32,
    pub msg_flags: libc::__u32,
    pub timeout_flags: libc::__u32,
    pub accept_flags: libc::__u32,
    pub cancel_flags: libc::__u32,
}

#[allow(non_camel_case_types)]
type __kernel_rwf_t = libc::c_int;

#[repr(C)]
pub union buf_index {
    pub buf_index: libc::__u16,
    pub __pad2: [libc::__u64; 3],
}

#[repr(C)]
pub struct io_uring_cq {
    pub khead: *mut libc::c_uint,
    pub ktail: *mut libc::c_uint,
    pub kring_mask: *mut libc::c_uint,
    pub kring_entries: *mut libc::c_uint,
    pub koverflow: *mut libc::c_uint,
    pub cqes: *mut io_uring_cqe,

    pub ring_sz: libc::size_t,
    pub ring_ptr: *mut libc::c_void,
}

#[repr(C)]
pub struct io_uring_cqe {
    pub user_data: libc::__u64, /* sqe->data submission passed back */
    pub res: libc::__s32,       /* result code for this event */
    pub flags: libc::__u32,
}

#[repr(C)]
pub struct io_uring_params {
    pub sq_entries: libc::__u32,
    pub cq_entries: libc::__u32,
    pub flags: libc::__u32,
    pub sq_thread_cpu: libc::__u32,
    pub sq_thread_idle: libc::__u32,
    pub features: libc::__u32,
    pub resv: [libc::__u32; 4],
    pub sq_off: io_sqring_offsets,
    pub cq_off: io_cqring_offsets,
}

#[repr(C)]
pub struct io_sqring_offsets {
    pub head: libc::__u32,
    pub tail: libc::__u32,
    pub ring_mask: libc::__u32,
    pub ring_entries: libc::__u32,
    pub flags: libc::__u32,
    pub dropped: libc::__u32,
    pub array: libc::__u32,
    pub resv1: libc::__u32,
    pub resv2: libc::__u64,
}

#[repr(C)]
pub struct io_cqring_offsets {
    pub head: libc::__u32,
    pub tail: libc::__u32,
    pub ring_mask: libc::__u32,
    pub ring_entries: libc::__u32,
    pub overflow: libc::__u32,
    pub cqes: libc::__u32,
    pub resv: [libc::__u64; 2],
}

#[repr(C)]
pub struct __kernel_timespec {
    pub tv_sec: i64,
    pub tv_nsec: libc::c_longlong,
}

#[link(name = "uring")]
extern {
    pub fn io_uring_queue_init(
        entries: libc::c_uint,
        ring: *mut io_uring,
        flags: libc::c_uint,
    ) -> libc::c_int;

    pub fn io_uring_queue_init_params(
        entries: libc::c_uint,
        ring: *mut io_uring,
        params: *mut io_uring_params,
    ) -> libc::c_int;

    pub fn io_uring_queue_mmap(
        fd: libc::c_int,
        params: *mut io_uring_params,
        ring: *mut io_uring,
    ) -> libc::c_int;

    pub fn io_uring_queue_exit(ring: *mut io_uring);

    pub fn io_uring_peek_batch_cqe(
        ring: *mut io_uring,
        cqes: *mut *mut io_uring_cqe,
        count: libc::c_uint
    ) -> libc::c_uint;

    pub fn io_uring_wait_cqes(
        ring: *mut io_uring,
        cqe_ptr: *mut *mut io_uring_cqe,
        wait_nr: libc::c_uint,
        ts: *const __kernel_timespec,
        sigmask: *const libc::sigset_t
    ) -> libc::c_int;

    pub fn io_uring_wait_cqe_timeout(
        ring: *mut io_uring,
        cqe_ptr: *mut *mut io_uring_cqe,
        ts: *mut __kernel_timespec
    ) -> libc::c_int;

    pub fn io_uring_submit(ring: *mut io_uring) -> libc::c_int;

    pub fn io_uring_submit_and_wait(ring: *mut io_uring, wait_nr: libc::c_uint) -> libc::c_int;

    pub fn io_uring_get_sqe(ring: *mut io_uring) -> *mut io_uring_sqe;

    pub fn io_uring_register_buffers(
        ring: *mut io_uring,
        iovecs: *const libc::iovec,
        nr_iovecs: libc::c_uint,
    ) -> libc::c_int;

    pub fn io_uring_unregister_buffers(ring: *mut io_uring) -> libc::c_int;

    pub fn io_uring_register_files(
        ring: *mut io_uring,
        files: *const libc::c_int,
        nr_files: libc::c_uint,
    ) -> libc::c_int;

    pub fn io_uring_unregister_files(ring: *mut io_uring) -> libc::c_int;

    pub fn io_uring_register_files_update(
        ring: *mut io_uring,
        off: libc::c_uint,
        files: *const libc::c_int,
        nr_files: libc::c_uint,
    ) -> libc::c_int;

    pub fn io_uring_register_eventfd(ring: *mut io_uring, fd: libc::c_int) -> libc::c_int;

    pub fn io_uring_unregister_eventfd(ring: *mut io_uring) -> libc::c_int;
}

#[link(name = "iouc")]
extern {
    pub fn iouc_cqe_seen(ring: *mut io_uring);

    pub fn iouc_cqe_advance(ring: *mut io_uring, nr: libc::c_uint);
}
