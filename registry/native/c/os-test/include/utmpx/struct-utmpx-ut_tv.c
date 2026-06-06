/*[XSI]*/
#if 202405L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 800
#elif 200809L <= _POSIX_C_SOURCE
#define _XOPEN_SOURCE 700
#endif
#include <utmpx.h>
void foo(struct utmpx* bar)
{
	struct timeval *qux = &bar->ut_tv;
	(void) qux;
}
int main(void) { return 0; }
