# Interface to Linux's io_uring interface

`iou` is a wrapper around the [liburing][liburing] library, which provides a
high level interface to Linux's new [io_uring][io_uring] interface. It is
intended to be extensible and flexible for any use case of io_uring, while
still resolving many of the basic safety issues on users' behalf.

The primary API of iou is the `IoUring` type and its components, the
`SubmissionQueue`, `CompletionQueue` and `Registrar`. This provides a Rust-like
and high level API that manages the io_uring for you.

## Safety

Most of the APIs in iou are safe, and many of the safety issues in using
io_uring are completely resolved. In particular, the liburing library which iou
is based on correctly implements the atomics necessary to coordinate with the
kernel across the io_uring interface. However, some key interfaces remain
unsafe. In particular, preparing IO events to be submitted to the io_uring is
not safe: users must ensure that the buffers and file descriptors are regarded
as borrowed during the lifetime of the IO.

[io_uring]: http://kernel.dk/io_uring.pdf
[liburing]: http://git.kernel.dk/cgit/liburing/

## Kernel support

In order to use io_uring, the machine you are running your code on must have a
kernel which supports that interface. The first version of io_uring was added
in Linux 5.1, but it did not include all of the features supported by this
library. Some features of this library may not work depending on which version
of Linux you are using.
