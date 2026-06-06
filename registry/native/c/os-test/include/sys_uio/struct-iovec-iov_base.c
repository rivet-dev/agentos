/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/uio.h>
void foo(struct iovec* bar)
{
	void **qux = &bar->iov_base;
	(void) qux;
}
int main(void) { return 0; }
