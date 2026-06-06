/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/uio.h>
void foo(struct iovec* bar)
{
	size_t *qux = &bar->iov_len;
	(void) qux;
}
int main(void) { return 0; }
