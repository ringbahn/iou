#include "liburing.h"

extern inline void iouc_cqe_advance(struct io_uring *ring, unsigned nr) {
    struct io_uring_cq *cq = &ring->cq;
    io_uring_smp_store_release(cq->khead, *cq->khead + nr);
}

extern inline void iouc_cqe_seen(struct io_uring *ring) {
    iouc_cqe_advance(ring, 1);
}
