/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <sys/resource.h>
void foo(struct rusage* bar)
{
	struct timeval *qux = &bar->ru_stime;
	(void) qux;
}
int main(void) { return 0; }
